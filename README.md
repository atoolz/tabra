# Tabra

**Tab. Complete. Ship.**

IDE-style terminal autocomplete powered by [withfig specs](https://github.com/withfig/autocomplete). Single Rust binary. No Node.js, no cloud, no login.

```
$ git ch
┌─────────────────────────────────────┐
│ C checkout    Switch branches       │  ← selected
│ C cherry      Find commits          │
│ C cherry-pick Apply commit changes  │
│ C check-attr  Display gitattributes │
└─────────────────────────────────────┘
```

Fuzzy matching, descriptions, subcommand awareness. Works with 500+ CLIs: git, docker, kubectl, npm, cargo, aws, terraform, and more.

## What is this?

Tabra is a background daemon that adds IDE-style autocomplete to your existing terminal. You keep using iTerm2, Alacritty, Kitty, Ghostty, or whatever terminal you prefer. Tabra renders a floating popup with contextual suggestions as you type.

## Why does this exist?

[Fig](https://github.com/withfig/autocomplete) (25k stars) did exactly this and was loved by 100-300k developers. Amazon acquired the company, killed the product, and turned it into Kiro CLI which:

- Is **closed-source**
- Requires an **AWS account login**
- Has been **renamed 3 times** (Fig -> CodeWhisperer CLI -> Q Developer CLI -> Kiro CLI)
- Pivoted to AI chat; autocomplete became a secondary feature

Amazon took Fig, extracted what they wanted for their AI strategy, and left the original use case without an owner. Features like Scripts, Dotfiles, and Plugins were explicitly abandoned.

The [withfig/autocomplete](https://github.com/withfig/autocomplete) spec repo (25k stars, MIT) is still public with definitions for 500+ CLIs. Thousands of hours of community work available for anyone to use.

[Inshellisense](https://github.com/microsoft/inshellisense) (Microsoft, 9.8k stars) proved that consuming these specs works. But it requires Node.js (~60MB runtime) and is maintained by essentially one developer.

## How is Tabra different?

| | Tabra | Inshellisense | Kiro CLI | Warp |
|---|---|---|---|---|
| Runtime | Single Rust binary (<5MB) | Node.js (~60MB) | Closed binary | Full terminal app |
| Login required | No | No | AWS account | Account |
| Works in any terminal | Yes | Yes | Partial | No (is the terminal) |
| Open source | Apache 2.0 | MIT | No | No |
| Spec ecosystem | withfig (500+ CLIs) | withfig (500+ CLIs) | withfig (proprietary fork) | Built-in |
| Target latency | <5ms per completion | ~20-50ms | Unknown | N/A |

## Architecture

```
Shell (zsh/bash/fish)
  └── Shell Hook (ZLE widget / readline binding)
        └── Unix Socket IPC (JSON-Lines)
              └── Tabra Daemon (Rust, ~30MB RSS)
                    ├── Spec Loader (withfig JSON, hot-reload)
                    ├── Parser (tokenizer + spec tree walker)
                    ├── Resolver (suggestion collection)
                    ├── Matcher (nucleo fuzzy matching)
                    └── Renderer (ANSI overlay)
```

## Quick Start

```bash
# Install (from source)
cargo install --path .

# Install completion specs
tabra install-specs --from ./specs/

# Add to your .zshrc
echo 'eval "$(tabra init zsh)"' >> ~/.zshrc

# Restart your shell
exec zsh
```

## Usage

```bash
# Start daemon manually (usually auto-started by the shell hook)
tabra daemon

# Check daemon status
tabra status

# Print shell hook for eval
tabra init zsh    # or: bash, fish

# Stop daemon
tabra stop
```

## Development Status

> **Score: 5.2/10 | Status: WOUNDED | License: Apache 2.0 | Tech: Rust | MVP: 4+ months**

Tabra is in early development. The core engine (parser, resolver, matcher, renderer) is implemented and compiles. The zsh hook works for basic completion. What's missing:

- [ ] Download and compile withfig specs into JSON
- [ ] End-to-end testing with real specs
- [ ] Bash and Fish shell hooks
- [ ] Generator script execution (dynamic completions like `git branch` names)
- [ ] Performance benchmarking
- [ ] Release binaries

### Why "wounded"?

Honest assessment from our [market analysis](https://github.com/atoolz/market-hunter):

- **Inshellisense (9.8k stars) exists and works.** The "no Node.js" pitch is real but niche.
- **Terminal overlay rendering is genuinely hard.** Fig had a team and $20M. Compatibility across terminal emulators, tmux, and different shell configurations is a deep problem.
- **Fig proved this doesn't monetize.** Amazon with all its resources couldn't make terminal autocomplete into a business.
- **The withfig spec ecosystem is slowly decaying.** Last significant commit was May 2025. Tabra would need to become the steward of the ecosystem to keep specs current.

We build it anyway because the tool should exist as open source, regardless of commercial viability.

## Why Apache 2.0

Tabra is a daemon that runs in the developer's terminal. It intercepts keystrokes, renders an autocomplete popup, and dies when the terminal closes. It doesn't expose any network interface. It's not a service.

The ecosystem Tabra consumes is MIT: the [withfig/autocomplete](https://github.com/withfig/autocomplete) specs (25k stars) are MIT. The original Fig was MIT. Inshellisense (Microsoft) is MIT. Using a copyleft license would create cultural incompatibility with an entirely MIT ecosystem, pushing away contributors who already write specs and could contribute to Tabra.

The biggest threat to Tabra is not someone stealing the code. It's not having contributors. The spec ecosystem requires constant maintenance (CLIs change flags between versions). The more permissive the license, the more people contribute specs. And specs are Tabra's moat: without coverage for 500+ CLIs, the product doesn't work.

Apache 2.0 maximizes contributions. Commercial features (private spec registry for internal CLIs, usage analytics, AI completion) will live in a separate `tabra-teams` repository with a proprietary license. The autocomplete core and specs never change license.

## Monetization Plan

| Tier | What | Price |
|---|---|---|
| **Free (Apache 2.0)** | Autocomplete overlay, all withfig specs, zsh+bash+fish, local daemon | $0 forever |
| **Tabra Teams** (future) | Private spec registry for internal CLIs, usage analytics, AI completion plugin | $5-10/user/month |

## Contributing

Tabra needs contributors, especially for:

- **Spec maintenance**: keeping withfig specs up to date as CLIs release new versions
- **Shell hooks**: bash and fish integration
- **Terminal compatibility**: testing across emulators (iTerm2, Alacritty, Kitty, Ghostty, WezTerm, Windows Terminal)
- **Bug reports**: edge cases in parsing, rendering, and shell integration

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Apache License 2.0. See [LICENSE](LICENSE).

Copyright 2026 AToolZ.
