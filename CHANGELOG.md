# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.0] - 2026-04-12

Initial public release.

### Added

- **Narrative graph** — automatic extraction of characters, objectives, conflicts, and relationships from manuscript text
- **Native desktop GUI** (`laires gui`) — egui/eframe app with chat, canvas, scene sidebar, and force-directed graph visualization
- **Split-pane TUI** (`laires open`) — ratatui terminal UI with graph, lint, pacing, and file explorer overlays
- **Two operating modes** — Consultant (read-only analysis + revision briefs) and Workshop (live co-editing)
- **Multi-turn agent chat** with 28 built-in tools across 7 categories (file, graph, perspective, structural, canvas, brief, custom)
- **Character perspectives** — LLM-powered subjective scene interpretation, blind spot detection, and knowledge tracking
- **Multi-file project support** — automatic file discovery, LLM classification, and manifest persistence
- **Prose and Fountain** scene detection with .docx and .txt import (`laires convert`)
- **Multiple LLM providers** — Anthropic, OpenAI, Gemini, Local (Ollama), and any OpenAI-compatible endpoint
- **Consistency checking** — lint rules, divergence detection (inferred vs declared intent), and orphan detection
- **Custom skills framework** — TOML-defined tools in `.laires/skills/` with permission model and audit logging
- **VCS integration** — `laires diff` and `laires log` for narrative graph changes across git/jj commits
- **CLI commands** — init, scan, graph, status, chat, gui, open, lint, diff, log, perspective, brief, convert
