---
phase: 8
title: agent-science-kit alignment
status: pending
repo: agent-science-kit
covers: ["§5 all rows", "decision 2"]
---

# Phase 8: agent-science-kit alignment

## Files

- `skills/ask-setup/SKILL.md` (~L188–205, L548)
- `skills/ask-setup/references/installation.md` (§10b ~L354–378, verify list ~L729)
- `skills/ask-setup/references/capability-detection.md` (~L365–380, L877)
- `skills/ask-quarto-manuscript/references/render-and-troubleshoot.md` (§1a ~L26–41)
- `skills/ask-quarto-manuscript/references/imrad-structure.md` ("Config Synchronization" ~L71–102)
- `skills/ask-quarto-manuscript/assets/manuscript-template/myst.yml` (delete)
- `skills/ask-quarto-manuscript/assets/manuscript-template/_quarto.yml`
- `skills/ask-init/references/initialization-workflow.md` (gitignore block ~L168–185, scaffold list ~L248–256)
- `skills/ask-init/references/project-structure.md` (row for `_quarto.yml`/`myst.yml`)
- `README.md` (~L250, L276, L329–337)

## Steps

1. **Install.** Replace `uv tool install mystquarto` / `pip install mystquarto` with `cargo install mystquarto` (preferred when cargo is present) or the release installer from `github.com/ntluong95/myst-quarto-rustCLI/releases`. The minimum is `>= 0.3.0`. The capability probe uses `mystquarto --version` and records the version. Readiness requires ≥ 0.3.0, and 0.2.x is `degraded`.
2. **Preview recipe (decision 2).** Replace the `_build/myst` command in `render-and-troubleshoot.md` and `README.md` with:

   ```bash
   tmp="$(mktemp -d)"
   mystquarto to-myst . -o "$tmp/myst"
   (cd "$tmp/myst" && myst start)          # live preview, nothing written to the project
   # optional static copy:
   (cd "$tmp/myst" && myst build --html) && rm -rf _build/myst-html && cp -R "$tmp/myst/_build/html" _build/myst-html
   ```

   State plainly that the MyST twin is a disposable preview, that Quarto is the only source, and that nothing else is written to the project.
3. **Template.** Delete the template's `myst.yml`. Update the imrad "Config Synchronization" table so it describes the conversion mapping (Quarto field → generated MyST field) instead of two hand-synced files. Update `ask-init` so it no longer scaffolds `myst.yml`, and have drift detection report an existing root `myst.yml` as "legacy, safe to delete" without deleting it.
4. **Gitignore block.** Add `_build/`, `.quarto/`, `_freeze/` (only when not using freeze for reproducibility; keep this project-decided) and `.mystquarto/` to the ASK governance block.
5. **Quarto subfolder with executable cell.** Reproduce `readfile .../manuscript/manuscript/index.qmd` on Quarto 1.9.36 in a minimal repo. Then pick one fix:
   - If it's a Quarto bug, file it upstream and document the workaround (`execute-dir: project` or moving `index.qmd` to root) in `render-and-troubleshoot.md`.
   - If root placement is required, change the template to `manuscript.article: index.qmd` at the root and keep sections in `manuscript/`. The ask-init structure docs must follow.

   Decide after the repro; don't guess.
6. **Import path (decision 1).** Document in `ask-quarto-manuscript` how to bring an existing MyST project into ASK: `mystquarto to-quarto <myst-project> -o <new-empty-dir>`, then review and move `manuscript/**`, `literature/references.bib` and figures into the ASK repo by hand. The tool never merges into the repo.

## Validation

- `/ask:setup` on this Mac reports `mystquarto READY 0.3.0`.
- A freshly `ask-init`ed project renders with `quarto render`, including one executable cell, with 0 `ERROR`.
- The preview recipe gives 0 ⛔ from `myst build --html` in temp, and `git status --porcelain` in the project is empty afterwards (apart from optional `_build/myst-html`, which is gitignored).
- `grep -rn "pip install mystquarto\|uv tool install mystquarto\|_build/myst " skills README.md` finds nothing.
