---
phase: 7
title: CLI surface and release
status: pending
repo: myst-quarto-rustCLI
---

# Phase 7: CLI surface and release

## Steps

1. Add `#[command(version)]` to all three `clap` parsers in `crates/mystquarto/src/args.rs`, so `mystquarto --version` prints `mystquarto 0.3.0`. Add a CLI test.
2. Remove the inert `--no-label-map` flag or wire it up. It is currently documented as doing nothing.
3. Bump the version to 0.3.0 and write a CHANGELOG entry. Call out the breaking changes: `-o` is required for project roots, closure-scoped copying, the `_` include rename is removed, and the label-prefix fix.
4. Update the README: options table (`--scope`, required `-o`, output-dir rules), a "Using with agent-science-kit" section that links ASK's preview recipe, and a "What is never copied" section (the denylist).
5. Release through the existing cargo-dist workflow (`dist-workspace.toml`, `.github/workflows/release.yml`). Confirm that `cargo install mystquarto` and the release installer both give 0.3.0.
6. Reinstall locally (`~/.cargo/bin/mystquarto`, currently 0.2.0 from the release installer) and delete the review's scratch dirs (`/tmp/mq-*`, `/tmp/mqbin`).

## Validation

- CI is green on all targets, including the phase 1 renderer job.
- `mystquarto --version`, `myst2quarto --version` and `quarto2myst --version` all print 0.3.0.
