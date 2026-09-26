---
phase: 5
title: Single-file mode
status: pending
repo: myst-quarto-rustCLI
covers: ["§5 single-file row"]
---

# Phase 5: Single-file mode

## Problem

`quarto2myst manuscript/index.qmd --output X` converts one file only. It doesn't follow includes, doesn't rebase `../` paths, and writes no config. The result can't build.

## Decision

A single-file input is treated as a **one-article project rooted at that file**:

1. The closure starts from the file (phase 4 resolver, minus config roots). Includes, images, bib and embeds are pulled in.
2. Output layout mirrors paths relative to the **common ancestor** of the closure, so `../images/fig1.png` keeps resolving without rewriting paths.
3. The tool emits a minimal target config: `myst.yml` with `project.toc` = the file and `bibliography` from frontmatter, or `_quarto.yml` with `project.type: default`. If the input's enclosing project config exists, it is converted instead of synthesising one.
4. If the file sits inside a directory that has a project config, the tool prints an Info hint that converting the project root is usually what the user wants.

## Files

- `crates/mystquarto/src/orchestrate.rs` (`execute_single_file`, ~L977)
- `crates/mystquarto-core/src/closure.rs` (entry point for file roots)
- `crates/mystquarto/tests/cli.rs`

## Validation

- `quarto2myst manuscript/index.qmd -o $TMP/x`, then `myst build --html` in `$TMP/x`, gives 0 ⛔. The Introduction section is present in the HTML, and the figure image loads.
- The same test passes in the reverse direction for `index.md`.
