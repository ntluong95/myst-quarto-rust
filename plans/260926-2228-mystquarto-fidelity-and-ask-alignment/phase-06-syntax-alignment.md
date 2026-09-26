---
phase: 6
title: Syntax alignment
status: pending
repo: myst-quarto-rustCLI
covers: ["§3 table", B5, B6]
---

# Phase 6: Syntax alignment

## Files

- `mappings.toml` (plus `scripts/render-mapping-tables.py` to regenerate README tables; `tests/doc-sync.rs` enforces the sync)
- `crates/mystquarto-core/src/reader/{myst,quarto,inline,fence}.rs`
- `crates/mystquarto-core/src/writer/{myst,quarto}.rs`
- `tests/corpus/constructs/` (one new case per row)
- `docs/dialect-comparison.md`

## Mappings (each needs a corpus case in both directions plus a round-trip)

| # | MyST | Quarto |
|---|---|---|
| S1 | `:::{warning} Title` (any typed admonition with a title) | `::: {.callout-warning title="Title"}` (type kept both ways) |
| S2 | `{admonition} Title` + `:class: tip` | `callout-tip title="Title"`. Also fix quoted-attribute parsing so `title="Custom title"` keeps its spaces. |
| S3 | `{dropdown} Title` / `:class: dropdown` | `callout-note collapse="true" title="Title"`. Reverse: `collapse="true"` with no type becomes `{dropdown}`. |
| S4 | `{list-table} Caption` + `:header-rows:` + `:label:` | pipe table + `: Caption {#tbl-x}`. Use a grid table when cells hold block content, and emit a lossy diagnostic only for spans. |
| S5 | `{code-block} lang` + `:linenos:` / `:caption:` / `:emphasize-lines:` | ```` ```{.lang code-line-numbers="true" filename="caption"} ```` with `code-line-numbers="1,3"` for emphasis |
| S6 | `{epigraph}` / `{pull-quote}` / `{blockquote}` | `> quote` plus `.epigraph` class div so the reverse is exact |
| S7 | `[](#label)` link xref | `@label` (reverse keeps `@label`, which MyST supports) |
| S8 | `` {math}`x` `` role | `$x$` |
| S9 | `[text](file.qmd)` → to-myst | `[text](file.md)` for any closure content file (fixes the `{doc}` round trip) |
| S10 | `{grid} N` + `{card} Title` | `::: {.grid}` with `::: {.g-col-12 .g-col-md-(12/N)}` children. Card title becomes a `**Title**` first line with a `.card` class kept for the reverse. |
| S11 | `{sub}`, `{sup}`, `{kbd}`, `{abbr}` | `~x~`, `^x^`, `[x]{.kbd}`, `<abbr>`: the **reverse** mapping must be added (today it is one-way) |
| S12 | `{mermaid}` | ```` ```{mermaid} ```` both ways, never `{code-cell} mermaid` |
| S13 | Quarto cell `#\| label: fig-x` / `#\| fig-cap:` | MyST code-cell with `#\| label:` / `#\| caption:` in-cell options (mystmd style), not `:caption:` directive options |
| S14 | Quarto `author:` list with affiliations | MyST `authors:` with `affiliations:` ids plus top-level `affiliations:` list |
| S15 (B5) | Quarto `::: {#fig-p layout-ncol=2}` with subfigure images `{#fig-a}` | MyST `:::{figure}` + `:label: fig-p`, subfigures as nested `:::{figure} path` + `:label: fig-a`. `layout-ncol` becomes a preserved attribute and is restored on the way back. |

## Writer rule: fence nesting

When a directive contains nested directives, the MyST writer must give the outer fence one more colon (or backtick) than the deepest inner fence. Prefer colon fences (`:::`) for any directive with markdown content and backticks only for code-like directives (`code-cell`, `math`, `mermaid`). Add a property test: parse(write(ir)) == ir for nested admonition, grid/card and tab-set trees.

## B6 policy

`MQ0201` "no equivalent, preserved as comment" is now reserved for constructs that truly have no rendering equivalent (e.g. `{glossary}`, `{proof}` types without Quarto counterparts). After S3–S6 none of the four reported constructs hit it. Any construct that still falls back must be rendered as a visible, readable block (a raw fenced div with the original text), **not** an HTML comment, so content never disappears from the rendered page.

## Validation

- Every S-row has a corpus case, and `tests/corpus` passes.
- Phase 1 harness on `myst-native` gives: 0 ⚠️ from the allow-listed set, and a round-trip diff that is empty apart from blank lines.
- The README tables are regenerated, and `doc-sync` passes.
