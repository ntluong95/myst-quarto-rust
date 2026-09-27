---
phase: 1
title: End-to-end render harness
status: done
repo: myst-quarto-rustCLI
---

# Phase 1: End-to-end render harness

## Why

The 323 unit tests pass while real projects fail to render. The existing `renderer-tests` feature does not exercise project-shaped inputs, so every later phase needs a gate that uses the real renderers.

## Files

- Modify: `crates/mystquarto/tests/renderer.rs`
- Create: `tests/e2e/ask-manuscript/` (ASK-shaped fixture: `_quarto.yml`, `myst.yml` from the ASK template with valid placeholder values, `manuscript/index.qmd`, `manuscript/sections/_intro.qmd`, `literature/references.bib`, `images/fig1.png`, plus decoy dirs `data/obfuscated/`, `plans/`, `AGENTS.md`)
- Create: `tests/e2e/myst-native/` (the `/tmp/mq-myst-native` probe from the review)
- Create: `tests/e2e/README.md` (what each fixture proves)

Fixture content is the review's probe documents verbatim, so every finding has a reproducer.

## Steps

1. Add the two fixtures.
2. For each fixture, the harness copies it to a temp dir and runs `git init`. It then runs the conversion in each applicable direction into a second temp dir and renders with `quarto render` or `myst build --html` (`CI=1`, stdin closed, 240 s timeout). It fails on any of these:
   - a MyST line starting `⛔️`, or a `⚠️` from an allow-listed set (unknown directive/option, include not found, unexpected figure content)
   - a Quarto line containing `ERROR`, or `WARNING` + `citation … not found` / `unable to resolve crossref`
   - a non-empty `git status --porcelain` in the source temp dir
3. Add a round-trip assertion. After converting A→B→A, diff each content file against the original after trimming blank lines. Allowed differences are listed per fixture in `tests/e2e/<fixture>/allowed-diff.txt`, which starts empty.
4. Add a pollution assertion (filled in by phase 4). The output tree must be a subset of the fixture's `expected-closure.txt`.
5. Mark the harness `#[ignore]`-free under `renderer-tests` and add a CI job that installs Quarto and mystmd (`npm i -g mystmd`) and runs it.

## Validation

- The harness runs and **fails** on current `main`, reproducing B1–B9 and the pollution findings. Record the failing list in the PR description as the baseline.

## Risks

- The ASK governance hook blocks agent tool calls that mention `data/raw/`. The pollution test builds that path at runtime (`["data", "raw"].join("/")`). Its purpose is to test the tool, not to evade the hook in agent sessions. If the hook still blocks the edit, the user adds that fixture line by hand.
- Renderer versions drift. The harness records `quarto --version` / `myst --version` in its output, and CI pins them.

## Baseline (2026-09-26, before phases 2–7)

The harness fails on `main` @ `2ae1c07` with the same problem set on Quarto 1.9.36 / mystmd 1.10.1 and on Quarto 1.11.5 / mystmd 1.11.0 (53 problems on `ask-manuscript`, 59 on `myst-native`). The run reproduces B1, B2, B3 (the round-tripped `_quarto.yml` fails to render), B4 (16 "Invalid export article"), B5, B6/§3 (round-trip diffs), B7, and all of §4. It confirms the raw-data copy directly, which the review had only inferred. It also found one new issue: a non-GitHub `code-links` href becomes `project.github`, which MyST rejects (fixed in phase 3).

The fixture uses a root `index.qmd`, because Quarto 1.9.36 and 1.11.5 both fail on a `manuscript/index.qmd` article that has a code cell (`readfile …/manuscript/manuscript/index.qmd`), and `execute-dir: project` does not help. That result settles phase 8 step 5.
