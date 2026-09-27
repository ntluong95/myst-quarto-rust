//! Exact config round trips through a recorded snapshot.
//!
//! The two config dialects do not map one to one: `_quarto.yml` has
//! `execute`, `project.render`, `manuscript.code-links` and per-format
//! options that `myst.yml` cannot hold, and `myst.yml` has fields Quarto
//! lacks. Rather than keep a per-key preserve list (which always misses the
//! next key), a conversion records the source config's exact text and the
//! target config it wrote ([`crate::preserve::ConfigSnapshot`]). Converting
//! back then:
//!
//! - restores the original text byte for byte when the derived config is
//!   semantically unchanged, comments and key order included, and
//! - otherwise applies only the user's edits: a three-way merge of the
//!   original (`source`), the conversion of the recorded derived config
//!   (`base`), and the conversion of the edited config (`fresh`). A key
//!   whose conversion did not change keeps its original value, and a key
//!   the edit changed takes the fresh value.

use crate::preserve::ConfigSnapshot;
use crate::yaml::emit::{emit, EmitField, YamlDoc};
use crate::yaml::{parse_mapping, YamlReadError, YamlValue};

/// How [`restore`] produced the target config.
#[derive(Debug, Clone, PartialEq)]
pub enum Restored {
    /// The derived config was unchanged; this is the original source text.
    Verbatim(String),
    /// The derived config was edited; this merges the edits into the
    /// original.
    Merged(String),
}

impl Restored {
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Restored::Verbatim(t) | Restored::Merged(t) => t,
        }
    }
}

/// Restores the original config from `snapshot` given the config being
/// converted back (`current_text`). `fresh_text` is `current_text`
/// converted normally, and `convert` is the same conversion, applied here to
/// the recorded derived text to get the merge base.
///
/// # Errors
/// Propagates a YAML error from any of the three texts.
pub fn restore(
    snapshot: &ConfigSnapshot,
    current_text: &str,
    fresh_text: &str,
    convert: impl Fn(&str) -> Result<String, YamlReadError>,
) -> Result<Restored, YamlReadError> {
    if same_meaning(current_text, &snapshot.derived)? {
        return Ok(Restored::Verbatim(snapshot.source.clone()));
    }
    let source = parse_mapping(&snapshot.source)?;
    let base = parse_mapping(&convert(&snapshot.derived)?)?;
    let fresh = parse_mapping(fresh_text)?;
    let merged = merge_mappings(&source, &base, &fresh);
    Ok(Restored::Merged(emit(&YamlDoc(
        merged
            .into_iter()
            .map(|(k, v)| EmitField::new(k, v))
            .collect(),
    ))))
}

/// `true` when two YAML documents hold the same data, ignoring comments,
/// formatting and mapping key order.
///
/// # Errors
/// Propagates a YAML error from either text.
pub fn same_meaning(a: &str, b: &str) -> Result<bool, YamlReadError> {
    Ok(canonical(&YamlValue::Mapping(parse_mapping(a)?))
        == canonical(&YamlValue::Mapping(parse_mapping(b)?)))
}

/// `value` with mapping keys sorted recursively and block-literal strings
/// folded into plain strings, so equal data compares equal.
#[must_use]
pub fn canonical(value: &YamlValue) -> YamlValue {
    match value {
        YamlValue::Mapping(pairs) => {
            let mut pairs: Vec<(String, YamlValue)> = pairs
                .iter()
                .map(|(k, v)| (k.clone(), canonical(v)))
                .collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            YamlValue::Mapping(pairs)
        }
        YamlValue::Sequence(items) => YamlValue::Sequence(items.iter().map(canonical).collect()),
        YamlValue::BlockLiteral(s) => YamlValue::String(s.clone()),
        other => other.clone(),
    }
}

fn get<'a>(m: &'a [(String, YamlValue)], key: &str) -> Option<&'a YamlValue> {
    m.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn same(a: Option<&YamlValue>, b: Option<&YamlValue>) -> bool {
    a.map(canonical) == b.map(canonical)
}

/// Merges key by key in the source's key order, then appends keys only the
/// edit introduced.
fn merge_mappings(
    source: &[(String, YamlValue)],
    base: &[(String, YamlValue)],
    fresh: &[(String, YamlValue)],
) -> Vec<(String, YamlValue)> {
    let mut out = Vec::new();
    let keys = source.iter().map(|(k, _)| k).chain(
        fresh
            .iter()
            .map(|(k, _)| k)
            .filter(|k| get(source, k).is_none()),
    );
    for key in keys {
        if let Some(v) = merge_value(get(source, key), get(base, key), get(fresh, key)) {
            out.push((key.clone(), v));
        }
    }
    out
}

fn merge_value(
    source: Option<&YamlValue>,
    base: Option<&YamlValue>,
    fresh: Option<&YamlValue>,
) -> Option<YamlValue> {
    if same(base, fresh) {
        return source.cloned();
    }
    match (source, base, fresh) {
        (Some(YamlValue::Mapping(s)), Some(YamlValue::Mapping(b)), Some(YamlValue::Mapping(f))) => {
            Some(YamlValue::Mapping(merge_mappings(s, b, f)))
        }
        _ => fresh.cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in conversion: renames `name` to `title` and drops `extra`.
    fn convert(text: &str) -> Result<String, YamlReadError> {
        let m = parse_mapping(text)?;
        let fields = m
            .into_iter()
            .filter(|(k, _)| k != "extra")
            .map(|(k, v)| EmitField::new(if k == "name" { "title".into() } else { k }, v))
            .collect();
        Ok(emit(&YamlDoc(fields)))
    }

    fn snapshot() -> ConfigSnapshot {
        ConfigSnapshot {
            name: "source.yml".into(),
            source: "# kept comment\ntitle: A\nextra:\n  x: 1\nopts:\n  a: 1\n  b: 2\n".into(),
            derived: "name: A\nopts:\n  a: 1\n  b: 2\n".into(),
        }
    }

    #[test]
    fn an_unchanged_derived_config_restores_the_original_text_exactly() {
        let current = "opts: {b: 2, a: 1}\nname: A\n";
        let restored = restore(&snapshot(), current, &convert(current).unwrap(), convert).unwrap();
        assert_eq!(restored, Restored::Verbatim(snapshot().source));
    }

    #[test]
    fn an_edit_is_applied_and_everything_else_is_restored() {
        let current = "name: B\nopts:\n  a: 1\n  b: 3\n  c: 4\n";
        let restored = restore(&snapshot(), current, &convert(current).unwrap(), convert).unwrap();
        let Restored::Merged(text) = restored else {
            panic!("expected a merge")
        };
        let merged = canonical(&YamlValue::Mapping(parse_mapping(&text).unwrap()));
        let want = canonical(&YamlValue::Mapping(
            parse_mapping("title: B\nextra:\n  x: 1\nopts:\n  a: 1\n  b: 3\n  c: 4\n").unwrap(),
        ));
        assert_eq!(merged, want);
    }

    #[test]
    fn a_key_removed_by_the_edit_is_removed() {
        let current = "name: A\n";
        let restored = restore(&snapshot(), current, &convert(current).unwrap(), convert).unwrap();
        let text = restored.text().to_string();
        assert!(!text.contains("opts"), "{text}");
        assert!(text.contains("extra"), "{text}");
    }
}
