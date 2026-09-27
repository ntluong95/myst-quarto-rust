# mystquarto

Bidirectional **modern MyST (mystmd v1)** ↔ Quarto converter. Transforms
directives, roles, config files, and frontmatter between the two formats
through a typed document IR — not a regex line-scanner — so label
normalization, cross-references, and citations survive the round trip.

> **Coming from the Python version?** `pip install mystquarto` / `uvx
> myst2quarto` are discontinued as of 0.2.0. See
> [`docs/migration-from-python.md`](docs/migration-from-python.md).

## Installation

```bash
cargo install mystquarto
```

Or without a local install:

```bash
npx mystquarto to-quarto docs/ -o /tmp/docs-quarto
```

Or grab a prebuilt binary for macOS (arm64/x64), Linux (x64/arm64/musl), or
Windows (x64) from the [Releases page](https://github.com/ntluong95/myst-quarto-rustCLI/releases).

## Usage

```bash
# Convert MyST → Quarto
myst2quarto docs/ -o docs-quarto/

# Convert Quarto → MyST
quarto2myst docs/ -o docs-myst/

# Unified CLI
mystquarto to-quarto docs/ -o docs-quarto/
mystquarto to-myst docs/ -o docs-myst/
```

`-o` is required. The output directory must not exist yet, or be empty, or
hold this tool's own previous output of the same direction. It may sit inside
the project only if it is gitignored (for example `_build/myst`). An existing
project is always refused, even with `--force`: a conversion never merges
into one, so you move the files you want by hand.

### Options

| Flag | Description |
|---|---|
| `-o DIR` / `--output DIR` | Output directory (required unless `--in-place`; see above) |
| `--scope manuscript\|all` | `manuscript` (default) reads only what the project config reaches; `all` walks the whole folder (see below) |
| `--in-place` | Modify files in-place (requires `--force`, refuses on a dirty VCS state, and in an agent-science-kit project) |
| `--force` | Bypass the `--in-place` overwrite and clean-VCS-state gates |
| `--config-only` | Only convert config files (`myst.yml` ↔ `_quarto.yml`) |
| `--no-config` | Skip config file conversion |
| `--dry-run` | Show what would change; writes zero bytes |
| `--strict[=warn\|all]` | `--strict` fails the run on Warning+ diagnostics; `--strict=all` also fails on expected-lossy conversions |

Every lossy or ambiguous conversion prints a `file:line` diagnostic with a
stable code — see [`docs/diagnostics.md`](docs/diagnostics.md). Nothing is
silently dropped: unmappable constructs are preserved verbatim in
`.mystquarto/preserved.json` and restored on the reverse conversion; label
renames are tracked in `.mystquarto/labels.json` so `fig:samples` ↔
`fig-samples` round-trips.

### Which files are read

With a project config (`_quarto.yml` / `myst.yml`), a conversion reads only
the manuscript's **closure**. That means the files the config names
(`manuscript.article`, `project.render`, `book.chapters`, `project.toc`,
export articles, resources, bibliography), plus everything those files
reach: includes, embeds, figures and images, links to other documents, and
frontmatter bibliographies. Nothing else in the folder is read or copied.
Without a config, the whole folder is walked, skipping agent and tooling
files (`AGENTS.md`, `CLAUDE.md`, `README.md`, `CHANGELOG.md`, `plans/`).
`--scope all` walks the whole folder even when a config exists.

In every mode, `.gitignore` is honoured, and these locations are **never**
read or copied, even when the manuscript references them:

- the raw-data tier `data/raw/` and original literature `literature/og/`
- `.ask/`, `.git/`, and `.env*` files

A reference into one of them is error MQ0606 and fails the run.

### Using with agent-science-kit

In an [agent-science-kit](https://github.com/ntluong95/agent-science-kit)
project, Quarto is the only source. A MyST version is a disposable preview
built outside the project, so nothing is written into it:

```bash
tmp="$(mktemp -d)"
mystquarto to-myst . -o "$tmp/myst"
(cd "$tmp/myst" && myst start)
```

To bring an existing MyST project into an ASK project, convert it into a new
directory and move the manuscript files across by hand:

```bash
mystquarto to-quarto path/to/myst-project -o "$(mktemp -d)/imported"
```

The full recipe, including a static HTML copy, is in ASK's
`skills/ask-quarto-manuscript/references/render-and-troubleshoot.md`.

## What it converts

### Block directives

| MyST | Quarto |
|---|---|
| `` ```{code-cell} python `` + `#\| label:` / `#\| caption:` | `` ```{python} `` + `#\| label:` / `#\| fig-cap:` |
| `:tags: [remove-input]` | `#\| echo: false` |
| `:tags: [remove-output]` | `#\| output: false` |
| `:tags: [remove-cell]` | `#\| include: false` |
| `:tags: [hide-input]` | `#\| code-fold: true` |
| `:::{figure} path` | `![caption](path){#fig-id width=X}` |
| `:::{figure}` holding nested figures | `::: {#fig-id layout-ncol=N}` holding images |
| `` ```{math} `` + `:label:` | `$$ ... $$ {#eq-id}` |
| `:::{note}` / `{warning}` / `{tip}` / `{important}` / `{caution}` | `::: {.callout-*}` |
| `:::{warning} Title` | `::: {.callout-warning title="Title"}` |
| `:::{admonition} Title` + `:class: tip` | `::: {.callout-tip title="Title"}` |
| `:::{seealso}`, `{hint}`, `{danger}`, … | Nearest callout plus the kind as a class (`.callout-note .seealso`) |
| `:::{dropdown} Title` | `::: {.callout-note .dropdown collapse="true" title="Title"}` |
| `:class: dropdown` (+ `:open:`) | `collapse="true"` (`"false"`) |
| `::::{tab-set}` / `:::{tab-item}` | `::: {.panel-tabset}` / `## Label` |
| `:::{margin}` / `{aside}` | `::: {.column-margin}` |
| `::::{grid} N` / `:::{card} Title` | `::: {.grid}` / `::: {.card .g-col-12 .g-col-md-(12/N)}` with a `**Title**` line |
| `:::{epigraph}` / `{pull-quote}` | `::: {.epigraph}` around a `>` quote |
| `` ```{list-table} Caption `` | Pipe table (grid table for multi-line cells) + `: Caption {#tbl-id}` |
| `:::{table} Caption` | Markdown table + `: Caption {#tbl-id}` |
| `` ```{code-block} lang `` + `:linenos:` / `:emphasize-lines:` / `:caption:` | `` ```{.lang code-line-numbers="…" filename="…"} `` |
| `` ```{image} url `` | `![alt](url){width=X}` |
| `` ```{bibliography} `` / `{tableofcontents}` | Removed (Quarto handles both via config) |
| `` ```{mermaid} `` | `` ```{mermaid} `` |

A construct with no equivalent (for example `{glossary}`) is never dropped.
It stays visible as a literal code block of its original source, and it is
restored exactly on the reverse conversion.

### Inline roles

| MyST | Quarto |
|---|---|
| `` {eval}`expr` `` | `` `{python} expr` `` |
| `` {cite}`key` `` | `[@key]` |
| `` {cite:t}`key` `` | `@key` |
| `` {cite:p}`key` `` | `[@key]` |
| `` {cite}`a,b,c` `` | `[@a; @b; @c]` |
| `` {numref}`fig-id` `` / `` {ref}`label` `` / `[](#label)` | `@label` (a link to the other page for a label defined elsewhere, outside Quarto books) |
| `` {eq}`label` `` | `@eq-label` |
| `` {doc}`path` `` / `[text](path.md)` | `[path](path.qmd)` / `[text](path.qmd)` |
| `` {math}`x` `` | `$x$` |
| `` {sub}`x` `` / `` {sup}`x` `` | `~x~` / `^x^` |
| `` {kbd}`Ctrl` `` | `[Ctrl]{.kbd}` |
| `` {abbr}`CI (confidence interval)` `` | `<abbr title="confidence interval">CI</abbr>` |

### Config files (`myst.yml` ↔ `_quarto.yml`)

| `myst.yml` | `_quarto.yml` |
|---|---|
| `project.title` | `title:` or `book.title:` |
| `project.authors` | `author:` |
| `project.bibliography` | `bibliography:` |
| `project.toc` | `book.chapters:` |
| `site.template: book-theme` | `project.type: book` |
| `site.template: article-theme` | `project.type: manuscript` |
| `project.exports[].article` | `manuscript.article` |
| `project.toc` (article + `.ipynb` entries) | `manuscript.notebooks` |
| `project.exports[format: pdf]` | `format.pdf:` |

`format:` on the Quarto side always gets `html:` as its first (default) entry
— Quarto's `format:` map renders whichever key comes first when no `--to` is
given, and `typst`/`docx`/`pdf` are alternates, never the default render
target. `typst` always carries `number-sections: true`, since Quarto's Typst
writer can't resolve a cross-reference to an unnumbered heading. `html` is
never round-tripped back into myst.yml's `exports[]` on the reverse
direction — it isn't a legal value there (MyST's own schema rejects it) and
is the implicit default on both sides.

### Frontmatter (per-file YAML)

| MyST | Quarto |
|---|---|
| `kernelspec: {name: python3}` | `jupyter: python3` |
| `kernelspec: {name: ir}` | `engine: knitr` |
| `label:` | `id:` |
| `exports:` | `format:` |

A page with an `{r}` code-cell but no `engine:`/`jupyter:` frontmatter at
all still gets `kernelspec: {name: ir}` on conversion to MyST — Quarto
infers the R engine implicitly from the fence, so the converter checks the
body, not just the frontmatter. See
[`docs/diagnostics.md`](docs/diagnostics.md) (MQ0412/MQ0413) for what's
additionally needed to make it *execute* under MyST, not just display as
code — installing IRkernel alone is not enough.

## Architecture

```
                    ┌──────────────────┐
   .md (MyST)  ───► │   MystReader     │ ─┐
                    └──────────────────┘  │
                                          ├──►  ┌─────────┐  ──┬─► MystWriter  ──► .md
                    ┌──────────────────┐  │     │ DocIR   │    │
  .qmd (Quarto) ──► │  QuartoReader    │ ─┘     │ + spans │    └─► QuartoWriter ──► .qmd
                    └──────────────────┘        └─────────┘
                                                     │
                                    ┌────────────────┼────────────────┐
                                    │                │                │
                            ┌───────────────┐ ┌─────────────┐ ┌──────────────┐
                            │ LabelRegistry │ │ Diagnostics │ │ PathGuard    │
                            │ (run-scoped)  │ │ file:line   │ │ containment  │
                            └───────────────┘ └─────────────┘ └──────────────┘
```

Both readers parse into a typed `Document` IR that knows what kind of
construct each block and label is (a figure needs `fig-`, a table needs
`tbl-`) before either writer runs. `PathGuard` refuses symlink escapes,
`..` include traversal, include cycles, and output-inside-input recursion.
See [`docs/dialect-comparison.md`](docs/dialect-comparison.md) for the full
MyST/Quarto construct reference this implementation targets.

Crate layout (Cargo workspace):

| Crate | Responsibility |
|---|---|
| `mystquarto-core` | IR, readers, writers, label registry, diagnostics, config/frontmatter, path guard |
| `mystquarto` | `clap` CLI, file discovery, orchestration, reporting — the published binary crate |

## Development

```bash
git clone https://github.com/ntluong95/myst-quarto-rustCLI
cd myst-quarto-rustCLI
cargo test --workspace                                       # unit + corpus + round-trip
cargo test -p mystquarto --features renderer-tests --test renderer  # needs quarto + myst installed
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

## License

MIT-0
