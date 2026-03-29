# MIAWK

MIAWK is a native desktop shell for chat, agents, account flows, and swarm views, built with Rust and Dioxus.

## Status

MIAWK is currently in early public release shape. Tagged GitHub releases build platform artifacts for:

- macOS `x86_64`
- macOS `arm64`
- Windows `x86_64`
- Windows `arm64`
- Linux `x86_64`
- Linux `arm64`

The website download flow targets the latest GitHub release assets.

## Local Development

```bash
cargo check --locked
```

To build native packages locally, install the Dioxus CLI and run:

```bash
./scripts/package-native.sh
```

## Releases

Publishing a GitHub release triggers the multi-platform packaging workflow in `.github/workflows/platform-builds.yml`.
