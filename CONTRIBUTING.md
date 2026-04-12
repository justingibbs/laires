# Contributing to Laires

Thanks for your interest in contributing to Laires! Here's how to get started.

## Getting started

```bash
git clone https://github.com/justingibbs/laires.git
cd laires
cargo build
cargo test
```

**Requirements:** Rust 1.93+ (edition 2024). Install via [rustup](https://rustup.rs/).

## Making changes

1. Fork the repo and create a branch from `main`
2. Make your changes
3. Run `cargo fmt` and `cargo clippy` before committing
4. Add tests for new functionality
5. Make sure all tests pass with `cargo test`
6. Open a pull request

## Code style

- Run `cargo fmt` — we use the default rustfmt settings
- Run `cargo clippy` — all warnings should be clean
- Follow existing patterns in the codebase

## Reporting bugs

Open an issue with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- Your OS and Rust version (`rustc --version`)

## Feature requests

Open an issue describing the feature and why it would be useful. For larger changes, please discuss in an issue before starting a PR — this helps avoid duplicate work and ensures the design fits the project.

## Architecture

Laires is built on the **Concept & Synchronization** pattern. See `context/laires-spec.md` for the full technical specification. The key directories:

- `src/concepts/` — Core domain modules (text buffer, scene map, narrative graph, etc.)
- `src/cli/` — CLI command handlers
- `src/gui/` — Native desktop GUI (egui/eframe)
- `src/sync/` — Synchronization layer between concepts
- `src/runtime/` — Shared runtime services (project loader, agent session, scene cache)

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
