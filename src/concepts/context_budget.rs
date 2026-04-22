use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph};
use crate::concepts::provider::{Message, Role, ToolSchema};

/// Approximate token count using chars/4 heuristic.
/// Good enough for budget tracking — not billing-accurate.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Breakdown of estimated token usage for a single LLM request.
#[derive(Debug, Clone, Default)]
pub struct ContextReport {
    pub system_prompt_tokens: usize,
    pub graph_tokens: usize,
    #[allow(dead_code)]
    pub divergence_tokens: usize,
    pub history_tokens: usize,
    #[allow(dead_code)]
    pub user_message_tokens: usize,
    pub tool_schema_tokens: usize,
    pub total_tokens: usize,
}

impl ContextReport {
    /// Build a report from the components of a chat request.
    pub fn from_chat_request(
        system_prompt: &str,
        graph_json: &str,
        divergence_json: &str,
        history: &[Message],
        user_message: &str,
        tool_schemas: &[ToolSchema],
    ) -> Self {
        let system_prompt_tokens = estimate_tokens(system_prompt);
        let graph_tokens = estimate_tokens(graph_json);
        let divergence_tokens = estimate_tokens(divergence_json);

        let history_tokens: usize = history
            .iter()
            .map(|m| {
                let mut size = estimate_tokens(&m.content);
                if let Some(tcs) = &m.tool_calls {
                    for tc in tcs {
                        size += estimate_tokens(&tc.name);
                        size += estimate_tokens(&tc.arguments.to_string());
                    }
                }
                if let Some(trs) = &m.tool_results {
                    for tr in trs {
                        size += estimate_tokens(&tr.result.to_string());
                    }
                }
                size
            })
            .sum();

        let user_message_tokens = estimate_tokens(user_message);

        let tool_schema_tokens: usize = tool_schemas
            .iter()
            .map(|t| {
                estimate_tokens(&t.name)
                    + estimate_tokens(&t.description)
                    + estimate_tokens(&t.parameters.to_string())
            })
            .sum();

        let total_tokens = system_prompt_tokens
            + graph_tokens
            + divergence_tokens
            + history_tokens
            + user_message_tokens
            + tool_schema_tokens;

        Self {
            system_prompt_tokens,
            graph_tokens,
            divergence_tokens,
            history_tokens,
            user_message_tokens,
            tool_schema_tokens,
            total_tokens,
        }
    }

    /// One-line summary suitable for logging or status bar display.
    pub fn summary(&self) -> String {
        format!(
            "~{}K tokens (graph: {}K, history: {}K, tools: {}K, system: {}K)",
            self.total_tokens / 1000,
            self.graph_tokens / 1000,
            self.history_tokens / 1000,
            self.tool_schema_tokens / 1000,
            self.system_prompt_tokens / 1000,
        )
    }
}

/// Compress older history messages into a summary, keeping the last `keep_recent` verbatim.
///
/// For each older user+assistant pair, extracts a brief summary line like
/// "user asked about X, assistant found Y". Returns a new Vec<Message> with
/// at most one summary message followed by the recent messages.
pub fn summarize_history(history: &[Message], keep_recent: usize) -> Vec<Message> {
    if history.len() <= keep_recent {
        return history.to_vec();
    }

    let older_count = history.len() - keep_recent;
    let older = &history[..older_count];
    let recent = &history[older_count..];

    // Build summary lines from older message pairs
    let mut summary_lines = Vec::new();
    let mut i = 0;
    while i < older.len() {
        let msg = &older[i];
        match msg.role {
            Role::User => {
                let user_brief = extract_key_phrase(&msg.content);
                // Check if next message is an assistant response
                if i + 1 < older.len() && older[i + 1].role == Role::Assistant {
                    let asst_brief = extract_key_phrase(&older[i + 1].content);
                    summary_lines.push(format!(
                        "- user asked about {user_brief}, assistant responded about {asst_brief}"
                    ));
                    i += 2;
                } else {
                    summary_lines.push(format!("- user asked about {user_brief}"));
                    i += 1;
                }
            }
            Role::Assistant => {
                let asst_brief = extract_key_phrase(&msg.content);
                summary_lines.push(format!("- assistant said: {asst_brief}"));
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    if summary_lines.is_empty() {
        return recent.to_vec();
    }

    let summary_text = format!(
        "Earlier conversation summary:\n{}",
        summary_lines.join("\n")
    );

    let mut result = vec![Message {
        role: Role::User,
        content: summary_text,
        tool_calls: None,
        tool_results: None,
    }];
    result.extend_from_slice(recent);
    result
}

/// Extract a short key phrase from a message (first sentence, truncated).
fn extract_key_phrase(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "(empty)".to_string();
    }

    // Take first sentence or first 80 chars, whichever is shorter
    let first_sentence = trimmed
        .split_once(['.', '?', '!'])
        .map(|(s, _)| s.trim())
        .unwrap_or(trimmed);

    if first_sentence.len() <= 80 {
        first_sentence.to_string()
    } else {
        format!("{}...", &first_sentence[..77])
    }
}

/// Extract character/scene references from user input by matching
/// against known node names and IDs in the graph. Returns matching node IDs.
pub fn extract_relevant_ids(user_input: &str, graph: &NarrativeGraph) -> Vec<String> {
    let input_lower = user_input.to_lowercase();
    let mut ids = Vec::new();

    for node in graph.get_characters() {
        if let GraphNode::Character {
            id, name, aliases, ..
        } = node
        {
            if name.len() >= 3 && input_lower.contains(&name.to_lowercase()) {
                ids.push(id.clone());
                continue;
            }
            for alias in aliases {
                if alias.len() >= 3 && input_lower.contains(&alias.to_lowercase()) {
                    ids.push(id.clone());
                    break;
                }
            }
        }
    }

    for node in graph.get_scenes() {
        if let GraphNode::Scene {
            id,
            title: Some(title),
            ..
        } = node
            && title.len() >= 3
            && input_lower.contains(&title.to_lowercase())
        {
            ids.push(id.clone());
        }
    }

    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        // 40 chars -> ~10 tokens
        assert_eq!(
            estimate_tokens("a]b\nc d\te fghijklmn opqrstuvwxyz1234567"),
            9
        );
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_context_report_basic() {
        let report = ContextReport::from_chat_request(
            "You are a helpful assistant.",
            r#"{"characters":[]}"#,
            "",
            &[],
            "Hello world",
            &[],
        );
        assert!(report.system_prompt_tokens > 0);
        assert!(report.graph_tokens > 0);
        assert_eq!(report.divergence_tokens, 0);
        assert_eq!(report.history_tokens, 0);
        assert!(report.user_message_tokens > 0);
        assert_eq!(report.tool_schema_tokens, 0);
        assert_eq!(
            report.total_tokens,
            report.system_prompt_tokens + report.graph_tokens + report.user_message_tokens
        );
    }

    #[test]
    fn test_context_report_summary_format() {
        let report = ContextReport {
            system_prompt_tokens: 500,
            graph_tokens: 10000,
            divergence_tokens: 200,
            history_tokens: 3000,
            user_message_tokens: 100,
            tool_schema_tokens: 5000,
            total_tokens: 18800,
        };
        let s = report.summary();
        assert!(s.contains("~18K tokens"));
        assert!(s.contains("graph: 10K"));
    }

    #[test]
    fn test_extract_relevant_ids_character_name() {
        let mut g = NarrativeGraph::new();
        g.add_node(crate::concepts::narrative_graph::GraphNode::Character {
            id: "char-1".to_string(),
            name: "Marcus".to_string(),
            aliases: vec!["Marc".to_string()],
            description: None,
        });
        g.add_node(crate::concepts::narrative_graph::GraphNode::Character {
            id: "char-2".to_string(),
            name: "Elena".to_string(),
            aliases: vec![],
            description: None,
        });

        let ids = extract_relevant_ids("Tell me about Marcus", &g);
        assert_eq!(ids, vec!["char-1"]);

        // Case-insensitive
        let ids = extract_relevant_ids("what does marcus want?", &g);
        assert_eq!(ids, vec!["char-1"]);

        // Alias match
        let ids = extract_relevant_ids("What is Marc doing?", &g);
        assert_eq!(ids, vec!["char-1"]);

        // No match
        let ids = extract_relevant_ids("How is the pacing?", &g);
        assert!(ids.is_empty());

        // Multiple matches
        let ids = extract_relevant_ids("Compare Marcus and Elena", &g);
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_extract_relevant_ids_scene_title() {
        let mut g = NarrativeGraph::new();
        g.add_node(crate::concepts::narrative_graph::GraphNode::Scene {
            id: "scene-1".to_string(),
            title: Some("The Banquet".to_string()),
            summary: "A feast.".to_string(),
            characters_present: vec![],
            location: None,
            time: None,
            file_path: String::new(),
        });

        let ids = extract_relevant_ids("What happens at the banquet?", &g);
        assert_eq!(ids, vec!["scene-1"]);
    }

    #[test]
    fn test_extract_relevant_ids_short_names_skipped() {
        let mut g = NarrativeGraph::new();
        g.add_node(crate::concepts::narrative_graph::GraphNode::Character {
            id: "char-1".to_string(),
            name: "Al".to_string(), // too short (< 3 chars)
            aliases: vec![],
            description: None,
        });

        let ids = extract_relevant_ids("Al is interesting", &g);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_extract_relevant_ids_empty_graph() {
        let g = NarrativeGraph::new();
        let ids = extract_relevant_ids("Tell me about everyone", &g);
        assert!(ids.is_empty());
    }

    fn make_msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.to_string(),
            tool_calls: None,
            tool_results: None,
        }
    }

    #[test]
    fn test_summarize_history_short_history_unchanged() {
        let history = vec![
            make_msg(Role::User, "Hello"),
            make_msg(Role::Assistant, "Hi there!"),
        ];
        let result = summarize_history(&history, 6);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "Hello");
        assert_eq!(result[1].content, "Hi there!");
    }

    #[test]
    fn test_summarize_history_exact_keep_recent() {
        let history = vec![
            make_msg(Role::User, "One"),
            make_msg(Role::Assistant, "Two"),
            make_msg(Role::User, "Three"),
            make_msg(Role::Assistant, "Four"),
            make_msg(Role::User, "Five"),
            make_msg(Role::Assistant, "Six"),
        ];
        let result = summarize_history(&history, 6);
        assert_eq!(result.len(), 6);
        assert_eq!(result[0].content, "One");
    }

    #[test]
    fn test_summarize_history_compresses_older() {
        let history = vec![
            make_msg(Role::User, "Tell me about the plot."),
            make_msg(Role::Assistant, "The plot involves a heist."),
            make_msg(Role::User, "Who is Marcus?"),
            make_msg(Role::Assistant, "Marcus is the protagonist."),
            make_msg(Role::User, "Recent question one"),
            make_msg(Role::Assistant, "Recent answer one"),
            make_msg(Role::User, "Recent question two"),
            make_msg(Role::Assistant, "Recent answer two"),
            make_msg(Role::User, "Recent question three"),
            make_msg(Role::Assistant, "Recent answer three"),
        ];
        let result = summarize_history(&history, 6);
        // 1 summary + 6 recent = 7
        assert_eq!(result.len(), 7);
        assert_eq!(result[0].role, Role::User);
        assert!(result[0].content.contains("Earlier conversation summary:"));
        assert!(result[0].content.contains("plot"));
        assert!(result[0].content.contains("Marcus"));
        // Recent messages preserved verbatim
        assert_eq!(result[1].content, "Recent question one");
        assert_eq!(result[2].content, "Recent answer one");
        assert_eq!(result[6].content, "Recent answer three");
    }

    #[test]
    fn test_summarize_history_empty() {
        let result = summarize_history(&[], 6);
        assert!(result.is_empty());
    }

    #[test]
    fn test_summarize_history_odd_message_count() {
        // Unpaired user message in older section
        let history = vec![
            make_msg(Role::User, "Orphan question"),
            make_msg(Role::User, "Recent one"),
            make_msg(Role::Assistant, "Recent two"),
        ];
        let result = summarize_history(&history, 2);
        assert_eq!(result.len(), 3); // 1 summary + 2 recent
        assert!(result[0].content.contains("Orphan question"));
        assert_eq!(result[1].content, "Recent one");
    }

    #[test]
    fn test_extract_key_phrase_truncation() {
        let short = extract_key_phrase("Hello world");
        assert_eq!(short, "Hello world");

        let with_period = extract_key_phrase("First sentence. Second sentence.");
        assert_eq!(with_period, "First sentence");

        let empty = extract_key_phrase("");
        assert_eq!(empty, "(empty)");

        let long = extract_key_phrase(&"a".repeat(200));
        assert!(long.len() <= 80);
        assert!(long.ends_with("..."));
    }
}
