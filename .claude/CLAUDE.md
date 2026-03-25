# Tabra

**Tab. Complete. Ship.**

IDE-style terminal autocomplete using withfig specs. Single Rust binary, no Node.js, no cloud, no login.

## What it is

Tabra is a terminal autocomplete overlay that provides IDE-style popup suggestions for 500+ CLI tools. It runs as a background daemon, intercepts keystrokes via shell hooks, and renders a floating popup below your cursor with fuzzy-matched completions.

## Architecture

```
Shell (zsh/bash/fish)
  └── Shell Hook (ZLE widget)
        └── Unix Socket IPC
              └── Tabra Daemon (Rust)
                    ├── Spec Loader (withfig JSON specs)
                    ├── Parser (tokenizer + spec tree walker)
                    ├── Resolver (collects candidate suggestions)
                    ├── Matcher (nucleo fuzzy matching)
                    └── Renderer (ANSI overlay)
```

## Key files

- `src/main.rs` - CLI entry point (clap)
- `src/spec/types.rs` - withfig spec data structures
- `src/spec/loader.rs` - Spec loading and hot-reload
- `src/engine/parser.rs` - Command line tokenizer + spec walker
- `src/engine/resolver.rs` - Suggestion collection from specs
- `src/engine/matcher.rs` - Fuzzy matching via nucleo
- `src/ipc/protocol.rs` - Request/Response types, socket path
- `src/ipc/server.rs` - Async Unix socket server
- `src/ipc/client.rs` - Sync client for CLI subcommands
- `src/daemon/mod.rs` - Daemon lifecycle
- `src/render/overlay.rs` - ANSI popup renderer
- `src/render/theme.rs` - Color theme
- `src/shell/hook.rs` - Zsh hook script

## Conventions

- Rust 2021 edition
- Error handling: `anyhow` for application errors, `thiserror` for library errors
- Async: tokio (daemon only), sync std for CLI client
- Logging: `tracing` with env filter (RUST_LOG=tabra=debug)
- Specs: withfig/autocomplete JSON format, stored in ~/.local/share/tabra/specs/

## Development

```bash
cargo check          # Type check
cargo test           # Run tests
cargo run -- daemon  # Start daemon
cargo run -- init zsh  # Print zsh hook
cargo run -- status  # Check daemon health
```

## MVP priorities

1. Zsh integration working end-to-end
2. Top 50 CLI specs (git, docker, kubectl, npm, cargo, etc.)
3. Fuzzy matching with highlighted matches
4. Popup rendering with borders and descriptions
5. Filesystem completion (filepaths, folders)

## What we explicitly don't do (yet)

- Bash/Fish support (post-MVP)
- AI completion (optional plugin later, never core)
- Cloud sync, telemetry, accounts
- Windows support
- GUI settings
