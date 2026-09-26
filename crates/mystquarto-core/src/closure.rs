//! Which files a conversion reads: the manuscript's *closure*.
//!
//! A project directory usually holds far more than the manuscript: data,
//! analysis notebooks, agent instructions, plans, original literature. A
//! conversion must never read or copy those. When the input has a project
//! config, the file set is everything reachable from that config (content
//! roots, then each content file's includes, images, links, embeds and
//! bibliographies, recursively) — not a directory walk. Without a config (a
//! bare folder of documents) it is a directory walk, minus agent/tooling
//! files.
//!
//! Two filters apply in every mode:
//!
//! - a hard **denylist** ([`is_denied`]) of governed locations — the raw
//!   data tier, original literature, `.ask/`, `.git/`, `.env*`. A reference
//!   into it is an [`Severity::Error`]: a manuscript must only reference
//!   promoted artifacts, so such a reference is a genuine bug, and the file
//!   is never read;
//! - `.gitignore` rules (via the `ignore` crate). A gitignored file the
//!   closure references is skipped with a [`Severity::Warning`].

use std::collections::{BTreeSet, VecDeque};
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSetBuilder};

use crate::diagnostics::codes::io as codes;
use crate::diagnostics::{Diagnostic, Severity};
use crate::fs::assets::ASSET_SKIP_DIRS;
use crate::fs::path_guard::{effective_output_excluded_from_walk, is_descendant};
use crate::yaml::{parse_mapping, YamlValue};

/// Governed locations no conversion may read, relative to the project root.
/// Directory prefixes end in `/`.
const DENIED_PREFIXES: &[&str] = &["data/raw/", "literature/og/"];
/// Path components that deny everything beneath them, at any depth.
const DENIED_COMPONENTS: &[&str] = &[".ask", ".git"];

/// Both dialects' project configs.
const CONFIG_NAMES: &[&str] = &["myst.yml", "_quarto.yml"];

/// Agent and tooling files that are never manuscript content, excluded by
/// name from a directory walk.
const AGENT_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "README.md", "CHANGELOG.md"];
/// Directories of agent/tooling material excluded from a directory walk.
const AGENT_DIRS: &[&str] = &["plans"];

/// `true` when `rel` (relative to the project root) is in a governed
/// location no conversion may read.
#[must_use]
pub fn is_denied(rel: &Path) -> bool {
    let text = rel.to_string_lossy().replace('\\', "/");
    DENIED_PREFIXES
        .iter()
        .any(|p| text.starts_with(p) || text == p.trim_end_matches('/'))
        || rel.components().any(|c| match c {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                DENIED_COMPONENTS.contains(&name.as_ref()) || name.starts_with(".env")
            }
            _ => false,
        })
}

/// Which dialect a project config is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigDialect {
    Myst,
    Quarto,
}

/// What to resolve.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// The canonical project root.
    pub root: &'a Path,
    /// The output root, excluded when it sits inside the input.
    pub output_root: Option<&'a Path>,
    /// Extension of the dialect being converted from (`md` / `qmd`).
    pub source_ext: &'a str,
    /// Where the closure starts.
    pub start: Start<'a>,
}

#[derive(Debug, Clone)]
pub enum Start<'a> {
    /// The project config at the root, in this dialect.
    Config {
        path: &'a Path,
        dialect: ConfigDialect,
    },
    /// Explicit files (single-file mode).
    Files(&'a [PathBuf]),
    /// A directory walk. `all` keeps agent/tooling files too.
    Walk { all: bool },
}

/// The resolved file set, as absolute paths under the root, each list
/// sorted.
#[derive(Debug, Default, Clone)]
pub struct FileSet {
    /// Files to convert (the source dialect's extension).
    pub content: Vec<PathBuf>,
    /// Notebooks (`.ipynb`) — copied, and indexed for embeds.
    pub notebooks: Vec<PathBuf>,
    /// Everything else to copy verbatim.
    pub assets: Vec<PathBuf>,
    /// Symlinks found and never followed.
    pub skipped_symlinks: Vec<PathBuf>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Resolves `request` into the files a conversion may read.
#[must_use]
pub fn resolve(request: &Request<'_>) -> FileSet {
    let visible = visible_files(request.root, request.output_root);
    let mut resolver = Resolver {
        request,
        visible: &visible,
        seen: BTreeSet::new(),
        queue: VecDeque::new(),
        set: FileSet::default(),
    };
    match &request.start {
        Start::Walk { all } => resolver.walk(*all),
        Start::Files(files) => {
            for f in *files {
                resolver.add(f.clone(), None);
            }
        }
        Start::Config { path, dialect } => {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            let roots = match parse_mapping(&text) {
                Ok(config) => config_roots(&config, *dialect),
                Err(_) => ConfigRoots::default(),
            };
            // No render list, toc or article (or a MyST project without a
            // toc): the renderer builds every document in the folder.
            if roots.walk || roots.content.is_empty() {
                resolver.walk(false);
            }
            resolver.add_patterns(&roots.content, Some(path));
            resolver.add_patterns(&roots.assets, Some(path));
        }
    }
    resolver.drain();
    let mut set = resolver.set;
    for list in [
        &mut set.content,
        &mut set.notebooks,
        &mut set.assets,
        &mut set.skipped_symlinks,
    ] {
        list.sort();
        list.dedup();
    }
    set
}

struct Resolver<'a> {
    request: &'a Request<'a>,
    visible: &'a BTreeSet<PathBuf>,
    seen: BTreeSet<PathBuf>,
    queue: VecDeque<PathBuf>,
    set: FileSet,
}

impl Resolver<'_> {
    fn root(&self) -> &Path {
        self.request.root
    }

    fn walk(&mut self, all: bool) {
        let root = self.root().to_path_buf();
        let files: Vec<PathBuf> = self
            .visible
            .iter()
            .filter(|p| {
                let rel = p.strip_prefix(&root).unwrap_or(p);
                all || !is_agent_file(rel)
            })
            .cloned()
            .collect();
        for f in files {
            self.add(f, None);
        }
    }

    /// Adds every file a root-relative pattern names: a plain path, a
    /// directory (all visible files under it), or a glob. A leading `!`
    /// excludes (Quarto `project.render`).
    fn add_patterns(&mut self, patterns: &[String], from: Option<&Path>) {
        let root = self.root().to_path_buf();
        let mut include = GlobSetBuilder::new();
        let mut exclude = GlobSetBuilder::new();
        let mut any_glob = false;
        for pattern in patterns {
            let (negated, pattern) = match pattern.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, pattern.as_str()),
            };
            let pattern = pattern.trim_start_matches("./").trim_start_matches('/');
            if pattern.contains(['*', '?', '[']) {
                let glob = GlobBuilder::new(pattern).literal_separator(true).build();
                if let Ok(glob) = glob {
                    any_glob = true;
                    if negated {
                        exclude.add(glob);
                    } else {
                        include.add(glob);
                    }
                }
            } else if !negated {
                self.add(root.join(pattern), from);
            }
        }
        if !any_glob {
            return;
        }
        let (Ok(include), Ok(exclude)) = (include.build(), exclude.build()) else {
            return;
        };
        let matched: Vec<PathBuf> = self
            .visible
            .iter()
            .filter(|p| {
                let rel = p.strip_prefix(&root).unwrap_or(p);
                include.is_match(rel) && !exclude.is_match(rel)
            })
            .cloned()
            .collect();
        for f in matched {
            self.add(f, from);
        }
    }

    /// Queues `path` (referenced from `from`, if known), applying the
    /// denylist and the `.gitignore` filter.
    fn add(&mut self, path: PathBuf, from: Option<&Path>) {
        let root = self.root().to_path_buf();
        let path = normalize(&path);
        if !is_descendant(&root, &path) || path == root {
            return;
        }
        if !self.seen.insert(path.clone()) {
            return;
        }
        let rel = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
        if is_denied(&rel) {
            let mut d = Diagnostic::new(
                Severity::Error,
                codes::DENIED_REFERENCE,
                format!(
                    "`{}` is in a governed location (raw data, original literature, .ask, \
                     .git or .env) and is never read or copied; reference a promoted artifact \
                     instead",
                    rel.display()
                ),
            );
            if let Some(f) = from {
                d = d.with_file(f.to_path_buf());
            }
            self.set.diagnostics.push(d);
            return;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            // A toc may name a document without its extension (MyST allows
            // `file: intro`); otherwise a dangling reference is the
            // renderer's to report.
            if path.extension().is_none() {
                for ext in [self.request.source_ext, "ipynb"] {
                    self.add(path.with_extension(ext), from);
                }
            }
            return;
        };
        if meta.file_type().is_symlink() {
            self.set.skipped_symlinks.push(path);
            return;
        }
        if meta.is_dir() {
            let under: Vec<PathBuf> = self
                .visible
                .iter()
                .filter(|p| p.starts_with(&path))
                .cloned()
                .collect();
            for f in under {
                self.add(f, from);
            }
            return;
        }
        if !self.visible.contains(&path) {
            let mut d = Diagnostic::new(
                Severity::Warning,
                codes::CLOSURE_FILE_IGNORED,
                format!(
                    "`{}` is referenced but gitignored (or in a build/cache folder); not copied",
                    rel.display()
                ),
            );
            if let Some(f) = from {
                d = d.with_file(f.to_path_buf());
            }
            self.set.diagnostics.push(d);
            return;
        }
        self.queue.push_back(path);
    }

    fn drain(&mut self) {
        while let Some(path) = self.queue.pop_front() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == self.request.source_ext {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let dir = path.parent().unwrap_or(self.root()).to_path_buf();
                    for reference in referenced_paths(&text) {
                        let target = if let Some(abs) = reference.strip_prefix('/') {
                            self.root().join(abs)
                        } else {
                            dir.join(&reference)
                        };
                        self.add(target, Some(&path));
                    }
                }
                self.set.content.push(path);
            } else if ext == "ipynb" {
                self.set.notebooks.push(path);
            } else if path
                .file_name()
                .is_some_and(|n| CONFIG_NAMES.contains(&n.to_string_lossy().as_ref()))
            {
                // Configs are converted (or deliberately ignored) by the
                // caller, never copied: a copied stale config would
                // overwrite the converted one.
            } else {
                self.set.assets.push(path);
            }
        }
    }
}

fn is_agent_file(rel: &Path) -> bool {
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    AGENT_FILES.contains(&name)
        || rel
            .components()
            .next()
            .is_some_and(|c| AGENT_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
}

/// `true` when `path` (a directory under `root`) is gitignored by a
/// `.gitignore` in `root` or in any directory between `root` and `path`.
#[must_use]
pub fn is_gitignored_dir(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let mut dir = root.to_path_buf();
    let mut ancestors = vec![dir.clone()];
    for component in rel.components() {
        dir.push(component);
        ancestors.push(dir.clone());
    }
    ancestors.pop(); // `path` itself: its own .gitignore cannot ignore it
    ancestors.iter().any(|base| {
        let mut builder = ignore::gitignore::GitignoreBuilder::new(base);
        if builder.add(base.join(".gitignore")).is_some() {
            return false; // no readable .gitignore here
        }
        builder.build().is_ok_and(|gi| {
            path.strip_prefix(base)
                .is_ok_and(|r| gi.matched_path_or_any_parents(r, true).is_ignore())
        })
    })
}

/// Every non-ignored file under `root`, honouring `.gitignore` files (with
/// or without a git repository), skipping build/cache folders, the denylist
/// and the output root, and never following symlinked directories.
/// Symlinked *files* are included so a reference to one can be reported.
fn visible_files(root: &Path, output_root: Option<&Path>) -> BTreeSet<PathBuf> {
    let root_owned = root.to_path_buf();
    let output = output_root.map(Path::to_path_buf);
    let mut walker = ignore::WalkBuilder::new(root);
    walker
        .hidden(false)
        .git_ignore(true)
        .git_exclude(false)
        .git_global(false)
        .ignore(false)
        .parents(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let path = entry.path();
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            if is_dir {
                let name = entry.file_name().to_string_lossy();
                if ASSET_SKIP_DIRS.contains(&name.as_ref()) {
                    return false;
                }
                if let Some(out) = &output {
                    if effective_output_excluded_from_walk(out, path) {
                        return false;
                    }
                }
            }
            let rel = path.strip_prefix(&root_owned).unwrap_or(path);
            !is_denied(rel)
        });
    walker
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| !t.is_dir()))
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Resolves `.` and `..` lexically, without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[derive(Debug, Default)]
struct ConfigRoots {
    /// Every document in the folder is a page too (MyST without a `toc`).
    walk: bool,
    /// Root-relative content paths or globs.
    content: Vec<String>,
    /// Root-relative asset paths or globs.
    assets: Vec<String>,
}

fn get<'a>(m: &'a [(String, YamlValue)], key: &str) -> Option<&'a YamlValue> {
    m.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn mapping<'a>(m: &'a [(String, YamlValue)], key: &str) -> &'a [(String, YamlValue)] {
    match get(m, key) {
        Some(YamlValue::Mapping(inner)) => inner,
        _ => &[],
    }
}

/// A string, or every string in a list.
fn strings(v: Option<&YamlValue>) -> Vec<String> {
    match v {
        Some(YamlValue::String(s)) => vec![s.clone()],
        Some(YamlValue::Sequence(items)) => items
            .iter()
            .filter_map(|i| match i {
                YamlValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// File-valued keys a format or page can carry.
const FILE_KEYS: &[&str] = &[
    "bibliography",
    "csl",
    "css",
    "template",
    "reference-doc",
    "include-in-header",
    "include-before-body",
    "include-after-body",
    "logo",
    "image",
    "cover-image",
    "banner",
    "thumbnail",
];

fn file_values(m: &[(String, YamlValue)]) -> Vec<String> {
    FILE_KEYS.iter().flat_map(|k| strings(get(m, k))).collect()
}

fn config_roots(config: &[(String, YamlValue)], dialect: ConfigDialect) -> ConfigRoots {
    let mut roots = ConfigRoots::default();
    match dialect {
        ConfigDialect::Quarto => {
            let project = mapping(config, "project");
            let manuscript = mapping(config, "manuscript");
            let book = mapping(config, "book");
            roots.content.extend(strings(get(project, "render")));
            if let Some(article) = strings(get(manuscript, "article")).into_iter().next() {
                roots.content.push(article);
            } else if matches!(get(project, "type"), Some(YamlValue::String(t)) if t == "manuscript")
            {
                roots.content.push("index.qmd".to_string());
            }
            if let Some(YamlValue::Sequence(nbs)) = get(manuscript, "notebooks") {
                for nb in nbs {
                    match nb {
                        YamlValue::String(s) => roots.content.push(s.clone()),
                        YamlValue::Mapping(m) => roots.content.extend(strings(get(m, "notebook"))),
                        _ => {}
                    }
                }
            }
            if !book.is_empty() {
                roots.content.push("index.qmd".to_string());
                for key in ["chapters", "appendices"] {
                    if let Some(YamlValue::Sequence(items)) = get(book, key) {
                        quarto_chapters(items, &mut roots.content);
                    }
                }
                roots.assets.extend(file_values(book));
            }
            // Quarto extensions a format may name; kept so the project
            // still renders after a round trip.
            roots.assets.push("_extensions".to_string());
            roots.assets.extend(strings(get(manuscript, "resources")));
            roots.assets.extend(strings(get(project, "resources")));
            roots.assets.extend(file_values(config));
            for (_, options) in mapping(config, "format") {
                if let YamlValue::Mapping(m) = options {
                    roots.assets.extend(file_values(m));
                }
            }
        }
        ConfigDialect::Myst => {
            let project = mapping(config, "project");
            match get(project, "toc") {
                Some(YamlValue::Sequence(toc)) => myst_toc(toc, &mut roots.content),
                _ => roots.walk = true,
            }
            if get(project, "bibliography").is_none() {
                // MyST loads every `.bib` at the root when none is named.
                roots.assets.push("*.bib".to_string());
            }
            roots.assets.push("_extensions".to_string());
            if let Some(YamlValue::Sequence(exports)) = get(project, "exports") {
                for export in exports {
                    if let YamlValue::Mapping(m) = export {
                        roots.content.extend(strings(get(m, "article")));
                        roots.assets.extend(
                            strings(get(m, "template"))
                                .into_iter()
                                .filter(|t| crate::config::exports::is_template_path(t)),
                        );
                    }
                }
            }
            roots.assets.extend(strings(get(project, "resources")));
            roots.assets.extend(file_values(project));
            roots
                .assets
                .extend(file_values(mapping(mapping(config, "site"), "options")));
        }
    }
    roots
}

fn quarto_chapters(items: &[YamlValue], out: &mut Vec<String>) {
    for item in items {
        match item {
            YamlValue::String(s) => out.push(s.clone()),
            YamlValue::Mapping(m) => {
                if let Some(YamlValue::String(part)) = get(m, "part") {
                    if part.ends_with(".qmd") || part.ends_with(".md") {
                        out.push(part.clone());
                    }
                }
                if let Some(YamlValue::Sequence(chapters)) = get(m, "chapters") {
                    quarto_chapters(chapters, out);
                }
            }
            _ => {}
        }
    }
}

fn myst_toc(entries: &[YamlValue], out: &mut Vec<String>) {
    for entry in entries {
        let YamlValue::Mapping(m) = entry else {
            continue;
        };
        out.extend(strings(get(m, "file")));
        out.extend(strings(get(m, "pattern")));
        for key in ["children", "chapters", "sections"] {
            if let Some(YamlValue::Sequence(children)) = get(m, key) {
                myst_toc(children, out);
            }
        }
    }
}

/// Paths a content file references: includes and embeds, image/figure and
/// include directives, markdown images and links, HTML `src`, and
/// file-valued frontmatter keys. URLs, anchors and label references are
/// skipped. Paths are returned as written (relative to the file, or
/// root-absolute with a leading `/`).
#[must_use]
pub fn referenced_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok((Some(fm), _, _)) = crate::reader::split_frontmatter(text) {
        if let YamlValue::Mapping(m) = &fm.parsed {
            out.extend(file_values(m));
        }
    }
    for line in text.lines() {
        let trimmed = line.trim_start();
        // {{< include x >}} / {{< embed x#cell >}} / {{< video x >}}
        let mut rest = trimmed;
        while let Some(start) = rest.find("{{<") {
            let after = &rest[start + 3..];
            let Some(end) = after.find(">}}") else { break };
            let mut words = after[..end].split_whitespace();
            if let (Some(name), Some(arg)) = (words.next(), words.next()) {
                if matches!(name, "include" | "embed" | "video") {
                    out.push(arg.to_string());
                }
            }
            rest = &after[end + 3..];
        }
        // ```{figure} x / :::{include} x
        let fence = trimmed.trim_start_matches(['`', ':']);
        if fence.len() < trimmed.len() {
            if let Some(body) = fence.strip_prefix('{') {
                if let Some((name, arg)) = body.split_once('}') {
                    if matches!(name, "include" | "literalinclude" | "figure" | "image") {
                        if let Some(arg) = arg.split_whitespace().next() {
                            out.push(arg.to_string());
                        }
                    }
                }
            }
        }
        // ](target "title")
        let mut rest = line;
        while let Some(start) = rest.find("](") {
            let after = &rest[start + 2..];
            let end = after.find(')').unwrap_or(after.len());
            let target = after[..end]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_start_matches('<')
                .trim_end_matches('>');
            out.push(target.to_string());
            rest = &after[end.min(after.len())..];
        }
        // src="x"
        let mut rest = line;
        while let Some(start) = rest.find("src=\"") {
            let after = &rest[start + 5..];
            let end = after.find('"').unwrap_or(after.len());
            out.push(after[..end].to_string());
            rest = &after[end.min(after.len())..];
        }
    }
    out.into_iter()
        .map(|p| p.split(['#', '?']).next().unwrap_or("").trim().to_string())
        .filter(|p| !p.is_empty() && !p.contains("://") && !p.starts_with("mailto:"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "mystquarto-closure-test-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn rels(root: &Path, files: &[PathBuf]) -> Vec<String> {
        files
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn raw_tier() -> String {
        ["data", "raw"].join("/")
    }

    /// An ASK-shaped project: the manuscript and its closure, plus decoys
    /// that must never be picked up.
    fn ask_project(root: &Path) {
        write(
            root,
            "_quarto.yml",
            "project:\n  type: manuscript\n  render:\n    - index.qmd\n    - manuscript/supplementary/*.qmd\nmanuscript:\n  article: index.qmd\n  resources:\n    - images/banner.png\n",
        );
        write(
            root,
            "index.qmd",
            "---\nbibliography: literature/references.bib\n---\n\n{{< include manuscript/sections/_intro.qmd >}}\n\n![Fig](images/fig1.png){#fig-a}\n",
        );
        write(
            root,
            "manuscript/sections/_intro.qmd",
            "# Intro\n\n![B](../../images/fig2.png)\n",
        );
        write(root, "manuscript/supplementary/s1.qmd", "# S1\n");
        write(root, "images/fig1.png", "png");
        write(root, "images/fig2.png", "png");
        write(root, "images/banner.png", "png");
        write(root, "images/unused.png", "png");
        write(root, "literature/references.bib", "@article{a,\n}\n");
        write(root, "literature/og/paper.txt", "original");
        write(root, "AGENTS.md", "# agents\n");
        write(root, "README.md", "# readme\n");
        write(root, "plans/plan.md", "# plan\n");
        write(root, "analysis/unchecked/explore.qmd", "# explore\n");
        write(root, "data/obfuscated/masked.csv", "id\n");
        write(root, &format!("{}/secret.csv", raw_tier()), "secret\n");
    }

    fn resolve_config(root: &Path) -> FileSet {
        let config = root.join("_quarto.yml");
        resolve(&Request {
            root,
            output_root: None,
            source_ext: "qmd",
            start: Start::Config {
                path: &config,
                dialect: ConfigDialect::Quarto,
            },
        })
    }

    #[test]
    fn a_config_closure_holds_only_what_the_manuscript_reaches() {
        let root = tempdir("ask");
        ask_project(&root);
        let set = resolve_config(&root);
        assert_eq!(
            rels(&root, &set.content),
            [
                "index.qmd",
                "manuscript/sections/_intro.qmd",
                "manuscript/supplementary/s1.qmd"
            ]
        );
        assert_eq!(
            rels(&root, &set.assets),
            [
                "images/banner.png",
                "images/fig1.png",
                "images/fig2.png",
                "literature/references.bib"
            ]
        );
        assert!(set.diagnostics.is_empty(), "{:?}", set.diagnostics);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_reference_into_a_governed_location_is_an_error_and_never_read() {
        let root = tempdir("denied");
        ask_project(&root);
        write(
            &root,
            "index.qmd",
            &format!(
                "![Leak](../{0}/secret.csv)\n![Leak]({0}/secret.csv)\n",
                raw_tier()
            ),
        );
        let set = resolve_config(&root);
        assert!(set
            .assets
            .iter()
            .chain(&set.content)
            .all(|p| !p.to_string_lossy().contains(&raw_tier())));
        let errors: Vec<_> = set
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error && d.code == codes::DENIED_REFERENCE)
            .collect();
        assert_eq!(errors.len(), 1, "{:?}", set.diagnostics);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_gitignored_closure_file_is_skipped_with_a_warning() {
        let root = tempdir("gitignored");
        ask_project(&root);
        write(&root, ".gitignore", "images/fig2.png\n");
        let set = resolve_config(&root);
        assert!(!rels(&root, &set.assets).contains(&"images/fig2.png".to_string()));
        assert!(set
            .diagnostics
            .iter()
            .any(|d| d.code == codes::CLOSURE_FILE_IGNORED));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bare_folder_walk_skips_agent_files_gitignored_files_and_the_denylist() {
        let root = tempdir("walk");
        write(&root, "doc.md", "# Doc\n");
        write(&root, "img/a.png", "png");
        write(&root, "AGENTS.md", "x");
        write(&root, "CLAUDE.md", "x");
        write(&root, "README.md", "x");
        write(&root, "plans/p.md", "x");
        write(&root, "secret/s.md", "x");
        write(&root, ".gitignore", "secret/\n");
        write(&root, ".env.local", "TOKEN=x");
        write(&root, &format!("{}/r.md", raw_tier()), "x");
        let set = resolve(&Request {
            root: &root,
            output_root: None,
            source_ext: "md",
            start: Start::Walk { all: false },
        });
        assert_eq!(rels(&root, &set.content), ["doc.md"]);
        assert_eq!(rels(&root, &set.assets), [".gitignore", "img/a.png"]);

        let all = resolve(&Request {
            root: &root,
            output_root: None,
            source_ext: "md",
            start: Start::Walk { all: true },
        });
        let content = rels(&root, &all.content);
        assert!(content.contains(&"README.md".to_string()));
        assert!(!content
            .iter()
            .any(|c| c.starts_with("secret") || c.contains("raw")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_myst_toc_with_children_and_patterns_seeds_the_closure() {
        let root = tempdir("myst");
        write(
            &root,
            "myst.yml",
            "version: 1\nproject:\n  bibliography: refs.bib\n  toc:\n    - file: index\n    - title: Part\n      children:\n        - pattern: chapters/*.md\n",
        );
        write(
            &root,
            "index.md",
            "```{figure} img/f.png\n:label: fig-f\n```\n\n```{include} snippets/_s.md\n```\n",
        );
        write(&root, "snippets/_s.md", "snippet\n");
        write(&root, "chapters/a.md", "# A\n");
        write(&root, "chapters/b.md", "# B\n");
        write(&root, "img/f.png", "png");
        write(&root, "refs.bib", "@a{b,\n}\n");
        write(&root, "notes.md", "# stray\n");
        let config = root.join("myst.yml");
        let set = resolve(&Request {
            root: &root,
            output_root: None,
            source_ext: "md",
            start: Start::Config {
                path: &config,
                dialect: ConfigDialect::Myst,
            },
        });
        assert_eq!(
            rels(&root, &set.content),
            [
                "chapters/a.md",
                "chapters/b.md",
                "index.md",
                "snippets/_s.md"
            ]
        );
        assert_eq!(rels(&root, &set.assets), ["img/f.png", "refs.bib"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_myst_project_without_a_toc_includes_every_document_and_root_bibs() {
        let root = tempdir("myst-no-toc");
        write(
            &root,
            "myst.yml",
            "version: 1\nproject:\n  exports:\n    - format: pdf\n      article: article.md\n",
        );
        write(&root, "article.md", "# A\n");
        write(&root, "notebooks/explore.md", "# N\n");
        write(&root, "notebooks/run.ipynb", "{}");
        write(&root, "references.bib", "@a{b,\n}\n");
        write(&root, "sub/nested.bib", "@a{c,\n}\n");
        write(&root, "AGENTS.md", "x");
        let config = root.join("myst.yml");
        let set = resolve(&Request {
            root: &root,
            output_root: None,
            source_ext: "md",
            start: Start::Config {
                path: &config,
                dialect: ConfigDialect::Myst,
            },
        });
        assert_eq!(
            rels(&root, &set.content),
            ["article.md", "notebooks/explore.md"]
        );
        assert_eq!(rels(&root, &set.notebooks), ["notebooks/run.ipynb"]);
        assert!(rels(&root, &set.assets).contains(&"references.bib".to_string()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_config_with_no_content_roots_falls_back_to_the_filtered_walk() {
        let root = tempdir("fallback");
        write(&root, "_quarto.yml", "project:\n  type: website\n");
        write(&root, "index.qmd", "# Home\n");
        write(&root, "about.qmd", "# About\n");
        write(&root, "AGENTS.md", "x");
        let set = resolve_config(&root);
        assert_eq!(rels(&root, &set.content), ["about.qmd", "index.qmd"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn configs_are_never_assets_and_a_nested_output_root_is_never_walked() {
        let root = tempdir("nested-output");
        write(&root, "myst.yml", "version: 1\n");
        write(&root, "_quarto.yml", "project: {}\n");
        write(&root, "doc.md", "# Doc\n");
        write(&root, "helper.py", "print(1)\n");
        write(&root, "docs-quarto/doc.qmd", "# prior output\n");
        write(&root, "docs-quarto/banner.png", "png");
        let output = root.join("docs-quarto");
        let set = resolve(&Request {
            root: &root,
            output_root: Some(&output),
            source_ext: "md",
            start: Start::Walk { all: false },
        });
        assert_eq!(rels(&root, &set.content), ["doc.md"]);
        assert_eq!(rels(&root, &set.assets), ["helper.py"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_output_dir_is_gitignored_by_a_root_or_nested_gitignore() {
        let root = tempdir("gitignored-dir");
        write(&root, ".gitignore", "_build/\n");
        write(&root, "site/.gitignore", "preview/\n");
        fs::create_dir_all(root.join("_build/myst")).unwrap();
        fs::create_dir_all(root.join("site/preview")).unwrap();
        fs::create_dir_all(root.join("manuscript")).unwrap();
        assert!(is_gitignored_dir(&root, &root.join("_build/myst")));
        assert!(is_gitignored_dir(&root, &root.join("site/preview")));
        assert!(!is_gitignored_dir(&root, &root.join("manuscript")));
        assert!(!is_gitignored_dir(&root, &root.join("not-yet-created")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn referenced_paths_skip_urls_anchors_and_strip_fragments() {
        let refs = referenced_paths(
            "See [a](other.md#sec), [b](https://x.org/y), [c](#local), ![d](img/x.png \"t\").\n<img src=\"img/y.png\">\n{{< embed nb.ipynb#fig-a >}}\n",
        );
        assert_eq!(refs, ["other.md", "img/x.png", "img/y.png", "nb.ipynb"]);
    }

    #[test]
    fn the_denylist_matches_governed_prefixes_and_components() {
        for denied in [
            format!("{}/x.csv", raw_tier()),
            "literature/og/p.pdf".to_string(),
            ".ask/state.json".to_string(),
            "sub/.git/config".to_string(),
            ".env".to_string(),
            "conf/.env.local".to_string(),
        ] {
            assert!(is_denied(Path::new(&denied)), "{denied}");
        }
        for allowed in [
            "data/obfuscated/x.csv",
            "literature/references.bib",
            "environment.yml",
        ] {
            assert!(!is_denied(Path::new(allowed)), "{allowed}");
        }
    }
}
