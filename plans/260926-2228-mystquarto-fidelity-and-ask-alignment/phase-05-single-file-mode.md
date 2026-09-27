---
phase: 5
title: Single-file mode
status: done
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

## Result

`single_file_mode_builds_on_its_own_in_both_directions` (renderer test) runs the plan's scenario at runtime. `manuscript/index.qmd` sits in a repo with no config and reaches `../images`, `../literature` and an include. `quarto2myst` on that one file gives a MyST site that builds with 0 problems, with the included Introduction and the figure in the HTML. Converting the result's `index.md` back renders with Quarto with 0 problems.

- There is no separate single-file code path anymore. A file becomes a `Selection` (root, closure, config choice) that the project path carries out, so assets, sidecars, the output marker and every safety rule apply unchanged.
- The search root is the nearest ancestor holding the source config, else the nearest git root, else the file's directory. The output-dir rules are checked against it.
- Decision 3 was refined. The enclosing config is converted only when it names the file, and then the whole project closure is used, which is the same as converting the root. Otherwise a minimal config is synthesized (MQ0418): `myst.yml` with a one-entry toc, the rebased page bibliography and `site.template: article-theme`, or a default `_quarto.yml` that renders the file. Converting a book's config for one chapter would list missing chapters and fail the MyST build. MQ0417 (Info) suggests converting the root whenever a config encloses the file.
- Found by the harness: `myst build --html` exits 0 and writes nothing when `myst.yml` has no `site`. The harness now treats a MyST build with no HTML as a failure.
