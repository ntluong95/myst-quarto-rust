---
phase: 4
title: Manuscript-scoped closure and output safety
status: done
repo: myst-quarto-rustCLI
covers: ["§4.1 copy-everything", "§4.2 convert-every-md", "§4.3 sibling default output", "§4.4 sidecar", "decision 1"]
---

# Phase 4: Manuscript-scoped closure and output safety

## Why

This is the pollution fix. Today the tool walks the whole directory and copies everything except a cache skip-list. It also converts every `.md`/`.qmd`, so ASK's `data/raw/`, `AGENTS.md` and `plans/` all leak into the output.

## Design

When the input directory contains a project config, the conversion set is the **closure** reachable from that config. There is no directory walk.

```
config (_quarto.yml / myst.yml)
  ├─ content roots: manuscript.article, manuscript.notebooks, project.render (globs),
  │                 book.chapters | project.toc, project.exports[].article
  ├─ bibliography / csl / resources (config-level)
  └─ for each content file (recursive):
        includes / {{< include >}} / {embed} / notebook refs
        image & figure paths, frontmatter bibliography, links to other content files
```

- Only closure files are converted or copied. Everything else is not read.
- The closure also applies a **hard denylist**, which wins even when a config references it: `data/raw/`, `literature/og/`, `.ask/`, `.git/`, `.env*`. A denied reference produces an Error diagnostic, is not copied, and the run exits non-zero. In ASK, a manuscript must only reference `output/validated/`-promoted artifacts, so a reference to `data/raw/` is a genuine bug.
- `.gitignore` rules (via the `ignore` crate) are honoured as a second filter. A gitignored closure file gives a Warning and is skipped.
- **With no project config** (a bare folder of `.md`/`.qmd`), behaviour stays the directory walk, but now with `.gitignore` plus the denylist applied. Agent/tooling files are excluded by name: `AGENTS.md`, `CLAUDE.md`, `README.md`, `CHANGELOG.md`, and `plans/`.
- The flag `--scope all` restores the full walk for users who want it. The denylist still applies.

## Output rules (decision 1)

- `-o/--output` is **required** when the input is a project root (contains a config). The sibling default `<input>-quarto` is removed for that case because it writes into the parent folder. The single-file and bare-folder defaults are also removed. The error message suggests a path.
- The output directory must not exist, or must be empty, or must contain a `.mystquarto/` marker from a previous run of the same direction. Anything else is refused. This makes "never merge into an existing ASK repo" mechanical.
- The output must not be inside the input, except for a gitignored path. The existing D16 guard is kept.
- `.mystquarto/` stays in the output tree, since it is the tool's own, and is documented as gitignore-worthy.
- `--in-place` is unchanged, but it is refused when the input root contains `.ask/` (an ASK project), because Quarto is canonical there.

## Files

- Create: `crates/mystquarto-core/src/closure.rs` (closure resolver, denylist, gitignore filter)
- Modify: `crates/mystquarto/src/discover.rs` (config present → closure; absent → filtered walk)
- Modify: `crates/mystquarto-core/src/fs/assets.rs` (copy list comes from the closure, not a walk)
- Modify: `crates/mystquarto/src/orchestrate.rs`, `args.rs` (`--scope`, required `-o`, output-dir rules, `.ask/` in-place refusal)
- Add dependency: `ignore` (already used by ripgrep; MIT/Unlicense)
- Update: `README.md` options table, `docs/diagnostics.md` (new codes for denied reference / gitignored closure file / output refused)

## Validation

- Phase 1 pollution assertion: the ASK fixture output equals `expected-closure.txt` exactly. That is the converted `manuscript/**`, `images/fig1.png`, `literature/references.bib`, the target config and `.mystquarto/`. No `data/`, `plans/`, `AGENTS.md` or `README`.
- Fake `data/raw/secret.csv` fixture (path built at runtime): not in output. When a figure references it, the run exits non-zero with the denial code.
- The source dir's `git status --porcelain` is empty after every run.
- Unit tests cover each output-dir rule, including refusing an existing ASK repo as the output.
- The user's manual repro script from the review (`/tmp/mq-raw-test`) prints no raw files.

## Result

The harness reports no closure problems. The ASK fixture's output matches `expected-closure.txt` exactly, and the runtime raw-data file never reaches the output. `ask-manuscript` went from 23 to 14 problems, all of them phase 6 syntax issues.

- `mystquarto-core/src/closure.rs` resolves the file set in three modes: config closure, explicit files (for phase 5), and a filtered walk. It applies the denylist (MQ0606, Error, fails the run), `.gitignore` (MQ0607, Warning), the agent-file exclusion (walk mode only), and never follows symlinks. `fs::assets::copy_files` copies an explicit list. The old directory walkers in `discover.rs` and `assets.rs` are gone, and so is the `walkdir` dependency.
- Output rules: `-o` is required, and the error suggests `$(mktemp -d)/<name>-<dialect>`. The output directory must be new, empty, or carry `.mystquarto/output.json` for the same direction. An output inside the input is refused unless gitignored (checked against every `.gitignore` between the root and the output). All of these are MQ0608, and `--force` does not bypass them. `--in-place` is refused when the root holds `.ask/`.
- Found on the real `submission-myst-full-main` copy: a `myst.yml` without `toc` makes MyST build every document, so it now falls back to the filtered walk. Root `.bib` files are included when no `bibliography` is named (MyST loads them automatically). `_extensions/` is carried in both directions so Quarto still renders after a round trip.
- `ignore` 0.4.33 needs Rust 1.88, so `rust-version` went from 1.80 to 1.88 (called out in the 0.3.0 CHANGELOG).
- `/tmp/mq-raw-test` from the review does not exist, so it could not be run. The e2e harness does the equivalent check on every run.
