# Contributing to Tabra

Thanks for your interest in contributing to Tabra. This document covers the development setup, conventions, and how to submit changes.

## Development Setup

### Prerequisites

- **Rust** (stable toolchain): `rustup` or system package
- **Node.js** (18+): only needed for compiling withfig specs
- **Git**: for the test suite (integration tests create real repos)

### Build and Test

```bash
# Clone
git clone https://github.com/atoolz/tabra.git
cd tabra

# Build
cargo build

# Run unit tests
cargo test --lib

# Run integration tests (requires specs/ directory)
cargo test --test integration -- --test-threads=1

# Run all tests
cargo test --lib && cargo test --test integration -- --test-threads=1

# Check formatting and lints
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings

# Run benchmarks
cargo bench --bench completion
```

### Compiling Specs

The `specs/` directory contains pre-compiled JSON specs for 54 CLI tools. To regenerate:

```bash
# Requires Node.js
node scripts/compile-specs.mjs --top 50 --out specs

# Validate specs against Rust types
cargo run -- validate-specs --from specs
```

### Running the Daemon

```bash
# Install specs to default location
cargo run -- install-specs --from specs

# Start daemon
cargo run -- daemon

# Test completion
cargo run -- complete-shell --buffer "git " --cursor 4 --cwd . --cols 80

# Stop daemon
cargo run -- stop
```

## Project Structure

```
src/
  lib.rs           # Library crate (re-exports all modules)
  main.rs          # Binary entry point (CLI via clap)
  daemon/mod.rs    # Daemon lifecycle
  engine/
    parser.rs      # Command line tokenizer + spec walker
    resolver.rs    # Suggestion collection + generator execution
    matcher.rs     # Fuzzy matching via nucleo
  ipc/
    protocol.rs    # Request/Response types
    server.rs      # Async Unix socket server
    client.rs      # Sync client for CLI subcommands
  render/
    overlay.rs     # ANSI popup renderer
    theme.rs       # Color theme (Catppuccin)
  shell/
    hook.rs        # Zsh hook
    bash_hook.rs   # Bash hook
    fish_hook.rs   # Fish hook
  spec/
    types.rs       # Withfig spec data structures
    loader.rs      # Spec loading, validation, hot-reload
scripts/
  compile-specs.mjs  # Withfig spec compiler (TS -> JSON)
specs/
  *.json           # Pre-compiled specs (committed)
tests/
  integration.rs   # Integration tests (spawn real daemon)
benches/
  completion.rs    # Criterion benchmarks
docs/
  generators.md    # Generator execution design doc
```

## How to Contribute

### Spec Maintenance

The most impactful contribution is keeping specs up to date. CLI tools change flags between versions.

To update a spec:
1. Run `node scripts/compile-specs.mjs --top 1` to see the current compilation
2. Check if the CLI tool has new subcommands/flags
3. The upstream specs live at [withfig/autocomplete](https://github.com/withfig/autocomplete)
4. Submit PRs to withfig/autocomplete first, then regenerate here

### Bug Reports

Open an issue with:
- The command you typed
- Expected behavior
- Actual behavior
- Your shell (zsh/bash/fish) and terminal emulator
- `tabra --version` output

### Code Changes

1. Fork the repo
2. Create a branch: `git checkout -b fix/description`
3. Make changes
4. Run `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings`
5. Run tests: `cargo test --lib && cargo test --test integration -- --test-threads=1`
6. Commit with a descriptive message
7. Open a PR

### Shell Hook Changes

Shell hooks live as raw strings in Rust files (`src/shell/hook.rs`, `bash_hook.rs`, `fish_hook.rs`). When modifying hooks:

- Test in a real shell session, not just the integration tests
- The integration tests (`test_init_*_output`) verify string content, not runtime behavior
- Be careful with quoting and escaping (the hooks are embedded in Rust raw strings)

### Adding a New Shell

1. Create `src/shell/{shell}_hook.rs` with a `pub fn {shell}_hook() -> String`
2. Add the variant to `ShellType` enum in `src/shell/mod.rs`
3. Wire it into `print_hook()`
4. Add a `test_init_{shell}_output` integration test
5. Document key binding approach in the hook comments

## Conventions

- **Error handling**: `anyhow` for application errors, `thiserror` for library errors
- **Logging**: `tracing` with env filter (`RUST_LOG=tabra=debug`)
- **Formatting**: `cargo fmt` (default settings)
- **Lints**: `cargo clippy -- -D warnings` must pass
- **Tests**: all tests must pass before merge
- **Commits**: descriptive messages, present tense ("Add X" not "Added X")

## License

By contributing, you agree that your contributions will be licensed under the Apache License 2.0.
