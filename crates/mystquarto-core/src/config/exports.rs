//! Reference §8.3: MyST `project.exports[]` (a list of `{format|template,
//! ...}`) <-> Quarto `format` (a map). Fixes D6: an export entry carrying
//! only a `template:` (no `format:`) previously produced `format: {}`,
//! invalid Quarto — the template name is inspected and mapped to a real
//! format, with the (non-portable) template name preserved as a comment.

use super::{as_mapping, as_str, get, warn, Diagnostic, Severity};
use crate::diagnostics::codes::config as codes;
use crate::yaml::YamlValue;

/// The Quarto `format:` field this phase would emit, plus any comment that
/// should render above it (documenting a guessed/non-portable format — see
/// [`crate::yaml::emit`]'s "one comment above a given top-level key" model,
/// which is why every export's note is folded into a single multi-line
/// comment on the one `format` field rather than attached per-entry).
pub struct FormatField {
    pub value: YamlValue,
    pub comment: Option<String>,
}

/// Converts every entry of `exports` (MyST's `project.exports[]`, already
/// narrowed to a `&[YamlValue]`) into Quarto's `format:` map. Returns `None`
/// when no entry produced a usable format (an empty `exports: []`, or every
/// entry being an unmappable `format: meca`) — callers must not emit
/// `format: {}` in that case, only skip the field.
///
/// Whenever at least one entry is usable, `html` is always emitted first
/// with a sensible default option block ([`html_defaults`]) — Quarto's
/// `format:` map renders its **first** key by default, and `typst`/`docx`/
/// `pdf` outputs are export alternatives, never the default render target.
/// Every other resolved format is appended after it, in export order.
#[must_use]
pub fn exports_to_format(exports: &[YamlValue]) -> (Option<FormatField>, Vec<Diagnostic>) {
    let mut entries: Vec<(String, YamlValue)> = Vec::new();
    let mut comment_lines: Vec<String> = Vec::new();
    let mut warnings = Vec::new();

    for export in exports {
        let Some(m) = as_mapping(export) else {
            continue;
        };
        if let Some(fmt) = get(m, "format").and_then(as_str) {
            match known_format(fmt) {
                Some(quarto_key) => entries.push((
                    quarto_key.to_string(),
                    with_export_options(default_format_options(quarto_key), m),
                )),
                None if fmt == "meca" => warnings.push(warn(
                    Severity::Warning,
                    codes::EXPORT_FORMAT_DROPPED,
                    "myst.yml project.exports: format `meca` has no Quarto equivalent \
                     (reference §8.3); dropped"
                        .to_string(),
                )),
                None => warnings.push(warn(
                    Severity::Warning,
                    codes::EXPORT_FORMAT_DROPPED,
                    format!(
                        "myst.yml project.exports: unrecognized format `{fmt}`; dropped (no \
                         known Quarto equivalent)"
                    ),
                )),
            }
        } else if let Some(template) = get(m, "template").and_then(as_str) {
            let (quarto_fmt, guessed) = infer_format_from_template(template);
            entries.push((quarto_fmt.to_string(), default_format_options(quarto_fmt)));
            comment_lines.push(if guessed {
                format!(
                    "mystquarto: could not infer a format from template `{template}`; \
                     guessed `{quarto_fmt}` — verify and override if wrong"
                )
            } else {
                format!(
                    "mystquarto: `template: {template}` is not portable to Quarto; inferred \
                     format `{quarto_fmt}`"
                )
            });
            if guessed {
                warnings.push(warn(
                    Severity::Warning,
                    codes::EXPORT_FORMAT_GUESSED,
                    format!(
                        "myst.yml project.exports: template `{template}`'s suffix is not \
                         recognized; guessed format `{quarto_fmt}`"
                    ),
                ));
            }
        }
    }

    if entries.is_empty() {
        return (None, warnings);
    }
    let mut all_entries = vec![("html".to_string(), html_defaults())];
    all_entries.extend(entries);
    let comment = (!comment_lines.is_empty()).then(|| comment_lines.join("\n"));
    (
        Some(FormatField {
            value: YamlValue::Mapping(all_entries),
            comment,
        }),
        warnings,
    )
}

/// Default `html:` option block emitted whenever `format:` is produced —
/// `html` is always the default render target, never `typst`/`docx`/`pdf`.
fn html_defaults() -> YamlValue {
    YamlValue::Mapping(vec![
        (
            "theme".to_string(),
            YamlValue::Sequence(vec![YamlValue::String("default".to_string())]),
        ),
        ("toc".to_string(), YamlValue::Bool(true)),
        (
            "comments".to_string(),
            YamlValue::Mapping(vec![("hypothesis".to_string(), YamlValue::Bool(true))]),
        ),
        ("number-sections".to_string(), YamlValue::Bool(true)),
        ("citations-hover".to_string(), YamlValue::Bool(true)),
        ("crossrefs-hover".to_string(), YamlValue::Bool(true)),
        (
            "toc-location".to_string(),
            YamlValue::String("right".to_string()),
        ),
        (
            "title-block-style".to_string(),
            YamlValue::String("manuscript".to_string()),
        ),
    ])
}

/// Per-format default option block for a non-`html` alternative format.
/// `typst` needs `number-sections: true` unconditionally: Quarto's Typst
/// writer compiles `@sec-xxx` cross-references to `#ref(<sec-xxx>, ...)`,
/// and Typst refuses to reference a heading that isn't numbered — without
/// this, any document with a section cross-reference fails to render to
/// Typst (`cannot reference heading without numbering`).
fn default_format_options(quarto_key: &str) -> YamlValue {
    match quarto_key {
        "typst" => YamlValue::Mapping(vec![("number-sections".to_string(), YamlValue::Bool(true))]),
        _ => YamlValue::Mapping(vec![]),
    }
}

/// Carries the per-export options that have a direct Quarto format option:
/// `output` -> `output-file`, and `template` -> `template` when it is a file
/// path (a MyST template *name* such as `lapreprint-typst` is not portable).
fn with_export_options(defaults: YamlValue, export: &[(String, YamlValue)]) -> YamlValue {
    let YamlValue::Mapping(mut options) = defaults else {
        return defaults;
    };
    if let Some(output) = get(export, "output").and_then(as_str) {
        options.push((
            "output-file".to_string(),
            YamlValue::String(output.to_string()),
        ));
    }
    if let Some(template) = get(export, "template").and_then(as_str) {
        if is_template_path(template) {
            options.push((
                "template".to_string(),
                YamlValue::String(template.to_string()),
            ));
        }
    }
    YamlValue::Mapping(options)
}

/// A template value that names a local file rather than a MyST template
/// registry entry.
pub(crate) fn is_template_path(template: &str) -> bool {
    template.contains('/')
        || [".typ", ".tex", ".docx"]
            .iter()
            .any(|ext| template.ends_with(ext))
}

fn known_format(fmt: &str) -> Option<&'static str> {
    match fmt {
        "pdf" => Some("pdf"),
        "docx" => Some("docx"),
        "tex" => Some("latex"),
        "jats" => Some("jats"),
        "typst" => Some("typst"),
        _ => None,
    }
}

/// Template-suffix -> inferred format (reference §8.3's table). Unrecognized
/// suffixes fall back to `pdf` with `guessed = true`, so callers always warn
/// rather than silently guessing.
fn infer_format_from_template(template: &str) -> (&'static str, bool) {
    if template.ends_with("-typst") {
        ("typst", false)
    } else if template.ends_with("-tex") || template.ends_with("-latex") {
        ("pdf", false)
    } else if template.ends_with("-docx") {
        ("docx", false)
    } else if template.ends_with("-jats") {
        ("jats", false)
    } else {
        ("pdf", true)
    }
}

/// Reference architecture note: "`manuscript.article` derives from
/// `exports[].article`." Returns the first `article:` value found across
/// `exports`, if any.
#[must_use]
pub fn manuscript_article(exports: &[YamlValue]) -> Option<String> {
    exports
        .iter()
        .filter_map(as_mapping)
        .find_map(|m| get(m, "article").and_then(as_str))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml::parse_mapping;

    fn exports_from(text: &str) -> Vec<YamlValue> {
        let parsed = parse_mapping(text).expect("valid YAML");
        match &parsed[0].1 {
            YamlValue::Sequence(s) => s.clone(),
            _ => panic!("expected a sequence"),
        }
    }

    #[test]
    fn explicit_format_maps_directly_after_the_default_html_entry() {
        let exports = exports_from("exports:\n  - format: pdf\n");
        let (field, warnings) = exports_to_format(&exports);
        assert!(warnings.is_empty());
        let YamlValue::Mapping(m) = field.unwrap().value else {
            panic!()
        };
        assert_eq!(m[0].0, "html");
        assert_eq!(m[1], ("pdf".to_string(), YamlValue::Mapping(vec![])));
    }

    #[test]
    fn typst_export_maps_to_quarto_typst_with_output_and_template_path() {
        let exports = exports_from(
            "exports:\n  - format: typst\n    template: templates/paper.typ\n    output: paper.pdf\n",
        );
        let (field, warnings) = exports_to_format(&exports);
        assert!(warnings.is_empty(), "{warnings:?}");
        let YamlValue::Mapping(m) = field.unwrap().value else {
            panic!()
        };
        let (key, YamlValue::Mapping(opts)) = &m[1] else {
            panic!()
        };
        assert_eq!(key, "typst");
        assert!(opts.contains(&(
            "output-file".to_string(),
            YamlValue::String("paper.pdf".into())
        )));
        assert!(opts.contains(&(
            "template".to_string(),
            YamlValue::String("templates/paper.typ".into())
        )));
    }

    #[test]
    fn a_template_name_is_not_carried_as_a_quarto_template_path() {
        let exports = exports_from("exports:\n  - format: typst\n    template: lapreprint-typst\n");
        let (field, _) = exports_to_format(&exports);
        let YamlValue::Mapping(m) = field.unwrap().value else {
            panic!()
        };
        let YamlValue::Mapping(opts) = &m[1].1 else {
            panic!()
        };
        assert!(opts.iter().all(|(k, _)| k != "template"));
    }

    #[test]
    fn tex_maps_to_latex_after_the_default_html_entry() {
        let exports = exports_from("exports:\n  - format: tex\n");
        let (field, _) = exports_to_format(&exports);
        let YamlValue::Mapping(m) = field.unwrap().value else {
            panic!()
        };
        assert_eq!(m[0].0, "html");
        assert_eq!(m[1].0, "latex");
    }

    #[test]
    fn html_is_always_the_first_default_format() {
        let exports = exports_from("exports:\n  - template: lapreprint-typst\n");
        let (field, _) = exports_to_format(&exports);
        let YamlValue::Mapping(m) = field.unwrap().value else {
            panic!()
        };
        assert_eq!(m[0].0, "html");
        let YamlValue::Mapping(html_opts) = &m[0].1 else {
            panic!("html entry must carry its default option block")
        };
        assert!(html_opts
            .iter()
            .any(|(k, v)| k == "number-sections" && v == &YamlValue::Bool(true)));
    }

    #[test]
    fn typst_always_carries_number_sections_so_crossrefs_render() {
        let exports = exports_from("exports:\n  - template: lapreprint-typst\n");
        let (field, _) = exports_to_format(&exports);
        let YamlValue::Mapping(m) = field.unwrap().value else {
            panic!()
        };
        let (_, typst_value) = m.iter().find(|(k, _)| k == "typst").expect("typst entry");
        assert_eq!(
            typst_value,
            &YamlValue::Mapping(vec![("number-sections".to_string(), YamlValue::Bool(true))])
        );
    }

    #[test]
    fn meca_is_dropped_with_a_warning_never_format_empty() {
        let exports = exports_from("exports:\n  - format: meca\n");
        let (field, warnings) = exports_to_format(&exports);
        assert!(
            field.is_none(),
            "an all-unmappable export list must not emit format: {{}}"
        );
        assert_eq!(warnings.len(), 1);
    }

    /// D6: `template:`-only export must infer a real format, never
    /// `format: {}`, and must preserve the template name as a comment.
    #[test]
    fn template_only_export_infers_typst_and_preserves_template_as_comment() {
        let exports =
            exports_from("exports:\n  - template: lapreprint-typst\n    article: article.md\n");
        let (field, warnings) = exports_to_format(&exports);
        assert!(warnings.is_empty(), "a recognized suffix should not warn");
        let field = field.expect("template-only export must still produce a format");
        let YamlValue::Mapping(m) = &field.value else {
            panic!()
        };
        assert_eq!(m[0].0, "html");
        assert_eq!(
            m[1],
            (
                "typst".to_string(),
                YamlValue::Mapping(vec![("number-sections".to_string(), YamlValue::Bool(true))])
            )
        );
        assert!(field.comment.unwrap().contains("lapreprint-typst"));
    }

    #[test]
    fn unrecognized_template_suffix_guesses_pdf_and_warns() {
        let exports = exports_from("exports:\n  - template: my-custom-house-style\n");
        let (field, warnings) = exports_to_format(&exports);
        let YamlValue::Mapping(m) = &field.unwrap().value else {
            panic!()
        };
        assert_eq!(m[0].0, "html");
        assert_eq!(m[1].0, "pdf");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn empty_exports_produces_no_format_field() {
        let (field, warnings) = exports_to_format(&[]);
        assert!(field.is_none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn manuscript_article_reads_the_first_article_field() {
        let exports =
            exports_from("exports:\n  - template: lapreprint-typst\n    article: article.md\n");
        assert_eq!(manuscript_article(&exports), Some("article.md".to_string()));
    }
}
