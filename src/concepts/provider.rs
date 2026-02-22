use serde::{Deserialize, Serialize};

use crate::error::LairesError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider_type: ProviderType,
    pub model: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderType {
    OpenAiCompatible,
    Anthropic,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Error(String),
}

#[derive(Debug, Default)]
pub struct UsageMetrics {
    pub total_tokens: u64,
    pub total_requests: u64,
    pub total_latency_ms: u64,
}

/// A message in the LLM conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_results: Option<Vec<ToolResult>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// LLM response from a completion request
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: ResponseUsage,
}

#[derive(Debug, Clone, Default)]
pub struct ResponseUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

pub struct Provider {
    config: ProviderConfig,
    connection_status: ConnectionStatus,
    is_local: bool,
    metrics: UsageMetrics,
    client: reqwest::Client,
}

impl Provider {
    pub fn new(config: ProviderConfig) -> Self {
        let is_local = matches!(config.provider_type, ProviderType::Local)
            || config.base_url.contains("localhost")
            || config.base_url.contains("127.0.0.1");

        Self {
            config,
            connection_status: ConnectionStatus::Disconnected,
            is_local,
            metrics: UsageMetrics::default(),
            client: reqwest::Client::new(),
        }
    }

    /// Create a provider from project config
    pub fn from_project_config(
        config: &crate::config::ProjectConfig,
    ) -> Result<Self, LairesError> {
        let provider_type = match config.llm.provider.as_str() {
            "anthropic" => ProviderType::Anthropic,
            "openai-compatible" | "openai" | "gemini" => {
                ProviderType::OpenAiCompatible
            }
            "local" => ProviderType::Local,
            other => {
                return Err(LairesError::Provider(format!(
                    "Unknown provider type: {other}"
                )))
            }
        };

        let base_url = config
            .llm
            .base_url
            .clone()
            .unwrap_or_else(|| match provider_type {
                ProviderType::Anthropic => {
                    "https://api.anthropic.com".to_string()
                }
                ProviderType::OpenAiCompatible => {
                    "https://api.openai.com/v1".to_string()
                }
                ProviderType::Local => {
                    "http://localhost:11434/v1".to_string()
                }
            });

        let provider_config = ProviderConfig {
            provider_type,
            model: config.llm.model.clone(),
            base_url,
            api_key_env: config.llm.api_key_env.clone(),
        };

        Ok(Self::new(provider_config))
    }

    /// Get the API key from the environment
    fn api_key(&self) -> Option<String> {
        self.config
            .api_key_env
            .as_ref()
            .and_then(|env_var| std::env::var(env_var).ok())
    }

    /// Make a completion request to the LLM (Anthropic Messages API)
    pub async fn complete(
        &mut self,
        messages: &[Message],
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
    ) -> Result<LlmResponse, LairesError> {
        match self.config.provider_type {
            ProviderType::Anthropic => {
                self.complete_anthropic(messages, tools, system_prompt)
                    .await
            }
            ProviderType::OpenAiCompatible | ProviderType::Local => {
                self.complete_openai(messages, tools, system_prompt).await
            }
        }
    }

    async fn complete_anthropic(
        &mut self,
        messages: &[Message],
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
    ) -> Result<LlmResponse, LairesError> {
        let api_key = self.api_key().ok_or_else(|| {
            LairesError::Provider(
                "API key not found. Set the environment variable specified in config.".to_string(),
            )
        })?;

        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| format_anthropic_message(m))
            .collect();

        let mut body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": 4096,
            "messages": api_messages,
        });

        if let Some(sys) = system_prompt {
            body["system"] = serde_json::json!(sys);
        }

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools.iter().map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            }).collect::<Vec<_>>());
        }

        let url = format!("{}/v1/messages", self.config.base_url);

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LairesError::Provider(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read response".to_string());
            return Err(LairesError::Provider(format!(
                "Anthropic API error ({status}): {text}"
            )));
        }

        let resp_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LairesError::Provider(e.to_string()))?;

        self.metrics.total_requests += 1;
        self.connection_status = ConnectionStatus::Connected;

        // Parse response
        let mut content = None;
        let mut tool_calls = Vec::new();

        if let Some(content_blocks) = resp_json["content"].as_array() {
            for block in content_blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        content = block["text"].as_str().map(|s| s.to_string());
                    }
                    Some("tool_use") => {
                        tool_calls.push(ToolCall {
                            id: block["id"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            name: block["name"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            arguments: block["input"].clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        let usage = ResponseUsage {
            prompt_tokens: resp_json["usage"]["input_tokens"]
                .as_u64()
                .unwrap_or(0),
            completion_tokens: resp_json["usage"]["output_tokens"]
                .as_u64()
                .unwrap_or(0),
        };

        self.metrics.total_tokens += usage.prompt_tokens + usage.completion_tokens;

        Ok(LlmResponse {
            content,
            tool_calls,
            usage,
        })
    }

    async fn complete_openai(
        &mut self,
        messages: &[Message],
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
    ) -> Result<LlmResponse, LairesError> {
        let mut api_messages: Vec<serde_json::Value> = Vec::new();

        if let Some(sys) = system_prompt {
            api_messages.push(serde_json::json!({
                "role": "system",
                "content": sys,
            }));
        }

        for m in messages {
            api_messages.extend(format_openai_message(m));
        }

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": api_messages,
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools.iter().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            }).collect::<Vec<_>>());
        }

        let url = format!("{}/chat/completions", self.config.base_url);

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        if let Some(api_key) = self.api_key() {
            req = req.header("authorization", format!("Bearer {api_key}"));
        }

        let response = req
            .json(&body)
            .send()
            .await
            .map_err(|e| LairesError::Provider(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read response".to_string());
            return Err(LairesError::Provider(format!(
                "OpenAI API error ({status}): {text}"
            )));
        }

        let resp_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LairesError::Provider(e.to_string()))?;

        self.metrics.total_requests += 1;
        self.connection_status = ConnectionStatus::Connected;

        let choice = &resp_json["choices"][0]["message"];
        let content = choice["content"].as_str().map(|s| s.to_string());

        let tool_calls = choice["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|tc| {
                        Some(ToolCall {
                            id: tc["id"].as_str()?.to_string(),
                            name: tc["function"]["name"].as_str()?.to_string(),
                            arguments: serde_json::from_str(
                                tc["function"]["arguments"].as_str()?,
                            )
                            .ok()?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let usage = ResponseUsage {
            prompt_tokens: resp_json["usage"]["prompt_tokens"]
                .as_u64()
                .unwrap_or(0),
            completion_tokens: resp_json["usage"]["completion_tokens"]
                .as_u64()
                .unwrap_or(0),
        };

        self.metrics.total_tokens += usage.prompt_tokens + usage.completion_tokens;

        Ok(LlmResponse {
            content,
            tool_calls,
            usage,
        })
    }

    pub fn is_local(&self) -> bool {
        self.is_local
    }

    pub fn connection_status(&self) -> &ConnectionStatus {
        &self.connection_status
    }

    pub fn model_name(&self) -> &str {
        &self.config.model
    }

    pub fn metrics(&self) -> &UsageMetrics {
        &self.metrics
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }
}

/// Format a Message for the Anthropic Messages API.
/// Assistant messages with tool_calls become content blocks with tool_use entries.
/// User messages with tool_results become content blocks with tool_result entries.
fn format_anthropic_message(m: &Message) -> serde_json::Value {
    let role = match m.role {
        Role::User | Role::Tool | Role::System => "user",
        Role::Assistant => "assistant",
    };

    // Assistant message with tool calls -> content blocks
    if m.role == Role::Assistant {
        if let Some(tool_calls) = &m.tool_calls {
            let mut content_blocks = Vec::new();
            if !m.content.is_empty() {
                content_blocks.push(serde_json::json!({
                    "type": "text",
                    "text": m.content,
                }));
            }
            for tc in tool_calls {
                content_blocks.push(serde_json::json!({
                    "type": "tool_use",
                    "id": tc.id,
                    "name": tc.name,
                    "input": tc.arguments,
                }));
            }
            return serde_json::json!({
                "role": "assistant",
                "content": content_blocks,
            });
        }
    }

    // User message with tool results -> content blocks
    if let Some(tool_results) = &m.tool_results {
        if !tool_results.is_empty() {
            let content_blocks: Vec<serde_json::Value> = tool_results
                .iter()
                .map(|tr| {
                    serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tr.tool_call_id,
                        "content": serde_json::to_string(&tr.result).unwrap_or_default(),
                    })
                })
                .collect();
            return serde_json::json!({
                "role": "user",
                "content": content_blocks,
            });
        }
    }

    // Plain text message
    serde_json::json!({
        "role": role,
        "content": m.content,
    })
}

/// Format a Message for the OpenAI Chat Completions API.
/// Assistant messages with tool_calls use the tool_calls array.
/// Tool results become separate messages with role "tool".
fn format_openai_message(m: &Message) -> Vec<serde_json::Value> {
    let role = match m.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
        Role::System => "system",
    };

    // Assistant message with tool calls
    if m.role == Role::Assistant {
        if let Some(tool_calls) = &m.tool_calls {
            let tc_array: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default(),
                        }
                    })
                })
                .collect();
            let mut msg = serde_json::json!({
                "role": "assistant",
                "tool_calls": tc_array,
            });
            if !m.content.is_empty() {
                msg["content"] = serde_json::json!(m.content);
            }
            return vec![msg];
        }
    }

    // Tool results -> separate messages with role "tool"
    if let Some(tool_results) = &m.tool_results {
        if !tool_results.is_empty() {
            return tool_results
                .iter()
                .map(|tr| {
                    serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tr.tool_call_id,
                        "content": serde_json::to_string(&tr.result).unwrap_or_default(),
                    })
                })
                .collect();
        }
    }

    // Plain text message
    vec![serde_json::json!({
        "role": role,
        "content": m.content,
    })]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_plain_message() {
        let msg = Message {
            role: Role::User,
            content: "Hello".to_string(),
            tool_calls: None,
            tool_results: None,
        };
        let result = format_anthropic_message(&msg);
        assert_eq!(result["role"], "user");
        assert_eq!(result["content"], "Hello");
    }

    #[test]
    fn test_anthropic_tool_use_message() {
        let msg = Message {
            role: Role::Assistant,
            content: "Let me check.".to_string(),
            tool_calls: Some(vec![ToolCall {
                id: "tc_1".to_string(),
                name: "read_scene".to_string(),
                arguments: serde_json::json!({"scene": "1"}),
            }]),
            tool_results: None,
        };
        let result = format_anthropic_message(&msg);
        assert_eq!(result["role"], "assistant");
        let content = result["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["name"], "read_scene");
    }

    #[test]
    fn test_anthropic_tool_result_message() {
        let msg = Message {
            role: Role::User,
            content: String::new(),
            tool_calls: None,
            tool_results: Some(vec![ToolResult {
                tool_call_id: "tc_1".to_string(),
                name: "read_scene".to_string(),
                result: serde_json::json!({"text": "Scene content"}),
            }]),
        };
        let result = format_anthropic_message(&msg);
        assert_eq!(result["role"], "user");
        let content = result["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "tc_1");
    }

    #[test]
    fn test_openai_plain_message() {
        let msg = Message {
            role: Role::User,
            content: "Hello".to_string(),
            tool_calls: None,
            tool_results: None,
        };
        let result = format_openai_message(&msg);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "user");
        assert_eq!(result[0]["content"], "Hello");
    }

    #[test]
    fn test_openai_tool_calls_message() {
        let msg = Message {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_string(),
                name: "query_graph".to_string(),
                arguments: serde_json::json!({"node_type": "character"}),
            }]),
            tool_results: None,
        };
        let result = format_openai_message(&msg);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "assistant");
        let tc = result[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tc[0]["function"]["name"], "query_graph");
    }

    #[test]
    fn test_openai_tool_results_message() {
        let msg = Message {
            role: Role::User,
            content: String::new(),
            tool_calls: None,
            tool_results: Some(vec![
                ToolResult {
                    tool_call_id: "call_1".to_string(),
                    name: "query_graph".to_string(),
                    result: serde_json::json!({"nodes": []}),
                },
                ToolResult {
                    tool_call_id: "call_2".to_string(),
                    name: "read_scene".to_string(),
                    result: serde_json::json!({"text": "content"}),
                },
            ]),
        };
        let result = format_openai_message(&msg);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["role"], "tool");
        assert_eq!(result[0]["tool_call_id"], "call_1");
        assert_eq!(result[1]["role"], "tool");
        assert_eq!(result[1]["tool_call_id"], "call_2");
    }
}
