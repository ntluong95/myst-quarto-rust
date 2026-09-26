---
title: mystquarto review and ASK alignment plan
date: 2026-09-26
summary: Green unit suite hid render-breaking conversion bugs and project-folder pollution; 8-phase fix plan written.
---

# mystquarto review and ASK alignment plan

## What happened
Cloned ntluong95/myst-quarto-rustCLI (v0.2.0) and tested it against real renders (Quarto 1.9.36, mystmd 1.10.1) on an ASK-shaped probe, a native MyST probe, and a copy of submission-myst-full-main. All 323 unit tests pass, but neither direction renders cleanly and the Quarto→MyST→Quarto round trip fails outright.

Key root causes: `registry/normalize.rs` prepends kind prefixes to labels that already have them (`fig-main`→`fig-fig-main`); include targets drop the `_` prefix while the file keeps it; `rewrite_content_extension` yields `index.qmd.qmd`; the other dialect's config in the input is copied through and the converted one is discarded silently; four MyST directives with Quarto equivalents are deleted from the output.

Pollution: the asset copier walks the whole tree with only a cache skip-list and ignores `.gitignore`, so an ASK project's raw-data tier, `AGENTS.md`, and `plans/` would be copied or converted. The default output is a sibling folder in the parent directory.

ASK drift: install docs still say uv/pip (dropped in 0.2.0); the documented `quarto2myst … _build/myst` recipe builds nothing; the template `myst.yml` has placeholder values MyST rejects; the template `_quarto.yml` fails as soon as an executable cell is added.

## Decision
MyST→Quarto writes only to a new, empty folder and never merges into an ASK repo. The MyST preview copy lives in system temp, outside the project. Conversion scope becomes the manuscript's file closure from config, with a hard denylist covering the raw-data tier, original literature, and `.ask/`.

## Next steps
Execute plans/260926-2228-mystquarto-fidelity-and-ask-alignment/ starting with phase 1 (e2e render harness). The raw-data copy is still inferred from code; the user's manual repro script will confirm it. The ASK hook blocked the agent from disabling it, which was correct.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
