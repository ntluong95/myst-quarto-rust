---
phase: 2
title: Label, include and path correctness
status: done
repo: myst-quarto-rustCLI
covers: [B1, B2, B3, B9]
---

# Phase 2: Label, include and path correctness

## Files

- `crates/mystquarto-core/src/registry/normalize.rs` (B1)
- `crates/mystquarto-core/src/writer/myst.rs` (`myst_include_target`, ~L505) and `writer/quarto.rs` (~L644) (B2)
- `crates/mystquarto-core/src/config/myst_to_quarto.rs` (`rewrite_content_extension`, ~L396) (B3)
- `crates/mystquarto-core/src/config/exports.rs`, `mappings.toml` (B9)

## Steps

1. **B1, doubled prefixes.** In `normalize`, when no colon is present, skip the inferred prefix if `base` already starts with that Quarto prefix (`fig-`, `tbl-`, `eq-`, `sec-`, theorem prefixes). Add unit tests: `fig-main`+Figure → `fig-main`, `sec-intro`+Section → `sec-intro`, `main`+Figure → `fig-main`, `figure-x`+Figure → `fig-figure-x` (a prefix match needs the hyphen).
2. **B2, include renames.** Pick one rule and apply it in both the include writer and the file writer. **Keep the leading underscore.** Quarto uses `_` to exclude partials from rendering, and MyST uses the same convention in toc-less projects. Remove `strip_prefix('_')` from both writers and add a test that the include target equals the written file path.
3. **B3, `.qmd.qmd`.** `rewrite_content_extension` should return names that already end in `.qmd` unchanged. Add a test table: `.md`→`.qmd`, `.qmd`→`.qmd`, `.ipynb`→`.ipynb`, no extension→`.qmd`. Check the mirror function in `quarto_to_myst.rs` (~L345/L410) for the same `.md.md` case.
4. **B9, typst.** Map a `myst.yml` `exports[format: typst]` to a Quarto `format: typst` entry, carrying `template`→`template` where it is a path, and `output`→`output-file`. Reverse direction too. Update the mappings table and `docs/`.

## Validation

- Unit tests above.
- Phase 1 harness: all label, include and `index.qmd.qmd` failures are gone, and the round-trip diff shows no label changes.

## Risk

Existing users may already have `fig-fig-*` labels written by 0.2.0 output. Note this in the CHANGELOG. A reverse conversion through `labels.json` still maps correctly because the sidecar records the exact pair.

## Result

B1, B2, B3 and B9 are fixed with unit tests. The harness no longer reports doubled labels, the missing `_intro` include, or `index.qmd.qmd`. The e2e problem count fell from 53 to 41 on `ask-manuscript`. Include targets and written file names now share one rule, `writer::swap_content_extension`. A code-cell label may keep either the `fig-` or the `tbl-` prefix.
