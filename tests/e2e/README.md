# End-to-end render fixtures

`crates/mystquarto/tests/renderer.rs` (`e2e_*` tests, `renderer-tests` feature) runs each
fixture through the real renderers:

1. Copy the fixture to a temp dir, add a gitignored raw-data file there at runtime, and `git init`.
2. Render the source natively, which proves the fixture itself is valid.
3. Convert it to the other dialect, check the output against `expected-closure.txt`, and render it.
4. Convert back, render again, then diff the content files and the config against the original.
5. Check that `git status --porcelain` in the source is still empty.

Every problem is collected and reported together. The test fails on any MyST `⛔️`, on any
MyST `⚠️` that signals lost or mis-parsed content, on any Quarto `ERROR`, and on any
unresolved citation or cross-reference warning.

| Fixture | Source dialect | What it proves |
|---|---|---|
| `ask-manuscript/` | Quarto | An agent-science-kit project. It has a root `index.qmd` (Quarto cannot render a `manuscript/` subfolder article that has code cells), an include, cites, cross-refs, callouts, a tabset, a margin, a Python cell, a panel figure and a footnote. A stale `myst.yml` sits next to `_quarto.yml`. The decoys (`AGENTS.md`, `README.md`, `plans/`, `analysis/`, `data/obfuscated/`, `literature/og/`) and the runtime raw-data tier must never reach the output. |
| `myst-native/` | MyST | A native MyST project. It covers figure, math, list-table, admonition `:class:`, dropdown, grid/card, sub/sup/abbr/kbd, aside, code-block, mermaid, seealso, epigraph, `+++`, and the `{doc}`/`{ref}`/`{numref}` roles. |

Per-fixture files:

- `expected-closure.txt` lists the files the forward conversion may write. A trailing `/` allows a whole subtree.
- `allowed-diff.txt` lists round-trip line differences that are accepted, one `<file>: <trimmed line>` per line. Each entry needs a reason in a comment above it.

Run the fixtures locally with Quarto, mystmd and Jupyter on `PATH`:

```bash
cargo test -p mystquarto --features renderer-tests --test renderer e2e_
```
