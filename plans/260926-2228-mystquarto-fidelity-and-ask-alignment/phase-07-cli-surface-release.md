---
phase: 7
title: CLI surface and release
status: in-progress
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

## Result

- Steps 1–4 are done. `--version` works on all three binaries (CLI test `every_binary_prints_its_version`). The version is 0.3.0. The CHANGELOG leads with the breaking changes. The README has the required `-o`, `--scope`, "Which files are read" (the denylist), and "Using with agent-science-kit" sections.
- Step 2: `--no-label-map` was not actually inert. It already skips writing `labels.json`; only its doc comment said otherwise. The doc was fixed and the flag kept.
- Step 6 (local): 0.3.0 installed into `~/.cargo/bin` from the branch, and the review's `/tmp/mq-*` scratch dirs deleted.
- Step 5 (release through cargo-dist) is **pending the user's go-ahead**. It publishes a tag, GitHub release artifacts and the crates.io packages.
