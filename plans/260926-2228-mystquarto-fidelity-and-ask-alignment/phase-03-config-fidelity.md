---
phase: 3
title: Config conversion fidelity
status: done
repo: myst-quarto-rustCLI
covers: [B4, B7, B8]
---

# Phase 3: Config conversion fidelity

## Files

- `crates/mystquarto/src/orchestrate.rs` (~L290–330 config loop, summary reporting)
- `crates/mystquarto-core/src/fs/assets.rs` (config names excluded from copy)
- `crates/mystquarto-core/src/config/myst_to_quarto.rs`, `quarto_to_myst.rs`, `sidecar.rs`
- Bib reachability check (the MQ0301 emitter; locate with `grep -rn MQ0301 crates/`)
- `docs/diagnostics.md`

## Steps

1. **B4, stale pass-through config.** The other dialect's config in the *input* is a source artifact, not a hand-authored output. Exclude **both** `myst.yml` and `_quarto.yml` from asset copying. Then the "refusing to overwrite" gate only fires for a real pre-existing file in the output dir, which phase 4 makes impossible anyway.
2. **B4, silent failure.** Any `FileStatus::Failed` must count in the summary's error total and set a non-zero exit code. Add a CLI test.
3. **B4, input has both configs.** Convert the direction's source config and emit an Info diagnostic noting that the target-dialect config in the input was ignored. Preserve it in `.mystquarto/preserved.json` so a reverse run can restore its fields.
4. **B7, Quarto fields lost on MyST→Quarto.** Every `_quarto.yml` key that has no `myst.yml` home must go into `preserved.json` on Quarto→MyST and be restored verbatim on the way back. That covers `project.output-dir`, `project.render`, `execute.*`, `manuscript.code-links`, `manuscript.resources`, `format.*` options, and unknown keys. The MQ0409 "dropped" warning becomes "preserved" (LossyExpected).
5. **B7, invented fields.** Stop emitting defaults the source never had (`jats: {}`, `pdf: {}`, `latex: {}`, `comments.hypothesis`, `theme: default`). Emit a format only when the source export list names it.
6. **B8, false MQ0301.** Resolve bibliography paths relative to the file that declares them: `myst.yml` → project root, frontmatter → that page's directory. Parse `.bib` keys with a real `@type{key,` scan. Test with `literature/references.bib` and `../literature/references.bib`.

## Validation

- ASK fixture: round-trip `_quarto.yml` is semantically equal to the original (compare parsed YAML, ignoring comments and key order).
- There are no MQ0301 warnings on either fixture.
- Exit code is non-zero when a config write fails (unit test with a read-only output dir).

## Result

Configs now round-trip exactly on both e2e fixtures. `ask-manuscript` went from 41 to 23 problems. None of the remaining ones are about configs or bibliographies.

- **B7 uses a snapshot, not a per-key preserve list.** Each conversion records the source config's exact text and the config it derived from it (`source_config` in `.mystquarto/preserved.json`, module `config::snapshot`). Converting back restores the original byte for byte, comments included, when the derived config is unchanged. When it was edited, a three-way merge applies only the edits. This covers every key in step 4's list, and any key added later. MQ0416 (Info) reports the restore, and MQ0409 is now LossyExpected ("kept and restored").
- **B4.** Asset copy excludes both dialects' configs. A target-dialect config found in the input is ignored with MQ0415 (Info), and its text is kept under `ignored_config`. A config write failure is now a `Failed` outcome with a non-zero exit, instead of aborting the run.
- **B7, invented fields.** `theme: default` and `comments.hypothesis` are no longer emitted. `repo-url` becomes `github` only for GitHub URLs, because MyST rejects other hosts.
- **Found by the harness.** An article-only `exports` entry is invalid in mystmd 1.11. The reverse direction now leaves the article only in `project.toc`, and the forward direction reads the first non-notebook toc entry back as `manuscript.article`. Corpus case d08 was updated to match, since it now names the article instead of commenting out the toc.
- **B8.** Citation keys resolve against `myst.yml` `project.bibliography` (relative to the root), a root `.bib`, and each page's frontmatter `bibliography` (relative to the page).
