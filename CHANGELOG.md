# Changelog

## 0.3.0

Conversions now render cleanly in both directions and never read or copy
anything outside the manuscript. Verified end to end against Quarto 1.11.5
and mystmd 1.11.0 (`cargo test -p mystquarto --features renderer-tests`).

**Breaking changes**

- **`-o/--output` is required** (unless `--in-place`). The old default,
  a sibling `<input>-quarto/` / `<input>-myst/` folder, wrote into the
  project's parent directory. The error message suggests a path.
- **The output directory must be new, empty, or this tool's own previous
  output of the same direction** (marked by `.mystquarto/output.json`). An
  existing project is refused even with `--force`, so a conversion can never
  merge into one (MQ0608). An output inside the input is refused unless it
  is gitignored.
- **With a project config, only the manuscript's closure is read and
  copied**: what `_quarto.yml` / `myst.yml` names, plus the includes,
  figures, links and bibliographies it reaches. Previously the whole folder
  was walked. `--scope all` restores a full walk. Without a config, agent
  and tooling files (`AGENTS.md`, `CLAUDE.md`, `README.md`, `CHANGELOG.md`,
  `plans/`) are skipped.
- **Governed locations are never read or copied**: `data/raw/`,
  `literature/og/`, `.ask/`, `.git/`, `.env*`. A manuscript reference into
  one fails the run (MQ0606). `.gitignore` is honoured everywhere (MQ0607).
- **Include targets keep their leading `_`** (`sections/_intro.qmd` ↔
  `sections/_intro.md`). 0.2.0 renamed the include to `intro.md` but wrote
  the file as `_intro.md`, so the include never resolved.
- **Labels already carrying their Quarto prefix are no longer prefixed
  again**: `fig-main` stays `fig-main`, not `fig-fig-main`. Output written by
  0.2.0 may contain doubled ids. A reverse conversion through its
  `labels.json` still restores the original labels.
- `--in-place` is refused in an agent-science-kit project (`.ask/`), where
  Quarto is the canonical source.
- `rust-version` is now 1.88 (required by the `ignore` crate).

### Added

- `--version` on `mystquarto`, `myst2quarto` and `quarto2myst`.
- `--scope manuscript|all`.
- Single-file conversion produces a project that builds. The file's
  closure is converted and laid out under its common ancestor, so `../`
  paths keep resolving, and a minimal `myst.yml` / `_quarto.yml` is written
  (MQ0418).
- Exact config round trips. A conversion records the source config and
  the config derived from it; converting back restores the original byte
  for byte, or merges only the fields you edited (MQ0416). `project.render`,
  `execute`, `manuscript.code-links`, `manuscript.resources`, format options
  and unknown keys all survive (MQ0409 is now "kept", not "dropped").
- Syntax mappings, both ways:
  - typed admonitions with titles (`:::{warning} Title`), `{admonition}` with `:class: tip`, and `{dropdown}`
  - MyST-only admonition kinds kept as a class
  - `{list-table}` (pipe or grid table), `{code-block}` options, `{epigraph}` / `{pull-quote}`
  - `{grid}` / `{card}`, and panel figures with subfigures
  - mystmd-style `#|` cell options
  - `author` affiliations ↔ MyST's `affiliations` list
  - `[](#label)`, `{math}`, links to other documents, and `~sub~` / `^sup^` / `[kbd]{.kbd}` / `<abbr>` back to MyST roles
- `typst` exports map to Quarto's `typst` format, with `output` → `output-file` and a template path.
- An end-to-end harness (`tests/e2e/`): an agent-science-kit project and a native MyST project are converted, rendered with the real renderers, converted back, and diffed.

### Fixed

- `manuscript.article` no longer becomes `index.qmd.qmd`.
- A stale target-dialect config in the input (for example a hand-kept `myst.yml` next to `_quarto.yml`) no longer overwrites the converted one. It is ignored and kept in the sidecar (MQ0415).
- A config write failure fails the run instead of aborting it.
- Bibliographies are found relative to the file that declares them, so citation keys defined in `literature/references.bib` are no longer reported missing (MQ0301).
- Invented format options (`theme: default`, `comments.hypothesis`) and article-only MyST exports are no longer emitted. A non-GitHub `repo-url` is no longer written as `github`.
- Nested directives no longer break the MyST output: the outer fence is always longer than any inner fence. Quarto divs nest correctly, and quoted attribute values keep their spaces.
- Constructs with no equivalent stay visible as a literal code block instead of disappearing into an HTML comment. They are still restored exactly on the reverse conversion.
- A cross-document reference outside a Quarto book becomes a link to the other page instead of an unresolvable `@id`.
- Blank lines no longer accumulate after frontmatter on every conversion.
- The crates package for crates.io, so `cargo install mystquarto` works. `mappings.toml` moved into `crates/mystquarto-core/`; it previously sat outside the crate and blocked packaging.

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
