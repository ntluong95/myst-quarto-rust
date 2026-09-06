# Changelog

## 0.2.0

**Breaking:** the Python distribution is discontinued. `pip install
mystquarto` and `uvx myst2quarto` no longer work — install the Rust binary
instead:

```bash
cargo install mystquarto
# or: npx mystquarto to-quarto docs/
# or: download a prebuilt binary from the Releases page
```

See [`docs/migration-from-python.md`](docs/migration-from-python.md) for the
full list of behavior changes.

### Added

- Complete Rust rewrite around a typed document IR (`mystquarto-core` +
  `mystquarto` crates), replacing the Python regex line-scanner.
- Target dialect is now **modern mystmd v1** (`@fig:samples`, `:label:`,
  `(sec:x)=`, `%` comments) on read and write; legacy Sphinx-role MyST
  (`` {cite}`key` ``, `:name:`) is still accepted on read but never emitted.
- Label normalization with a `.mystquarto/labels.json` sidecar: MyST labels
  like `fig:samples` become `fig-samples` on write and restore their
  original spelling on the reverse conversion.
- A preservation sidecar (`.mystquarto/preserved.json`) for constructs with
  no equivalent in the target dialect — nothing is silently dropped, and
  preserved content round-trips through a single-line marker rather than an
  HTML comment (which Pandoc would otherwise emit as live markup).
- Structured diagnostics with stable codes (`MQ0xxx`), file/line locations,
  and four severity classes (Error, Warning, LossyExpected, Info) — see
  [`docs/diagnostics.md`](docs/diagnostics.md). `--strict` fails a run on
  Warning+; `--strict=all` also fails on expected-lossy conversions.
- Path-safety guarantees: symlink escapes, `..` include traversal, include
  cycles, include depth limits, and output-inside-input recursion are all
  refused with a diagnostic rather than silently followed.
- Notebook cell relabelling so `{{< embed >}}` targets resolve after
  conversion.
- DOI citation fallback: citations with no matching `.bib` entry get a
  synthesized bibliography from cached CSL-JSON, so citations resolve
  instead of rendering as a literal `@key`.
- Renderer-backed validation (`cargo test --features renderer-tests`): the
  test corpus is fed through real `quarto render` and `myst build` and
  required to produce zero unresolved cross-references and zero unresolved
  citations, not just a green unit test.

### Fixed

16 conversion defects verified against the Python implementation — see
[`docs/dialect-comparison.md` §12](docs/dialect-comparison.md) for the full
list, including colon-label normalization, figure/table label loss,
literal `%` comments, dropped config fields, and DOI citation-key handling.

Found and fixed since, against a real-world Quarto manuscript conversion:

- `format:` always defaults to `html` first (Quarto's own default render
  target), with `typst`/`docx`/`pdf` as alternates; `typst` always carries
  `number-sections: true`, without which Quarto's Typst writer fails outright
  on any document with a section cross-reference.
- Quarto→MyST: a manuscript's `manuscript.article`/`manuscript.notebooks`
  are no longer silently dropped — they're mapped back to myst.yml's
  `project.toc` and `project.exports[].article` (MQ0414), and
  `site.template: article-theme` is restored. Previously the converted
  project had no toc at all, so MyST fell back to whatever page it
  discovered first (e.g. `README.md`) as the landing page, demoting the
  actual article to an untracked "supporting document."
- Quarto→MyST: `format: html` is no longer round-tripped into myst.yml's
  `exports[]` — it isn't a legal value there and made the converted
  myst.yml fail MyST's own validation outright.
- A page with an `{r}` code-cell but no `engine:`/`jupyter:` frontmatter
  (Quarto infers the engine implicitly from the fence) now gets a
  synthesized `kernelspec: {name: ir}` (MQ0413) so it stays executable
  under MyST instead of silently losing its executability.
- A brand-new output directory (no existing ancestor on disk yet) no longer
  fails to resolve with "no existing ancestor to canonicalize."

### Removed

- `src/mystquarto/` (Python source), `tests/*.py`, `tests/conftest.py`,
  `tests/fixtures/*.md` (Python-only fixtures), `pyproject.toml`, `uv.lock`,
  and the Python CI job. The pre-port Python tree remains available at the
  `pre-rust-port` git tag. `tests/corpus/` is retained — it's Rust-native
  test data now, and its `python-actual.*` records document the old
  behavior without keeping the code.

## 0.1.x (Python)

Prior releases (`pip install mystquarto` 0.1.0–0.1.2) were the Python
implementation. See the `pre-rust-port` git tag for that source tree.
