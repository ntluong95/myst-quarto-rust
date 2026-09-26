---
title: "mystquarto fidelity fixes and agent-science-kit alignment"
description: "Fix every finding from the 2026-09-26 review so MyST↔Quarto conversion renders cleanly both ways and never pollutes ASK project folders."
status: in-progress
priority: P1
effort: 5-7d
branch: fix/fidelity-and-ask-alignment
tags: [rust, cli, mystquarto, agent-science-kit, quarto, myst]
blockedBy: []
blocks: []
created: 2026-09-26
---

# mystquarto fidelity fixes and ASK alignment

Source findings: [`../reports/research-260926-2206-mystquarto-review-and-ask-alignment.md`](../reports/research-260926-2206-mystquarto-review-and-ask-alignment.md).
This plan uses the report's IDs (B1–B9, §3 syntax table, §4 pollution, §5 ASK rows).

Repositories touched:

- **mystquarto**: `04 EXPERIMENTS/myst-quarto-rustCLI` (phases 1–7)
- **agent-science-kit**: `04 EXPERIMENTS/agent-science-kit` (phase 8)

## Outcome

Three things must hold for an ASK-shaped project, a native MyST project, and the corpus fixtures:

- `mystquarto to-myst` followed by `myst build --html` produces zero MyST ⛔ errors and zero lost content.
- `mystquarto to-quarto` followed by `quarto render` produces zero Quarto `ERROR`s and zero unresolved cross-references.
- A full round trip returns labels, callout types, figures, tables and config fields unchanged.

Conversion never reads or copies anything outside the manuscript's own file closure. It never writes into the source project or the parent folder.

## Accepted decisions

1. **MyST→Quarto writes only to a new, separate folder** (option A). The tool never merges into an existing ASK repo. It refuses an output directory that already exists and is non-empty, unless that directory is its own previous output, which it recognises by the `.mystquarto/` marker. The user moves files into ASK by hand.
2. **The MyST preview copy lives outside the project**, in a system temp directory. ASK's preview recipe converts into temp and runs `myst start` or `myst build --html` there. Only the finished HTML comes back, and only when asked for, into gitignored `_build/myst-html/`.
3. Quarto stays ASK's canonical source. The root `myst.yml` is no longer hand-authored or kept in the ASK template.

## Non-goals

- There is no in-place "import into an existing ASK repo" mode.
- There is no MyST-canonical ASK workflow.
- There are no new MyST/Quarto constructs beyond those in the report's §3 table and B5.

## Phases

| # | Phase | Repo | Covers | Effort | Depends on |
|---|---|---|---|---|---|
| 1 | [End-to-end render harness](phase-01-render-harness.md) | mystquarto | acceptance gate for all later phases | 0.5d | – |
| 2 | [Label, include and path correctness](phase-02-label-include-path-bugs.md) | mystquarto | B1, B2, B3, B9 | 0.5d | 1 |
| 3 | [Config conversion fidelity](phase-03-config-fidelity.md) | mystquarto | B4, B7, B8 | 1d | 1 |
| 4 | [Manuscript-scoped closure and output safety](phase-04-manuscript-closure-output-safety.md) | mystquarto | §4 items 1–4, decision 1 | 1.5d | 1, 3 |
| 5 | [Single-file mode](phase-05-single-file-mode.md) | mystquarto | §5 single-file row | 0.5d | 4 |
| 6 | [Syntax alignment](phase-06-syntax-alignment.md) | mystquarto | §3 table, B5, B6 | 1.5d | 1, 2 |
| 7 | [CLI surface and release](phase-07-cli-surface-release.md) | mystquarto | `--version`, docs, 0.3.0 | 0.5d | 2–6 |
| 8 | [agent-science-kit alignment](phase-08-ask-alignment.md) | agent-science-kit | §5 rows, decision 2 | 0.5d | 7 |

Phases 2, 3 and 6 touch disjoint files and may run in parallel once phase 1 lands. Phase 4 needs phase 3 because both edit `orchestrate.rs`.

## Global acceptance criteria

- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` are green.
- [ ] `cargo test -p mystquarto --features renderer-tests --test renderer` is green on all three e2e fixtures (phase 1) in both directions.
- [ ] On the ASK fixture, the output tree contains only manuscript-closure files, and a fake `data/raw/` fixture never appears in it.
- [ ] `/ask:setup` detects mystquarto ≥ 0.3.0 with a version. ASK's documented preview recipe builds with 0 ⛔ and leaves `git status` clean.

## Rollback

Each phase is a separate commit on `fix/fidelity-and-ask-alignment`, so reverting one commit rolls back that phase. The ASK changes (phase 8) ship in their own PR and pin `mystquarto >= 0.3.0`. ASK keeps working with 0.2.0 until that PR merges.
