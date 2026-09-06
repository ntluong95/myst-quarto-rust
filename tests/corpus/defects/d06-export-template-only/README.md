# D06 — Export with only `template:` produces invalid `format: {}` (myst_to_quarto, config)

**Extension exception.** Per the phase-01 spec, this is a config-level
fixture: files use `.yml` throughout (`input.yml` / `expected.yml` /
`python-actual.yml`) instead of the `.md`/`.qmd` convention, in addition to
the full `input_dir/myst.yml` used to drive the real conversion.

**Defect statement.** A MyST export entry may specify only a `template:`
(publication template) with no `format:` key — this is the case for the
`article-template` fixture's `lapreprint-typst` export. The converter's
`_convert_exports_to_format` skips any export lacking a `format` key
entirely, so the whole `exports` list can produce an **empty** format block —
`format: {}` — which is not valid/useful Quarto (it declares zero output
formats).

**Reference-doc section.** docs/dialect-comparison.md §8.3 "Exports ↔
formats" — `template: lapreprint-typst` → `format: {typst: {}}` (⚠️ template
not portable, but a format *must* be inferred). Catalogued as D6 in §12.

**Root cause.** `_convert_exports_to_format` in `src/mystquarto/config.py:85-98`:
```python
fmt = export.get("format")
if not fmt:
    continue
```
An export with only `template:` is dropped from the loop entirely, so
`format_block` stays `{}`. `myst_to_quarto_config` (config.py:112-169) then
unconditionally sets `result["format"] = _convert_exports_to_format(...)`
whenever `"exports" in project`, regardless of whether anything survived.

**Capture command.**

```bash
uv run python -c "
from mystquarto.config import convert_myst_config
convert_myst_config('tests/corpus/defects/d06-export-template-only/input_dir/myst.yml', '/tmp/d06-capture')
"
# then copy /tmp/d06-capture/_quarto.yml -> tests/corpus/defects/d06-export-template-only/python-actual.yml
```
(Also reproducible via the full CLI: `uv run myst2quarto
tests/corpus/defects/d06-export-template-only/input_dir -o /tmp/d06-out`.)

**Verification.** `expected.yml` parsed with `yaml.safe_load` (valid YAML,
confirmed) and used as `_quarto.yml` for a trivial one-page project
(`index.qmd`). `quarto render` (default target, which resolves to `typst` per
this fixture's `format:` key) exits 0, compiles to PDF, no `ERROR`.

**Result vs. prediction.** Matched exactly — `python-actual.yml` is
`title: Sample Article\nformat: {}\n`.

**Rust behavior update (2026-09-06).** The Rust converter's `format:` map
always leads with `html:` (Quarto's `format:` map renders its first key by
default, and `typst`/`docx`/`pdf` are export alternatives, never the default
render target) plus a sensible default option block. `typst` additionally
always gets `number-sections: true` — Quarto's Typst writer compiles
`@sec-xxx` cross-references to `#ref(<sec-xxx>, ...)`, and Typst refuses to
reference an unnumbered heading, so any document with a section
cross-reference previously failed `quarto render --to typst` with `cannot
reference heading without numbering`. See
`crates/mystquarto-core/src/config/exports.rs`'s `html_defaults`/
`default_format_options`.
