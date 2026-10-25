---
title: Installation
sidebar_position: 2
---

# Install Wax

Wax is an alpha for macOS and Linux. Install channels provide the engine; Wax
downloads enabled language packs on demand into global `~/.wax/langs/` state.

## Homebrew

```bash
brew tap Daio-io/wax
brew install wax
```

## npm alpha

```bash
npm install -g @waxhq/wax@alpha
```

## Curl

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash
```

Review the [installer source](https://github.com/Daio-io/wax/blob/main/scripts/install.sh)
before piping it to a shell.

## Build from source

Install a current Rust toolchain, clone the repository, and run:

```bash
cd engine
cargo build --release -p wax-cli
./target/release/wax --help
```

## Verify the installation

```bash
wax --version
wax --help
```

Continue with [repository initialization](./initialize.md).
