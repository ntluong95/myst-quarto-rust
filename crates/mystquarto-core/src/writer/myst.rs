//! IR -> MyST writer. Emits **modern mystmd v1 only** — no legacy role ever
//! reaches output, regardless of whether it originated as legacy MyST syntax
//! (read by [`crate::MystReader`], which accepts it) or was never legacy at
//! all (a same-dialect round-trip of already-modern text). This is the
//! accepted "modern MyST only" decision (`plan.md`'s Accepted Decisions
//! table) enforced at the one place it can be violated: text rendering.
//!
//! See this module's parent for why label handling here is a `restore`
//! lookup, not a [`crate::LabelRegistry`] — that struct solves the
//! MyST->Quarto normalization problem, which does not exist in this
//! direction.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use crate::diagnostics::Diagnostic;
use crate::preserve::PreservedEntry;
use crate::reader::inline::InlineEvent;
use crate::writer::{push_spacing, resolve_myst_label, RestoreMap};
use crate::{AdmonitionKind, Block, BlockKind, Document, EmbedTarget, FigureSource, Label};

/// Renders a [`Document`] (from either dialect's reader) as MyST (`.md`)
/// text.
pub struct MystWriter<'a> {
    restore: &'a RestoreMap,
    known_labels: Vec<String>,
    /// See [`crate::writer::quarto::QuartoWriter`]'s identical fields —
    /// same accumulator role, shared [`crate::writer::render_preserved`]
    /// mechanism.
    preserved: RefCell<BTreeMap<String, PreservedEntry>>,
    diagnostics: RefCell<Vec<Diagnostic>>,
}

impl<'a> MystWriter<'a> {
    /// `restore` is `crate::registry::sidecar::restore_labels`'s output —
    /// pass an empty map for a same-dialect round-trip (labels then pass
    /// through completely unchanged, which is what makes byte-identical
    /// round-trip on the Stable class possible).
    ///
    /// `known_labels` is every label actually **defined** across the
    /// documents in this conversion set — not from a
    /// [`crate::LabelRegistry`] (that struct solves MyST->Quarto
    /// normalization, a problem that does not exist in this direction; see
    /// this module's docs), and deliberately **not** only from `restore`'s
    /// keys either: a Quarto project with no sidecar (never round-tripped)
    /// still has real, locally-defined labels that must be recognized as
    /// cross-references rather than misclassified as citations. See
    /// `crate::pipeline::labels_defined_in` for how a caller builds this
    /// list from a batch's parsed documents.
    #[must_use]
    pub fn new(restore: &'a RestoreMap, known_labels: Vec<String>) -> Self {
        Self {
            restore,
            known_labels,
            preserved: RefCell::new(BTreeMap::new()),
            diagnostics: RefCell::new(Vec::new()),
        }
    }

    /// Frontmatter is mapped field-by-field via
    /// [`crate::frontmatter::quarto_to_myst`] (reference §8.4) —
    /// `jupyter`/`engine` -> `kernelspec`, `format` -> `exports`, etc. `doc`'s
    /// own `engine` (detected from the body by
    /// [`crate::reader::quarto`]'s `detect_engine` — an `{r}` code-cell
    /// implies knitr even with no `engine:`/`jupyter:` field to read) is
    /// passed through so a page relying on Quarto's implicit engine
    /// detection still gets a kernelspec rather than silently losing its
    /// executability. Applying this unconditionally is correct for this
    /// writer's only current caller
    /// ([`crate::pipeline::convert_quarto_to_myst_batch`], always
    /// Quarto-sourced); for a same-dialect MyST->MyST round trip (this
    /// writer's other documented use, see this module's docs) it is a
    /// no-op, since a genuine MyST source document has no `jupyter`/`format`/
    /// `engine`/`crossref.eq-prefix` keys for it to touch.
    #[must_use]
    pub fn write(
        &self,
        doc: &Document,
    ) -> (String, Vec<Diagnostic>, BTreeMap<String, PreservedEntry>) {
        let mut out = String::new();
        let mut warnings = Vec::new();
        if let Some(fm) = &doc.frontmatter {
            let (mapped, fm_warnings) = crate::frontmatter::quarto_to_myst(fm, doc.engine);
            warnings = fm_warnings;
            out.push_str("---\n");
            out.push_str(&mapped);
            out.push_str("---\n");
        }
        for (i, block) in doc.blocks.iter().enumerate() {
            push_spacing(
                &mut out,
                block.blank_lines_before,
                i == 0 && doc.frontmatter.is_none(),
            );
            // At least one blank line after frontmatter; the source's own
            // blank lines already count.
            if doc.frontmatter.is_some() && i == 0 && block.blank_lines_before == 0 {
                out.push('\n');
            }
            let rendered = self.render_block(block, &doc.source);
            out.push_str(&rendered.join("\n"));
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        warnings.extend(self.diagnostics.take());
        (out, warnings, self.preserved.take())
    }

    fn rewrite(&self, lines: &[String]) -> Vec<String> {
        crate::writer::rewrite_lines(lines, &self.known_labels, |event| self.render_event(event))
    }

    fn render_event(&self, event: InlineEvent) -> Option<String> {
        match event {
            // `rewrite_line` never invokes `render` for a `Citation` event
            // — it always copies modern citation syntax through verbatim
            // itself (reference §4: identical in both dialects). This arm
            // exists only so the match is exhaustive.
            InlineEvent::Citation(_) => None,
            // A cross-reference token passes through unchanged unless a
            // restore map (built from the sidecar) says otherwise — see
            // `resolve_reference_label`.
            InlineEvent::CrossReference(key) => {
                let restored = self.resolve_reference_label(&key);
                (restored != key).then(|| format!("@{restored}"))
            }
            InlineEvent::LegacyRole { role, target } => render_role_to_myst(&role, &target),
            // Modern MyST inline eval — `{eval}`expr`` — regardless of
            // which Quarto engine tag the source used, since MyST's own
            // eval role carries no engine parameter (reference §5).
            InlineEvent::JupyterEval { expr, .. } => Some(format!("{{eval}}`{expr}`")),
            // Quarto knitr-only inline syntax has no MyST equivalent
            // (reference §6): rendered as MyST's eval role, same as the
            // Jupyter case, since MyST only ever executes via Jupyter — an
            // `IRkernel` (or equivalent) must exist for this to actually
            // run, which Phase 7's diagnostics will warn about once it
            // exists.
            InlineEvent::KnitrEval(expr) => Some(format!("{{eval}}`{expr}`")),
            // Already MyST syntax.
            InlineEvent::AnchorReference(_) => None,
            InlineEvent::DocumentLink {
                text,
                path,
                fragment,
            } => {
                let path = crate::writer::swap_content_extension(Path::new(&path), "qmd", "md");
                Some(format!("[{text}]({}{fragment})", path.display()))
            }
            InlineEvent::PandocInline { role, content } => Some(format!("{{{role}}}`{content}`")),
        }
    }

    /// Looks up `key` (a raw reference token as it appeared in the source
    /// being converted — either a MyST colon-label or a Quarto hyphen-id)
    /// against `restore`, trying every source file recorded there (the
    /// same cross-file resolution [`crate::LabelRegistry::resolve_reference`]
    /// does, but over the restore map instead of a registry). Falls back to
    /// `key` unchanged when nothing matches.
    fn resolve_reference_label(&self, key: &str) -> String {
        self.restore
            .iter()
            .find(|((_, id), _)| id == key)
            .map(|(_, label)| label.raw.clone())
            .unwrap_or_else(|| key.to_string())
    }

    fn render_block(&self, block: &Block, source: &Path) -> Vec<String> {
        match &block.kind {
            BlockKind::Heading { level, text, label } => {
                self.heading(*level, text, label.as_ref(), source)
            }
            BlockKind::Paragraph { lines } => self.rewrite(lines),
            BlockKind::CodeCell {
                lang,
                options,
                body,
                label,
            } => self.code_cell(lang, options, body, label.as_ref(), source),
            BlockKind::StaticCode { lang, body, attrs } => static_code(lang, body, attrs),
            BlockKind::Figure {
                src,
                caption,
                label,
                attrs,
            } => self.figure(src, caption, label.as_ref(), attrs, source),
            BlockKind::Table {
                caption,
                rows,
                label,
            } => self.table(caption, rows, label.as_ref(), source),
            BlockKind::Math { body, label } => self.math(body, label.as_ref(), source),
            BlockKind::Admonition {
                kind,
                title,
                body,
                collapse,
            } => self.admonition(*kind, title.as_deref(), body, *collapse, source),
            BlockKind::TabSet { items } => self.tab_set(items, source),
            BlockKind::Margin { body } => self.directive_wrap("margin", body, source),
            BlockKind::Include { target, .. } => fenced(
                '`',
                &format!("{{include}} {}", myst_include_target(target).display()),
                Vec::new(),
                Vec::new(),
            ),
            BlockKind::Embed { target, label } => self.embed(target, label.as_ref(), source),
            BlockKind::Blockquote {
                body,
                attribution,
                class,
            } => self.blockquote(body, attribution.as_deref(), class.as_deref(), source),
            BlockKind::Theorem {
                thm_type,
                label,
                body,
            } => self.theorem(thm_type, label.as_ref(), body, source),
            BlockKind::Directive {
                name,
                attrs,
                body,
                label,
            } => self.directive(name, attrs, label.as_ref(), body, source),
            BlockKind::Comment { text, .. } => vec![format!("% {text}")],
            BlockKind::Target { label } => {
                vec![format!("({})=", self.myst_label(source, label).raw)]
            }
            BlockKind::Raw { format, body } => raw(format, body),
            BlockKind::BlockBreak => vec!["+++".to_string()],
            // Empty `original` is the "missing-sidecar-entry"/"foreign
            // dialect, but no real content to fall back to" degrade —
            // `render_preserved` handles it (a fresh marker + a Warning
            // diagnostic), see its docs. Non-empty is handled by the next
            // arm.
            BlockKind::Preserved { original, .. } if original.is_empty() => {
                crate::writer::render_preserved(
                    &crate::writer::PreserveSink {
                        preserved: &self.preserved,
                        diagnostics: &self.diagnostics,
                    },
                    source,
                    block.span,
                    "content",
                    crate::writer::PreservedDisposition {
                        code: crate::diagnostics::codes::block::PRESERVED_RESTORED_OPAQUE,
                        severity: crate::diagnostics::Severity::LossyExpected,
                        dialect: crate::preserve::Dialect::Unknown,
                    },
                    original.clone(),
                    crate::writer::VisibleIn::Myst,
                )
            }
            // Non-empty `Preserved` content is native to *this* writer's
            // dialect in every reachable pipeline path (C2 fix): a fresh
            // `Unmappable` marker's `Dialect` is always the *reading*
            // writer's own — `MystWriter` records `Quarto`, `QuartoWriter`
            // records `Myst` (see each `Unmappable` arm below/in
            // `writer::quarto`) — so a reader only ever finds a
            // *foreign*-dialect entry for a marker its own writers wrote
            // (never a matching one; see
            // `crate::reader::myst::MystReader::push_preserved_or_marker`'s
            // doc), and foreign-to-the-reader is native-to-the-writer in a
            // two-dialect system. Verbatim passthrough here is therefore
            // correct, not a missed opportunity to re-wrap it in a fresh
            // marker. (The one case this doesn't cover — a hand-crafted
            // input file containing a marker whose recorded dialect matches
            // its *own* reader — reaches this arm with reader-native, not
            // writer-native, content; unreachable through this tool's own
            // writers, and still safe: `Preserved`'s `original` is never
            // reparsed, so the worst outcome is literal foreign-dialect
            // text appearing in the output, not silent corruption.)
            BlockKind::Preserved { original, .. } => original.clone(),
            BlockKind::Unmappable { original, reason } => {
                let is_path_safety = reason.contains("escapes")
                    || reason.contains("cycle")
                    || reason.contains("depth")
                    || reason.contains("absolute path");
                crate::writer::render_preserved(
                    &crate::writer::PreserveSink {
                        preserved: &self.preserved,
                        diagnostics: &self.diagnostics,
                    },
                    source,
                    block.span,
                    &crate::writer::preserved_kind(reason),
                    crate::writer::PreservedDisposition {
                        code: if is_path_safety {
                            crate::diagnostics::codes::io::PATH_SAFETY_REFUSED
                        } else {
                            crate::diagnostics::codes::block::UNMAPPABLE_PRESERVED
                        },
                        severity: if is_path_safety {
                            crate::diagnostics::Severity::Warning
                        } else {
                            crate::diagnostics::Severity::LossyExpected
                        },
                        dialect: crate::preserve::Dialect::Quarto,
                    },
                    original.clone(),
                    crate::writer::VisibleIn::Myst,
                )
            }
        }
    }

    fn myst_label(&self, source: &Path, label: &Label) -> Label {
        resolve_myst_label(source, label, self.restore)
    }

    fn heading(&self, level: u8, text: &str, label: Option<&Label>, source: &Path) -> Vec<String> {
        let text = self
            .rewrite(std::slice::from_ref(&text.to_string()))
            .remove(0);
        let hashes = "#".repeat(level as usize);
        match label {
            Some(l) => vec![
                format!("({})=", self.myst_label(source, l).raw),
                String::new(),
                format!("{hashes} {text}"),
            ],
            None => vec![format!("{hashes} {text}")],
        }
    }

    /// mystmd reads a cell's label, caption and other options from `#|`
    /// lines at the top of its body; tags stay a directive option.
    fn code_cell(
        &self,
        lang: &str,
        options: &crate::CellOptions,
        body: &[String],
        label: Option<&Label>,
        source: &Path,
    ) -> Vec<String> {
        let mut opts = Vec::new();
        if !options.tags.is_empty() {
            opts.push(format!(":tags: [{}]", options.tags.join(", ")));
        }
        let mut lines = Vec::new();
        if let Some(l) = label {
            lines.push(format!("#| label: {}", self.myst_label(source, l).raw));
        }
        if let Some(caption) = &options.caption {
            lines.push(format!("#| caption: {caption}"));
        }
        for (k, v) in &options.extra {
            lines.push(format!("#| {k}: {v}"));
        }
        lines.extend(body.iter().cloned());
        fenced('`', &format!("{{code-cell}} {lang}"), opts, lines)
    }

    fn figure(
        &self,
        src: &FigureSource,
        caption: &[String],
        label: Option<&Label>,
        attrs: &std::collections::BTreeMap<String, String>,
        source: &Path,
    ) -> Vec<String> {
        let arg = match src {
            FigureSource::Path(p) => p.display().to_string(),
            FigureSource::CellRef { label, .. } => format!("#{}", label.raw),
            FigureSource::Panel(subfigures) => {
                return self.figure_panel(subfigures, caption, label, attrs, source)
            }
        };
        let mut opts = Vec::new();
        if let Some(l) = label {
            opts.push(format!(":label: {}", self.myst_label(source, l).raw));
        }
        for (k, v) in attrs {
            if k == "id" {
                continue;
            }
            if k == "alt" && caption.first().map(|c| c.trim()) == Some(v.trim()) {
                continue;
            }
            opts.push(format!(":{k}: {v}"));
        }
        fenced(
            ':',
            &format!("{{figure}} {arg}"),
            opts,
            self.rewrite(caption),
        )
    }

    /// Subfigures nest inside a `{figure}`; Quarto's `layout-ncol` /
    /// `layout-nrow` travel as a `layout-ncol-N` class, since MyST has no
    /// such option.
    fn figure_panel(
        &self,
        subfigures: &[Block],
        caption: &[String],
        label: Option<&Label>,
        attrs: &std::collections::BTreeMap<String, String>,
        source: &Path,
    ) -> Vec<String> {
        let mut opts = Vec::new();
        if let Some(l) = label {
            opts.push(format!(":label: {}", self.myst_label(source, l).raw));
        }
        let mut classes: Vec<String> = attrs
            .get("class")
            .map(|c| c.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
        for (k, v) in attrs {
            match k.as_str() {
                "layout-ncol" | "layout-nrow" => classes.push(format!("{k}-{v}")),
                "class" => {}
                _ => opts.push(format!(":{k}: {v}")),
            }
        }
        if !classes.is_empty() {
            opts.push(format!(":class: {}", classes.join(" ")));
        }
        let mut body = Vec::new();
        for (i, sub) in subfigures.iter().enumerate() {
            if i > 0 {
                body.push(String::new());
            }
            body.extend(self.render_block(sub, source));
        }
        if !caption.is_empty() {
            body.push(String::new());
            body.extend(self.rewrite(caption));
        }
        fenced(':', "{figure}", opts, body)
    }

    fn table(
        &self,
        caption: &[String],
        rows: &[String],
        label: Option<&Label>,
        source: &Path,
    ) -> Vec<String> {
        let label_opt = label.map(|l| format!(":label: {}", self.myst_label(source, l).raw));
        // mystmd has no grid tables: a grid table becomes a `{list-table}`.
        if let Some(grid) = crate::writer::CellTable::from_grid(rows) {
            let mut opts = Vec::new();
            if grid.header_rows > 0 {
                opts.push(format!(":header-rows: {}", grid.header_rows));
            }
            opts.extend(label_opt);
            let title = self.rewrite(caption).join(" ");
            let open = if title.is_empty() {
                "{list-table}".to_string()
            } else {
                format!("{{list-table}} {title}")
            };
            return fenced('`', &open, opts, grid.to_list_table());
        }
        let opts: Vec<String> = label_opt.into_iter().collect();
        let mut body = self.rewrite(caption);
        if !body.is_empty() {
            body.push(String::new());
        }
        body.extend(rows.iter().cloned());
        fenced(':', "{table}", opts, body)
    }

    fn math(&self, body: &[String], label: Option<&Label>, source: &Path) -> Vec<String> {
        let opts: Vec<String> = label
            .map(|l| format!(":label: {}", self.myst_label(source, l).raw))
            .into_iter()
            .collect();
        fenced('`', "{math}", opts, body.to_vec())
    }

    /// Typed admonitions carry their title as the argument
    /// (`:::{warning} Careful`). A collapsible one is `:class: dropdown`,
    /// closed unless `:open:`; a `{dropdown}` is closed by default too.
    fn admonition(
        &self,
        kind: AdmonitionKind,
        title: Option<&str>,
        body: &[Block],
        collapse: Option<bool>,
        source: &Path,
    ) -> Vec<String> {
        let name = admonition_name(kind);
        let open = match title {
            Some(title) => format!("{{{name}}} {title}"),
            None => format!("{{{name}}}"),
        };
        let mut opts = Vec::new();
        if kind != AdmonitionKind::Dropdown && collapse.is_some() {
            opts.push(":class: dropdown".to_string());
        }
        if collapse == Some(false) {
            opts.push(":open:".to_string());
        }
        fenced(':', &open, opts, self.render_nested(body, source))
    }

    fn tab_set(&self, items: &[crate::TabItem], source: &Path) -> Vec<String> {
        let mut body = Vec::new();
        for item in items {
            body.extend(fenced(
                ':',
                &format!("{{tab-item}} {}", item.label),
                Vec::new(),
                self.render_nested(&item.body, source),
            ));
        }
        fenced(':', "{tab-set}", Vec::new(), body)
    }

    fn embed(&self, target: &EmbedTarget, label: Option<&Label>, source: &Path) -> Vec<String> {
        let arg = match target {
            EmbedTarget::Label(l) => l.raw.clone(),
            EmbedTarget::NotebookCell { cell_label, .. } => cell_label.raw.clone(),
        };
        let opts: Vec<String> = label
            .map(|l| format!(":label: {}", self.myst_label(source, l).raw))
            .into_iter()
            .collect();
        fenced('`', &format!("{{embed}} #{arg}"), opts, Vec::new())
    }

    fn blockquote(
        &self,
        body: &[Block],
        attribution: Option<&[String]>,
        class: Option<&str>,
        source: &Path,
    ) -> Vec<String> {
        let mut lines = self.render_nested(body, source);
        // MyST reads `-- Author` as the attribution only as its own
        // paragraph.
        if let Some(attribution) = attribution {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.extend(attribution.iter().map(|l| format!("-- {l}")));
        }
        let name = class.unwrap_or("blockquote");
        fenced(':', &format!("{{{name}}}"), Vec::new(), lines)
    }

    fn theorem(
        &self,
        thm_type: &str,
        label: Option<&Label>,
        body: &[Block],
        source: &Path,
    ) -> Vec<String> {
        let opts: Vec<String> = label
            .map(|l| format!(":label: {}", self.myst_label(source, l).raw))
            .into_iter()
            .collect();
        fenced(
            ':',
            &format!("{{prf:{thm_type}}}"),
            opts,
            self.render_nested(body, source),
        )
    }

    fn directive_wrap(&self, name: &str, body: &[Block], source: &Path) -> Vec<String> {
        fenced(
            ':',
            &format!("{{{name}}}"),
            Vec::new(),
            self.render_nested(body, source),
        )
    }

    fn directive(
        &self,
        name: &str,
        attrs: &std::collections::BTreeMap<String, String>,
        label: Option<&Label>,
        body: &[Block],
        source: &Path,
    ) -> Vec<String> {
        let mut opts: Vec<String> = label
            .map(|l| format!(":label: {}", self.myst_label(source, l).raw))
            .into_iter()
            .collect();
        for (k, v) in attrs {
            if k != crate::reader::myst::ARGUMENT {
                opts.push(format!(":{k}: {v}"));
            }
        }
        let open = match attrs.get(crate::reader::myst::ARGUMENT) {
            Some(arg) => format!("{{{name}}} {arg}"),
            None => format!("{{{name}}}"),
        };
        fenced(':', &open, opts, self.render_nested(body, source))
    }

    fn render_nested(&self, body: &[Block], source: &Path) -> Vec<String> {
        crate::writer::render_body(body, |b| self.render_block(b, source))
    }
}

/// The include target as written on disk: same directory and stem (a
/// leading `_` included, since both Quarto and MyST use it to mark a
/// partial), with only a `.qmd` extension swapped to `.md` — exactly the
/// rename the file writer applies, so the directive always resolves.
fn myst_include_target(target: &std::path::Path) -> std::path::PathBuf {
    crate::writer::swap_content_extension(target, "qmd", "md")
}

fn admonition_name(kind: AdmonitionKind) -> &'static str {
    match kind {
        AdmonitionKind::Note => "note",
        AdmonitionKind::Warning => "warning",
        AdmonitionKind::Tip => "tip",
        AdmonitionKind::Important => "important",
        AdmonitionKind::Caution => "caution",
        AdmonitionKind::Danger => "danger",
        AdmonitionKind::Error => "error",
        AdmonitionKind::Hint => "hint",
        AdmonitionKind::SeeAlso => "seealso",
        AdmonitionKind::Attention => "attention",
        AdmonitionKind::Dropdown => "dropdown",
    }
}

/// A directive fenced so nothing inside it can close it early: the fence is
/// one longer than the longest same-character fence in the body. Colon
/// fences for markdown bodies, backticks for code-like ones. Options follow
/// the opener; a blank line separates them from the body.
fn fenced(fence_char: char, open: &str, options: Vec<String>, body: Vec<String>) -> Vec<String> {
    let inner = body
        .iter()
        .map(|l| {
            l.trim_start()
                .chars()
                .take_while(|c| *c == fence_char)
                .count()
        })
        .filter(|n| *n >= 3)
        .max()
        .unwrap_or(0);
    let fence = fence_char.to_string().repeat((inner + 1).max(3));
    let mut out = vec![format!("{fence}{open}")];
    let has_options = !options.is_empty();
    out.extend(options);
    if has_options && !body.is_empty() {
        out.push(String::new());
    }
    out.extend(body);
    out.push(fence);
    out
}

/// Code with no options stays a plain fence; `mermaid` is a directive; code
/// with options (from MyST `{code-block}` or Quarto code attributes) is a
/// `{code-block}` with MyST option names.
fn static_code(
    lang: &Option<String>,
    body: &[String],
    attrs: &std::collections::BTreeMap<String, String>,
) -> Vec<String> {
    if lang.as_deref() == Some("mermaid") {
        return fenced('`', "{mermaid}", Vec::new(), body.to_vec());
    }
    let opts = crate::writer::myst_code_options(attrs);
    if opts.is_empty() {
        return fenced(
            '`',
            lang.as_deref().unwrap_or(""),
            Vec::new(),
            body.to_vec(),
        );
    }
    let open = match lang {
        Some(l) => format!("{{code-block}} {l}"),
        None => "{code-block}".to_string(),
    };
    fenced('`', &open, opts, body.to_vec())
}

fn raw(format: &str, body: &[String]) -> Vec<String> {
    fenced('`', &format!("{{raw}} {format}"), Vec::new(), body.to_vec())
}

/// Renders every `{name}`content`` form to **modern MyST**, never
/// re-emitting the legacy spelling — this is the one function a grep-based
/// test can point at to prove "no MyST output contains any legacy role."
fn render_role_to_myst(role: &str, target: &str) -> Option<String> {
    Some(match role {
        "cite" | "cite:p" => format!("[@{target}]"),
        "cite:t" => format!("@{target}"),
        "numref" | "ref" => format!("@{target}"),
        "eq" => {
            let label = if target.starts_with("eq-") || target.starts_with("eq:") {
                target.to_string()
            } else {
                format!("eq-{target}")
            };
            format!("@{label}")
        }
        "doc" => {
            let stem = target.strip_suffix(".qmd").unwrap_or(target);
            format!("[{target}]({stem}.md)")
        }
        // Already-modern MyST role syntax: passes through unchanged (this
        // function only exists to guarantee *legacy* forms never survive;
        // modern roles are, definitionally, already correct MyST).
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Attrs, Engine};
    use crate::{PreservationStore, ReaderContext, Span};

    fn block(kind: BlockKind, blank: u8) -> Block {
        Block {
            kind,
            span: Span::single(1),
            blank_lines_before: blank,
        }
    }

    #[test]
    fn same_dialect_round_trip_is_byte_identical_on_a_modern_myst_paragraph() {
        let input = "(sec:data)=\n\n## Data\n\nSome prose with [@smith2020] and @sec:data.\n";
        let reader = crate::MystReader::new(ReaderContext::new("a.md"));
        let doc = reader.read_str(input).unwrap();
        let restore = RestoreMap::new();
        let writer = MystWriter::new(&restore, Vec::new());
        let (out, _, _) = writer.write(&doc);
        assert_eq!(out, input);
    }

    /// Proves the writer actually calls
    /// `crate::frontmatter::quarto_to_myst` — `jupyter` must become
    /// `kernelspec` in the rendered `.md` text.
    #[test]
    fn jupyter_frontmatter_is_mapped_to_kernelspec_by_the_writer() {
        let raw = "title: Sample\njupyter: python3\n";
        let doc = Document {
            frontmatter: Some(crate::ir::Frontmatter {
                raw: raw.to_string(),
                parsed: crate::YamlValue::Mapping(
                    crate::yaml::parse_mapping(raw).expect("valid YAML"),
                ),
            }),
            blocks: vec![block(
                BlockKind::Paragraph {
                    lines: vec!["Body.".to_string()],
                },
                0,
            )],
            source: "a.md".into(),
            engine: Some(Engine::Jupyter),
        };
        let restore = RestoreMap::new();
        let writer = MystWriter::new(&restore, Vec::new());
        let (out, _, _) = writer.write(&doc);
        assert!(out.contains("kernelspec:"));
        assert!(out.contains("name: python3"));
        assert!(!out.contains("jupyter:"));
    }

    #[test]
    fn legacy_cite_role_is_rewritten_to_modern_bracket_syntax() {
        let doc = Document {
            frontmatter: None,
            blocks: vec![block(
                BlockKind::Paragraph {
                    lines: vec!["See {cite}`smith2020` here.".to_string()],
                },
                0,
            )],
            source: "a.md".into(),
            engine: Some(Engine::Jupyter),
        };
        let restore = RestoreMap::new();
        let writer = MystWriter::new(&restore, Vec::new());
        let (out, _, _) = writer.write(&doc);
        assert_eq!(out.trim(), "See [@smith2020] here.");
        assert!(!out.contains("{cite}"));
    }

    #[test]
    fn no_legacy_role_syntax_survives_a_battery_of_legacy_inputs() {
        let text = "\
{cite}`a` and {cite:t}`b` and {cite:p}`c`.
{numref}`fig-x` and {ref}`label` and {eq}`eq-y`.
{doc}`other.md`
";
        let reader = crate::MystReader::new(ReaderContext::new("a.md"));
        let doc = reader.read_str(text).unwrap();
        let restore = RestoreMap::new();
        let writer = MystWriter::new(&restore, Vec::new());
        let (out, _, _) = writer.write(&doc);
        for legacy in [
            "{cite}", "{cite:t}", "{cite:p}", "{numref}", "{ref}", "{eq}", "{doc}",
        ] {
            assert!(
                !out.contains(legacy),
                "found legacy role {legacy} in:\n{out}"
            );
        }
    }

    #[test]
    fn sidecar_restored_label_is_used_instead_of_the_quarto_id() {
        let doc = Document {
            frontmatter: None,
            blocks: vec![block(
                BlockKind::Figure {
                    src: FigureSource::Path("img.png".into()),
                    caption: vec![],
                    label: Some(Label::new("fig-samples")),
                    attrs: Attrs::new(),
                },
                0,
            )],
            source: "article.md".into(),
            engine: Some(Engine::Jupyter),
        };
        let mut restore = RestoreMap::new();
        restore.insert(
            ("article.md".into(), "fig-samples".to_string()),
            Label::new("fig:samples"),
        );
        let writer = MystWriter::new(&restore, Vec::new());
        let (out, _, _) = writer.write(&doc);
        assert!(out.contains(":label: fig:samples"));
    }

    #[test]
    fn preserved_block_reader_writer_round_trip() {
        let mut preserved = PreservationStore::default();
        preserved.insert("abc", vec!["% restored comment".to_string()]);
        let reader = crate::MystReader::new(ReaderContext {
            preserved,
            ..ReaderContext::new("a.md")
        });
        let doc = reader
            .read_str("<!-- mystquarto MQ0203: x see .mystquarto/preserved.json#abc -->\n")
            .unwrap();
        let restore = RestoreMap::new();
        let writer = MystWriter::new(&restore, Vec::new());
        let (out, _, _) = writer.write(&doc);
        assert_eq!(out.trim(), "% restored comment");
    }

    /// Counts every block in a tree, by kind name, so two parses can be
    /// compared structurally without their line spans.
    fn shape(blocks: &[Block], out: &mut Vec<String>) {
        for b in blocks {
            let name = format!("{:?}", b.kind);
            out.push(
                name.split(|c: char| !c.is_alphanumeric())
                    .next()
                    .unwrap_or("")
                    .to_string(),
            );
            match &b.kind {
                BlockKind::Admonition { body, .. }
                | BlockKind::Margin { body }
                | BlockKind::Blockquote { body, .. }
                | BlockKind::Theorem { body, .. }
                | BlockKind::Directive { body, .. } => shape(body, out),
                BlockKind::TabSet { items } => {
                    for item in items {
                        shape(&item.body, out);
                    }
                }
                BlockKind::Figure {
                    src: FigureSource::Panel(subs),
                    ..
                } => shape(subs, out),
                _ => {}
            }
        }
    }

    fn write_myst(doc: &Document) -> String {
        let restore = RestoreMap::new();
        MystWriter::new(&restore, Vec::new()).write(doc).0
    }

    #[test]
    fn nested_trees_survive_write_then_parse() {
        let sources = [
            "::::{note}\n:::{warning} Inner\nDeep.\n:::\n::::\n",
            "::::{grid} 2\n:::{card} A\n```python\nx = 1\n```\n:::\n:::{card} B\n:::{tip}\nNested tip.\n:::\n:::\n::::\n",
            ":::::{tab-set}\n::::{tab-item} One\n:::{note}\n```{code-cell} python\nx\n```\n:::\n::::\n::::{tab-item} Two\nPlain.\n::::\n:::::\n",
            "::::::{dropdown} Outer\n:::::{grid} 1\n::::{card} C\n:::{epigraph}\nQuote.\n:::\n::::\n:::::\n::::::\n",
        ];
        for source in sources {
            let reader = || crate::MystReader::new(ReaderContext::new("a.md"));
            let first = reader().read_str(source).unwrap();
            let written = write_myst(&first);
            let second = reader().read_str(&written).unwrap();
            let (mut a, mut b) = (Vec::new(), Vec::new());
            shape(&first.blocks, &mut a);
            shape(&second.blocks, &mut b);
            assert_eq!(a, b, "structure changed:\n{source}\n---\n{written}");
            assert_eq!(
                write_myst(&second),
                written,
                "writer is not stable on:\n{written}"
            );
        }
    }

    #[test]
    fn nested_trees_survive_a_quarto_round_trip() {
        let source = "::::{grid} 2\n:::{card} A\n::::{note} Title\nDeep.\n::::\n:::\n:::{card} B\nPlain.\n:::\n::::\n";
        let first = crate::MystReader::new(ReaderContext::new("a.md"))
            .read_str(source)
            .unwrap();
        let docs = vec![(std::path::PathBuf::from("a.md"), first.clone())];
        let registry = crate::LabelRegistry::build(&docs).0;
        let (qmd, _, _) = crate::writer::quarto::QuartoWriter::new(&registry).write(&first);
        let back = crate::QuartoReader::new(ReaderContext::new("a.qmd"))
            .read_str(&qmd)
            .unwrap();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        shape(&first.blocks, &mut a);
        shape(&back.blocks, &mut b);
        assert_eq!(a, b, "structure changed through Quarto:\n{qmd}");
    }
}
