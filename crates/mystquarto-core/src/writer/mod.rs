//! IR -> MyST / IR -> Quarto writers.
//!
//! **Label direction is asymmetric, deliberately.** [`crate::LabelRegistry`]
//! solves *normalization* (MyST-style `fig:samples` -> Quarto-legal
//! `fig-samples`, with cross-file collision handling) — a concept that only
//! exists in the MyST->Quarto direction, because Quarto's own ids are
//! already legal MyST label text (MyST "imposes no constraint" — reference
//! §3.3). So:
//!
//! - [`quarto::QuartoWriter`] consults a [`crate::LabelRegistry`] built over
//!   the MyST-sourced documents being converted.
//! - [`myst::MystWriter`] does **not** — it either passes a label through
//!   unchanged (the same-dialect MyST->MyST round-trip case, and the default
//!   for Quarto->MyST when no sidecar entry exists) or substitutes a
//!   sidecar-restored original spelling via a `restore` map
//!   (`(file, quarto_id) -> original MyST label`, built by
//!   [`crate::registry::sidecar::restore_labels`]). Building a second,
//!   reverse `LabelRegistry` for this would need to re-normalize already-
//!   normalized ids — a modeling error the phase spec's Phase 3 risk section
//!   warns against repeating ("the IR is wrong and Phase 4 needs an IR
//!   change more than twice"; the same discipline applies here to *not*
//!   forcing one struct to do two directions' jobs).

pub mod myst;
pub mod quarto;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diagnostics::{codes, Diagnostic, Severity};
use crate::preserve::{self, PreservedEntry};
use crate::reader::inline::{rewrite_line, InlineEvent};
use crate::{Block, Label, Span};

pub use myst::MystWriter;
pub use quarto::{resolve_embed_id, QuartoWriter};

/// Renders a `Preserved`/`Unmappable` block as its single-line, content-free
/// marker (reference §11, RD-2/RT-02) — shared by both writers, since the
/// marker syntax (an HTML comment) is dialect-agnostic: it renders as
/// nothing in both MyST and Quarto/Pandoc output, which is exactly why the
/// same syntax works as the round-trip anchor `crate::reader::preservation_marker_id`
/// recognizes regardless of which dialect is being read.
///
/// `original` empty means there is nothing to preserve (the "sidecar entry
/// for a marker being read back was missing" degrade — see
/// [`codes::block::PRESERVATION_ENTRY_MISSING`]): no sidecar entry is
/// written, and the returned marker says so rather than pointing at a
/// fabricated id that would perpetuate the same problem on a later round
/// trip.
///
/// `sink` bundles the two per-document accumulators
/// ([`QuartoWriter`]/[`MystWriter`]'s `preserved`/`diagnostics` fields) into
/// one argument — purely to keep this function's arity down; the two are
/// otherwise unrelated (one is sidecar data, one is user-facing output).
pub(crate) struct PreserveSink<'a> {
    pub preserved: &'a RefCell<BTreeMap<String, PreservedEntry>>,
    pub diagnostics: &'a RefCell<Vec<Diagnostic>>,
}

/// Bundles `render_preserved`'s per-call disposition (as opposed to
/// `PreserveSink`'s per-document accumulator state) — purely to keep that
/// function's arity down.
pub(crate) struct PreservedDisposition {
    pub code: &'static str,
    pub severity: Severity,
    /// The dialect `original` is written in — always the *writer's own*
    /// input dialect for a fresh `Unmappable` block (a `QuartoWriter` only
    /// ever processes MyST-sourced documents, so its `Unmappable` content
    /// is always `Dialect::Myst`; `MystWriter`'s production caller,
    /// `crate::pipeline::convert_quarto_to_myst_batch`, is symmetric).
    /// Recorded on the sidecar entry so a later reader can refuse to
    /// reparse it through the wrong dialect's parser — see
    /// `crate::preserve::Dialect`'s docs.
    pub dialect: crate::preserve::Dialect,
}

/// Which dialect a preserved construct's visible copy is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisibleIn {
    Quarto,
    Myst,
}

/// The class marking the visible copy of a preserved construct.
pub(crate) const PRESERVED_CLASS: &str = "mystquarto-preserved";

/// The marker comment (which the reverse conversion restores from exactly)
/// followed by the construct's original source as a visible code block, so
/// content without a target-dialect equivalent never disappears from the
/// rendered page.
pub(crate) fn render_preserved(
    sink: &PreserveSink,
    file: &Path,
    span: Span,
    kind: &str,
    disposition: PreservedDisposition,
    original: Vec<String>,
    visible_in: VisibleIn,
) -> Vec<String> {
    let PreservedDisposition {
        code,
        severity,
        dialect,
    } = disposition;
    if original.is_empty() {
        sink.diagnostics.borrow_mut().push(
            Diagnostic::new(
                Severity::Warning,
                codes::block::PRESERVATION_ENTRY_MISSING,
                "a preservation marker's sidecar entry was not found (missing or stale \
                 .mystquarto/preserved.json); its original content could not be restored",
            )
            .with_file(file.to_path_buf())
            .with_span(span),
        );
        return vec![
            "<!-- mystquarto: preservation entry missing, original content unavailable -->"
                .to_string(),
        ];
    }

    let id = preserve::entry_id(&original);
    let visible = visible_copy(&original, visible_in);
    sink.preserved.borrow_mut().insert(
        id.clone(),
        PreservedEntry {
            file: file.display().to_string(),
            line: span.start_line,
            code: code.to_string(),
            kind: kind.to_string(),
            dialect,
            original,
        },
    );
    sink.diagnostics.borrow_mut().push(
        Diagnostic::new(
            severity,
            code,
            format!("{kind} has no equivalent in the target dialect; preserved"),
        )
        .with_file(file.to_path_buf())
        .with_span(span)
        .with_preserved(id.clone()),
    );
    let mut out = vec![preserve::marker(code, kind, &id)];
    out.extend(visible);
    out
}

fn visible_copy(original: &[String], visible_in: VisibleIn) -> Vec<String> {
    let longest = original
        .iter()
        .map(|l| l.trim_start().chars().take_while(|c| *c == '`').count())
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    let mut out = match visible_in {
        VisibleIn::Quarto => vec![format!("{fence}{{.markdown .{PRESERVED_CLASS}}}")],
        VisibleIn::Myst => vec![
            format!("{fence}{{code-block}} markdown"),
            format!(":class: {PRESERVED_CLASS}"),
            String::new(),
        ],
    };
    out.extend(original.iter().cloned());
    out.push(fence);
    out
}

/// Extracts a short human-readable label for a preserved construct from a
/// reader-produced message (`BlockKind::Unmappable::reason`, e.g.
/// `"unrecognized MyST directive {glossary}"`): the text inside the first
/// `{...}`, matching the phase spec's own marker example
/// (`{glossary} preserved`). Falls back to `"construct"` when `reason` has
/// no such substring (e.g. a Quarto shortcode's reason, which names the
/// shortcode outside braces).
#[must_use]
pub(crate) fn preserved_kind(reason: &str) -> String {
    if let Some(start) = reason.find('{') {
        if let Some(end) = reason[start..].find('}') {
            return reason[start + 1..start + end].to_string();
        }
    }
    "construct".to_string()
}

/// Renders a block's `blank_lines_before` as that many blank lines, except
/// before the very first block in a sequence (nothing to separate it from).
/// Shared by both writers so vertical spacing round-trips identically
/// regardless of target dialect — this is what makes same-dialect
/// byte-identical round-trip possible (`crate::ir::Block::blank_lines_before`'s
/// whole reason for existing, RT-13).
pub(crate) fn push_spacing(out: &mut String, blank_lines_before: u8, is_first: bool) {
    if is_first {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for _ in 0..blank_lines_before {
        out.push('\n');
    }
}

/// Joins rendered block bodies (each already a `Vec<String>` of lines) with
/// single newlines, trimming nothing — callers control blank-line placement
/// via [`push_spacing`].
pub(crate) fn join_lines(lines: &[String]) -> String {
    lines.join("\n")
}

/// Collects every label spelling that should be recognized as a
/// cross-reference (rather than a citation) by [`rewrite_line`]'s
/// `known_labels` parameter, from every document in a conversion set —
/// both MyST-side raw labels and their normalized Quarto ids, since a
/// document's inline `@token` text is in whichever dialect *that* document
/// was read from, and a single writer call processes one document at a
/// time without re-deriving which dialect its own input was.
#[must_use]
pub fn known_reference_labels(registry: &crate::LabelRegistry) -> Vec<String> {
    let mut labels = Vec::new();
    for (_, myst_label, quarto_id) in registry.entries() {
        labels.push(myst_label.raw.clone());
        labels.push(quarto_id.to_string());
    }
    labels.sort();
    labels.dedup();
    labels
}

/// Rewrites every text-bearing line context a block can carry
/// (`Paragraph.lines`, a `Figure`/`Table`'s `caption`, a heading's `text`,
/// …) with dialect-specific inline rendering. Delegates the actual
/// per-event decision to `render`, so [`myst::MystWriter`] and
/// [`quarto::QuartoWriter`] each supply their own — this function only
/// owns "call [`rewrite_line`] on every line," not the rendering rules
/// themselves.
pub(crate) fn rewrite_lines(
    lines: &[String],
    known_labels: &[String],
    mut render: impl FnMut(InlineEvent) -> Option<String>,
) -> Vec<String> {
    lines
        .iter()
        .map(|l| rewrite_line(l, known_labels, &mut render))
        .collect()
}

/// Recursively renders nested `body: Vec<Block>` content (admonitions,
/// margins, tab items, blockquotes, theorems, generic directives) by
/// delegating to `render_block` for each child and joining with
/// [`push_spacing`] — shared so both writers implement nested-body
/// rendering identically rather than each hand-rolling the same loop.
pub(crate) fn render_body(
    body: &[Block],
    mut render_block: impl FnMut(&Block) -> Vec<String>,
) -> Vec<String> {
    let mut out = String::new();
    for (i, block) in body.iter().enumerate() {
        push_spacing(&mut out, block.blank_lines_before, i == 0);
        out.push_str(&join_lines(&render_block(block)));
    }
    out.lines().map(str::to_string).collect()
}

/// `(source file, MyST-side label as it will be emitted)` -> the string
/// [`myst::MystWriter`] should actually write for it. Built once per
/// conversion set from [`crate::registry::sidecar::restore_labels`]'s
/// output — see this module's docs on why this, not a second
/// `LabelRegistry`, is the right shape for the reverse direction.
pub type RestoreMap = std::collections::BTreeMap<(PathBuf, String), Label>;

/// Resolves what [`myst::MystWriter`] should emit for `label` as read in
/// `source`: the sidecar-restored original if `restore` has one, else
/// `label` unchanged (identity pass-through — correct both for the
/// same-dialect MyST->MyST case, where `restore` is always empty, and for
/// Quarto->MyST with no sidecar entry for this particular id).
#[must_use]
pub(crate) fn resolve_myst_label(source: &Path, label: &Label, restore: &RestoreMap) -> Label {
    restore
        .get(&(source.to_path_buf(), label.raw.clone()))
        .cloned()
        .unwrap_or_else(|| label.clone())
}

/// Swaps `from` to `to` on a content-file path and leaves every other
/// extension (and the whole directory/stem) untouched. Include directives
/// and the file writer both use this rule, so an include always names the
/// file that was actually written.
#[must_use]
pub fn swap_content_extension(path: &Path, from: &str, to: &str) -> PathBuf {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext == from => path.with_extension(to),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod include_path_tests {
    use super::swap_content_extension;
    use std::path::Path;

    #[test]
    fn include_targets_keep_the_underscore_and_swap_only_content_extensions() {
        let cases = [
            ("sections/_intro.qmd", "qmd", "md", "sections/_intro.md"),
            ("_intro.md", "md", "qmd", "_intro.qmd"),
            ("intro.md", "md", "qmd", "intro.qmd"),
            ("snippet.py", "md", "qmd", "snippet.py"),
            ("data/table.csv", "qmd", "md", "data/table.csv"),
        ];
        for (input, from, to, want) in cases {
            assert_eq!(
                swap_content_extension(Path::new(input), from, to),
                Path::new(want),
                "{input}"
            );
        }
    }
}

/// Code-block options in MyST spelling, in a fixed order: `linenos`,
/// `emphasize-lines`, `caption`, then anything else. Accepts either
/// dialect's names (Quarto's `code-line-numbers` / `filename`).
#[must_use]
pub fn myst_code_options(attrs: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    let numbers = attrs.get("code-line-numbers").map(String::as_str);
    if attrs.contains_key("linenos") || matches!(numbers, Some(n) if n != "false") {
        out.push(":linenos:".to_string());
    }
    let emphasis = attrs.get("emphasize-lines").cloned().or_else(|| {
        numbers
            .filter(|n| !matches!(*n, "true" | "false"))
            .map(str::to_string)
    });
    if let Some(lines) = emphasis {
        out.push(format!(":emphasize-lines: {lines}"));
    }
    if let Some(caption) = attrs.get("caption").or_else(|| attrs.get("filename")) {
        out.push(format!(":caption: {caption}"));
    }
    for (k, v) in attrs {
        if matches!(
            k.as_str(),
            "linenos" | "emphasize-lines" | "caption" | "code-line-numbers" | "filename"
        ) {
            continue;
        }
        out.push(if v.is_empty() {
            format!(":{k}:")
        } else {
            format!(":{k}: {v}")
        });
    }
    out
}

/// Code-block attributes in Quarto spelling: `code-line-numbers` (`true`,
/// or the emphasized lines), `filename`, then anything else.
#[must_use]
pub fn quarto_code_attrs(attrs: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let numbers = attrs
        .get("emphasize-lines")
        .cloned()
        .or_else(|| attrs.get("code-line-numbers").cloned())
        .or_else(|| attrs.contains_key("linenos").then(|| "true".to_string()));
    if let Some(n) = numbers {
        out.push(("code-line-numbers".to_string(), n));
    }
    if let Some(name) = attrs.get("filename").or_else(|| attrs.get("caption")) {
        out.push(("filename".to_string(), name.clone()));
    }
    for (k, v) in attrs {
        if matches!(
            k.as_str(),
            "linenos" | "emphasize-lines" | "caption" | "code-line-numbers" | "filename"
        ) {
            continue;
        }
        out.push((k.clone(), v.clone()));
    }
    out
}

/// A table as rows of cells, each cell a list of lines. The shared model
/// behind MyST `{list-table}`, Pandoc pipe tables and Pandoc grid tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellTable {
    pub header_rows: usize,
    pub rows: Vec<Vec<Vec<String>>>,
}

impl CellTable {
    /// Parses a MyST `{list-table}` body: `* - cell` starts a row, `  - cell`
    /// a further cell, and deeper-indented lines continue the current cell.
    #[must_use]
    pub fn from_list_table(body: &[String], header_rows: usize) -> Self {
        let mut rows: Vec<Vec<Vec<String>>> = Vec::new();
        for line in body {
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            if let Some(cell) = trimmed
                .strip_prefix("* - ")
                .or_else(|| (trimmed == "* -").then_some(""))
            {
                rows.push(vec![vec![cell.to_string()]]);
            } else if let Some(cell) = trimmed
                .strip_prefix("- ")
                .or_else(|| (trimmed == "-").then_some(""))
            {
                if indent > 0 {
                    if let Some(row) = rows.last_mut() {
                        row.push(vec![cell.to_string()]);
                        continue;
                    }
                }
            } else if let Some(cell) = rows.last_mut().and_then(|r| r.last_mut()) {
                if !trimmed.is_empty() || !cell.last().is_some_and(String::is_empty) {
                    cell.push(trimmed.to_string());
                }
            }
        }
        for row in &mut rows {
            for cell in row.iter_mut() {
                while cell.last().is_some_and(|l| l.is_empty()) {
                    cell.pop();
                }
            }
        }
        Self { header_rows, rows }
    }

    /// Parses Pandoc grid table lines (`+---+` borders, `|` cell lines, a
    /// `+===+` border after the header). Spans are not supported.
    #[must_use]
    pub fn from_grid(lines: &[String]) -> Option<Self> {
        let border = lines.first()?.trim();
        if !border.starts_with('+') {
            return None;
        }
        let cuts: Vec<usize> = border
            .char_indices()
            .filter(|(_, c)| *c == '+')
            .map(|(i, _)| i)
            .collect();
        if cuts.len() < 2 {
            return None;
        }
        let mut rows = Vec::new();
        let mut header_rows = 0;
        let mut current: Vec<Vec<String>> = vec![Vec::new(); cuts.len() - 1];
        for line in &lines[1..] {
            let line = line.trim();
            if line.starts_with('+') {
                rows.push(std::mem::replace(
                    &mut current,
                    vec![Vec::new(); cuts.len() - 1],
                ));
                if line.contains('=') {
                    header_rows = rows.len();
                }
            } else if line.starts_with('|') {
                for (c, cell) in current.iter_mut().enumerate() {
                    let text = line.get(cuts[c] + 1..cuts[c + 1]).unwrap_or("").trim();
                    cell.push(text.to_string());
                }
            }
        }
        for row in &mut rows {
            for cell in row.iter_mut() {
                while cell.last().is_some_and(String::is_empty) {
                    cell.pop();
                }
                while cell.first().is_some_and(String::is_empty) {
                    cell.remove(0);
                }
            }
        }
        Some(Self { header_rows, rows })
    }

    fn columns(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// Pipe-table lines when every cell is a single line and there is one
    /// header row; grid-table lines otherwise.
    #[must_use]
    pub fn to_markdown(&self) -> Vec<String> {
        let single_line = self.rows.iter().flatten().all(|c| c.len() <= 1);
        if single_line && self.header_rows == 1 {
            self.to_pipe()
        } else {
            self.to_grid()
        }
    }

    fn cell_text(cell: &[String]) -> String {
        cell.first().cloned().unwrap_or_default()
    }

    fn to_pipe(&self) -> Vec<String> {
        let cols = self.columns();
        let row_line = |row: &Vec<Vec<String>>| {
            let cells: Vec<String> = (0..cols)
                .map(|c| {
                    row.get(c)
                        .map(|cell| Self::cell_text(cell))
                        .unwrap_or_default()
                })
                .collect();
            format!("| {} |", cells.join(" | "))
        };
        let mut out = vec![row_line(&self.rows[0])];
        out.push(format!("|{}|", vec!["---"; cols].join("|")));
        out.extend(self.rows[1..].iter().map(row_line));
        out
    }

    fn to_grid(&self) -> Vec<String> {
        let cols = self.columns();
        let widths: Vec<usize> = (0..cols)
            .map(|c| {
                self.rows
                    .iter()
                    .filter_map(|r| r.get(c))
                    .flatten()
                    .map(|l| l.chars().count())
                    .max()
                    .unwrap_or(0)
                    .max(3)
            })
            .collect();
        let border = |ch: char| {
            let parts: Vec<String> = widths
                .iter()
                .map(|w| ch.to_string().repeat(w + 2))
                .collect();
            format!("+{}+", parts.join("+"))
        };
        let mut out = vec![border('-')];
        for (r, row) in self.rows.iter().enumerate() {
            let height = row.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for line in 0..height {
                let parts: Vec<String> = (0..cols)
                    .map(|c| {
                        let text = row
                            .get(c)
                            .and_then(|cell| cell.get(line))
                            .cloned()
                            .unwrap_or_default();
                        let pad = widths[c] - text.chars().count();
                        format!(" {text}{} ", " ".repeat(pad))
                    })
                    .collect();
                out.push(format!("|{}|", parts.join("|")));
            }
            out.push(border(if r + 1 == self.header_rows { '=' } else { '-' }));
        }
        out
    }

    /// `{list-table}` body lines.
    #[must_use]
    pub fn to_list_table(&self) -> Vec<String> {
        let mut out = Vec::new();
        for row in &self.rows {
            for (c, cell) in row.iter().enumerate() {
                let marker = if c == 0 { "* - " } else { "  - " };
                let mut lines = cell.iter();
                out.push(format!(
                    "{marker}{}",
                    lines.next().cloned().unwrap_or_default()
                ));
                for more in lines {
                    out.push(if more.is_empty() {
                        String::new()
                    } else {
                        format!("    {more}")
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod table_tests {
    use super::CellTable;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn a_single_line_list_table_becomes_a_pipe_table() {
        let t = CellTable::from_list_table(&lines("* - A\n  - B\n* - 1\n  - 2\n"), 1);
        assert_eq!(t.to_markdown(), lines("| A | B |\n|---|---|\n| 1 | 2 |"));
    }

    #[test]
    fn multi_line_cells_become_a_grid_table_that_parses_back() {
        let t = CellTable::from_list_table(
            &lines("* - Name\n  - Notes\n* - x\n  - first\n\n    second\n"),
            1,
        );
        let grid = t.to_markdown();
        assert!(grid[0].starts_with('+'), "{grid:?}");
        assert!(grid.iter().any(|l| l.contains('=')));
        let back = CellTable::from_grid(&grid).unwrap();
        assert_eq!(back.header_rows, 1);
        assert_eq!(back.rows[1][1], vec!["first", "", "second"]);
        assert_eq!(
            back.to_list_table(),
            lines("* - Name\n  - Notes\n* - x\n  - first\n\n    second")
        );
    }
}
