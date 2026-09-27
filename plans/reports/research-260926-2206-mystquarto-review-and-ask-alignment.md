# mystquarto review and agent-science-kit alignment

Conducted: 2026-09-26 22:06–22:40 (Europe/Stockholm)
Subject repo: `04 EXPERIMENTS/myst-quarto-rustCLI` (cloned from `ntluong95/myst-quarto-rustCLI`, `main` @ `2ae1c07`, v0.2.0)
Toolchain: Quarto 1.9.36, mystmd 1.10.1, fresh `cargo install --path crates/mystquarto` into `/tmp/mqbin`
Consumer: `04 EXPERIMENTS/agent-science-kit` @ `159b0bb`

## Verdict

**Not meeting the success criteria yet.** The unit suite is green (323 tests pass), but the
real-project results are poor. Neither direction produces a project that renders cleanly
from an ASK-shaped repo. A Quarto→MyST→Quarto round trip does not render at all.
The ASK integration is broken end to end, and running the converter on an ASK project
copies governed folders (including `data/raw/`) into the output tree.

Most defects are small, local fixes. The design (typed IR, sidecars, path guard) is sound.
The gaps are in the specific rules, and in the missing definition of which folders belong
to the manuscript.

## Table of contents

1. [How it was tested](#1-how-it-was-tested)
2. [Blocking conversion bugs](#2-blocking-conversion-bugs)
3. [Syntax alignment gaps](#3-syntax-alignment-gaps)
4. [Project-folder pollution](#4-project-folder-pollution)
5. [agent-science-kit alignment](#5-agent-science-kit-alignment)
6. [Recommended fix order](#6-recommended-fix-order)
7. [Unresolved questions](#7-unresolved-questions)

## 1. How it was tested

Three scratch projects under `/tmp`, left in place for reproduction:

| Probe | Path | Shape |
|---|---|---|
| ASK manuscript | `/tmp/mq-ask-probe` | ASK `_quarto.yml` + `myst.yml` template, `manuscript/index.qmd` with include, cites, eq/fig/tbl/sec refs, callouts, tabset, margin, python cell, panel figure, footnote |
| Native MyST | `/tmp/mq-myst-native` | `myst.yml` + `index.md`/`chapter.md` using figure, math, list-table, admonition `:class:`, dropdown, grid/card, sub/sup/abbr/kbd, aside, code-block, mermaid, seealso, epigraph, `+++` |
| Real MyST repo | `/tmp/mq-myst-src` | copy of `submission-myst-full-main` |

Each probe was rendered natively, then converted, then rendered, then converted back and
diffed against the original.

## 2. Blocking conversion bugs

These break rendering or silently lose content, and the tool reports 0 errors for all of them.

| # | Bug | Evidence | Location |
|---|---|---|---|
| B1 | **Label prefix doubled.** `fig-main`→`fig-fig-main`, `eq-model`→`eq-eq-model`, `(sec-intro)=`→`{#sec-sec-intro}`. Quarto then treats `@sec-sec-intro` as a citation (`Citeproc: citation sec-sec-intro not found`). This hits any author who already uses Quarto-style hyphen labels, which is ASK's convention. | Both probes | `registry/normalize.rs:132` prepends the inferred prefix without checking `base.starts_with(quarto_prefix)` |
| B2 | **Include target renamed, file not.** `{{< include sections/_intro.qmd >}}` → `{include} sections/intro.md`, but the file is written as `_intro.md`. The whole Introduction disappears from the MyST build. | ASK probe | `writer/myst.rs:514` strips `_`; file writer does not |
| B3 | **`manuscript.article: manuscript/index.qmd.qmd`.** The round-tripped `_quarto.yml` fails `quarto render` outright. The path already ends in `.qmd`, but the `.qmd` branch only strips `.md`. | ASK round trip | `config/myst_to_quarto.rs:396-403` (`rewrite_content_extension`) |
| B4 | **Config conversion silently skipped when the other dialect's config exists.** ASK projects ship both files. The existing target config is copied through verbatim, and the converted one is discarded. No error is printed, yet MQ04xx diagnostics are printed as if conversion happened. As a result `myst.yml` keeps `article: manuscript/index.qmd`, and MyST logs 16× "Invalid export article". | ASK probe, real repo | `orchestrate.rs:307-321` "refusing to overwrite" outcome is not surfaced in the summary |
| B5 | **Panel figure destroyed.** `::: {#fig-panel layout-ncol=2}` → invalid MyST figure (`:layout-ncol:` unknown, subfigures keep `{#fig-a}`), and back to Quarto as an empty `![]()`. | ASK probe | Quarto reader/MyST writer: subfigure handling |
| B6 | **Constructs deleted from the rendered output.** `list-table` (the table disappears), `dropdown`, `code-block` and `epigraph` become an HTML comment in `.qmd`. They restore only on the reverse trip. All four have direct Quarto equivalents (see §3). | Native probe | `mappings.toml` has no rows for them |
| B7 | **`_quarto.yml` fields silently dropped on MyST→Quarto:** `output-dir`, `render`, `execute.freeze`, `code-links`, `manuscript.resources`. Unwanted fields are added: `jats: {}`, `pdf: {}`, `latex: {}`, `comments.hypothesis`, `theme: default`. | ASK round trip | `config/myst_to_quarto.rs` |
| B8 | **False MQ0301 warnings.** Citation keys `smith2020`/`doe2021` are reported as undefined, but they are in `literature/references.bib`. The bib lookup does not resolve the project-relative path. | ASK round trip | bib reachability check |
| B9 | **`typst` export dropped as "no known Quarto equivalent".** Quarto supports `format: typst` natively. | Real repo | `config/exports.rs` |

## 3. Syntax alignment gaps

The table lists what diverges after conversion. The "Correct target" column is the construct
both renderers actually support.

| Source | Current output | Correct target |
|---|---|---|
| Quarto `callout-warning title="X"` | `{admonition} X` (type lost) → back as `callout-note` | `:::{warning} X` (MyST admonitions accept a title argument) |
| MyST `{admonition} Custom title` + `:class: tip` | `callout-note title="Custom title"` → back as `{admonition} Custom` (title truncated at the space) | `callout-tip title="Custom title"`, with the quoted attribute parsed correctly |
| MyST `{dropdown} Title` | deleted | `::: {.callout-note collapse="true" title="Title"}` |
| MyST `{list-table}` | deleted | pipe/grid table + `: Caption {#tbl-x}` |
| MyST `{code-block} python` + `:linenos:`/`:caption:` | deleted | ```` ```{.python code-line-numbers="true" filename="..."} ```` |
| MyST `{epigraph}` | deleted | `> quote` block |
| MyST `[](#tbl-demo)` link-style xref | unchanged (broken in Quarto) | `@tbl-demo` |
| MyST `` {math}`a^2` `` | unchanged (literal in Quarto) | `$a^2$` |
| MyST `{doc}` → `[chapter](chapter.qmd)` → back | stays `chapter.qmd` in MyST (broken link) | rewrite `.qmd` link targets to `.md` on to-myst |
| MyST `{grid}`/`{card} Title` | `.grid`/`.card` divs (Quarto ignores `.card`; titles lost) → back with **same-length nested backtick fences** (broken nesting) | Quarto `.grid` + `.g-col-6` children; card title as a bold first line. Writer must lengthen outer fences. |
| MyST `` {sub}`2` ``, `{kbd}` | `~2~`, `[Ctrl]{.kbd}` → back **unchanged** (MyST renders them literally) | reverse-map to `{sub}` / `{kbd}` |
| MyST `{mermaid}` | ```` ```{mermaid} ```` → back as `{code-cell} mermaid` (becomes an executable cell with no kernel) | keep `{mermaid}` on to-myst |
| MyST `{aside}` | `.column-margin` → back as `{margin}` | acceptable (MyST alias) |
| Quarto code cell `#\| fig-cap` | MyST `:caption:` on `code-cell` | MyST expects `#\| caption:` / `#\| label:` cell options, or a `{figure}` wrapper around `{embed}` |
| Quarto `author: [{name, affiliations}]` | copied as `author:` | MyST frontmatter key is `authors:` (`author` is an alias, but affiliations need `affiliations:` ids) |
| Frontmatter `bibliography: ../literature/references.bib` in a file whose output location differs | not rebased | rebase relative paths when the file moves (single-file mode) |

Parity baseline: every construct in the "Correct target" column is documented in the mystmd
guide (directives, roles, cross-references) and the Quarto guide (callouts, cross-references,
tables, code blocks). Neither doc set was changed in the last release in a way that affects
these.

## 4. Project-folder pollution

The tool never modified the source repo in any probe (`git status` stayed clean). Pollution
comes from **what it copies and where it writes**:

1. **It copies everything except a fixed skip list.** `ASSET_SKIP_DIRS` (`fs/assets.rs:53`) skips
   only caches and build folders (`_build`, `.git`, `.quarto`, `.venv`, …). It does not read
   `.gitignore`. On an ASK repo, `to-myst . -o X` therefore copies `data/raw/`,
   `data/obfuscated/`, `literature/og/` PDFs, `.ask/`, `renv/`, `_freeze/`, `output/` and
   `analysis/` into `X`. The probe confirmed that `data/obfuscated/masked.csv` was copied. The
   `data/raw/` copy is inferred from the same walk, because ASK's hook correctly blocked creating
   test data there. Copying `data/raw/` out of its gitignored zone breaks ASK's core privacy rule.
2. **It converts every `.md`/`.qmd`, not just the manuscript.** `README.md` became `README.qmd`.
   On ASK this would also convert `AGENTS.md`, `CLAUDE.md`, `plans/**`, `protocol/**`,
   `literature/md/**` and analytical `.qmd` files in `analysis/unchecked/`.
3. **The default output lands outside the project.** `mystquarto to-quarto .` writes
   `../<project>-quarto/`, a sibling in the parent folder (`04 EXPERIMENTS/` in your setup).
4. **The sidecar `.mystquarto/` is written into the output tree.** That is harmless for a
   throwaway build dir, but it must be gitignored when the output is inside the project.
5. **Neither `_build/` nor `.mystquarto/` is in ASK's `.gitignore` governance block.**

What "non-polluting" should mean for ASK: the converter reads the files that the manuscript
reaches (the `_quarto.yml` `render:`/`manuscript.article` list or the `myst.yml` `toc`/`exports`,
plus includes, embedded notebooks, images and the bib) and copies **only those**. Everything else
in the project is out of scope by construction. That is a *closure* walk from the config, not a
directory walk.

## 5. agent-science-kit alignment

| ASK says | Reality | Impact |
|---|---|---|
| Install via `uv tool install mystquarto` / `pip install mystquarto` (`ask-setup` SKILL.md:204, `references/installation.md:363-369`) | Discontinued in 0.2.0. The install is `cargo install`, `npx`, or the release binaries (your machine has the cargo-dist install). | `/ask:setup` installs a stale or missing tool |
| Capability record stores `mystquarto.version` (`capability-detection.md:877`) | The binary has no `--version` flag | Version probe fails |
| `quarto2myst manuscript/index.qmd --output _build/myst` then `myst build _build/myst --html` (`README.md:333`, `render-and-troubleshoot.md:34`) | Single-file mode converts one file only: no includes, no `myst.yml`, no assets, and `../` paths are not rebased. `myst build` also **ignores** the `_build/myst` argument. It builds from the root `myst.yml`, which points at `.qmd`, so it reports "No valid files". MyST also treats `_build` as its own output folder. | The documented MyST path builds nothing |
| Root `myst.yml` is "derived via quarto2myst" (`imrad-structure.md:74`) | The converter never overwrites an existing `myst.yml` (B4), and the template's `myst.yml` names `.qmd` articles that MyST cannot read | The derived config is never derived |
| Template `myst.yml` placeholders | `corresponding` without `email`, `<PROJECT_REPO_URL>` and `<YYYY-MM-DD>` are hard MyST errors | Every fresh ASK project fails MyST validation |
| Template `_quarto.yml` (`manuscript` type, article in `manuscript/`) | `quarto render` fails as soon as `index.qmd` has an executable cell: `readfile .../manuscript/manuscript/index.qmd` (Quarto 1.9.36 notebook path doubling). It renders fine without the cell. | ASK's canonical render breaks on the first code chunk. This is independent of mystquarto. |

**Recommended ASK contract.** Keep Quarto canonical. Never keep a hand-authored `myst.yml` at the
project root. Generate the MyST twin into a gitignored folder **outside `_build/`**, for example
`.ask/render/myst/`:

```bash
mystquarto to-myst . -o .ask/render/myst --scope manuscript   # --scope: proposed flag, see §6
cd .ask/render/myst && myst build --html
```

Add `.ask/render/` and `.mystquarto/` to ASK's governance block. The reverse direction (MyST
project → ASK) writes to a fresh directory and never in place.

## 6. Recommended fix order

In mystquarto:

1. Fix B1 (a one-line `starts_with` guard plus a test for already-prefixed hyphen labels), B2, B3 and B9. All are local fixes, and together they remove most render failures.
2. Fix B4: when the input carries the other dialect's config, convert and overwrite it in the output dir, which is the tool's own output. Treat any silent `Failed` outcome as an error in the summary.
3. Add **manuscript-scoped copying**: derive the file closure from `_quarto.yml`/`myst.yml` plus includes. Honour `.gitignore` and add a hard denylist for `data/raw/` and `literature/og/`. Make this the default whenever a project config exists.
4. Make single-file mode real: follow includes, rebase relative paths, and emit a minimal `myst.yml`/`_quarto.yml`. Otherwise reject a single file when a project config exists.
5. Add the mappings in §3 for dropdown, list-table, code-block, epigraph, `{math}`, `[](#x)`, admonition `:class:`, callout type with title, sub/sup/kbd reverse, mermaid reverse, and fence-length nesting. Fix B5, B7 and B8.
6. Add `--version`. Add a renderer test that runs the ASK template both ways and fails on any MyST ⛔ or Quarto ERROR. The existing `renderer-tests` feature is the right place.

In agent-science-kit:

1. Replace the uv/pip install lines with `cargo install mystquarto` or the release installer.
2. Replace the documented render command with the contract in §5.
3. Remove `myst.yml` from the template, or make it placeholder-valid with `.md` targets.
4. Extend the `.gitignore` block.
5. Report the Quarto manuscript subfolder and executable cell failure, or move `index.qmd` to the root.

## 7. Unresolved questions

- Should MyST→Quarto ever write into an existing ASK repo, or only into a fresh directory? This decides whether ASK needs an "import" mode.
- Is `.ask/render/myst/` acceptable as the derived-MyST location, or would you prefer a top-level gitignored folder?
- The `data/raw/` copy is inferred from code, not reproduced, because ASK's hook blocked the fixture. Do you want a repro in a throwaway repo with the hook disabled?
