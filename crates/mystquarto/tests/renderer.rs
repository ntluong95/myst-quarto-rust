#![cfg(feature = "renderer-tests")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn tempdir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "mystquarto-renderer-test-{label}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn cleanup(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
}

fn tree_snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn run(mut cmd: Command, label: &str) -> String {
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("{label} failed: {e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{label} exited with {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    format!("{stdout}{stderr}")
}

#[test]
fn converted_article_template_renders_and_builds_with_resolved_references() {
    let tmp = tempdir("article");
    let quarto_dir = tmp.join("quarto");
    let myst_dir = tmp.join("myst");
    let root = workspace_root();

    let mut convert = Command::new(env!("CARGO_BIN_EXE_myst2quarto"));
    convert
        .arg(root.join("article-template"))
        .arg("-o")
        .arg(&quarto_dir)
        .arg("--strict");
    run(convert, "myst2quarto --strict article-template");

    let mut strict_all = Command::new(env!("CARGO_BIN_EXE_myst2quarto"));
    strict_all
        .arg(root.join("article-template"))
        .arg("-o")
        .arg(tmp.join("quarto-strict-all"))
        .arg("--strict=all");
    assert!(
        !strict_all.output().unwrap().status.success(),
        "--strict=all must fail on expected-lossy preservation"
    );

    let mut quarto = Command::new("quarto");
    quarto
        .current_dir(&quarto_dir)
        .arg("render")
        .arg("article.qmd")
        .arg("--to")
        .arg("html")
        .arg("--no-execute");
    let quarto_log = run(quarto, "quarto render");
    let quarto_log_path = tmp.join("quarto-render.log");
    fs::write(&quarto_log_path, quarto_log).unwrap();
    assert!(
        quarto_dir.join("_manuscript/index.html").exists(),
        "quarto render must produce manuscript HTML"
    );

    let mut refs = Command::new(root.join("scripts/check-refs.sh"));
    refs.arg(quarto_dir.join("_manuscript"))
        .arg(&quarto_log_path)
        .arg("10.1038/nmeth.1974")
        .arg("10.1038/nprot.2013.143");
    run(refs, "check-refs.sh");

    let mut reverse = Command::new(env!("CARGO_BIN_EXE_quarto2myst"));
    reverse.arg(&quarto_dir).arg("-o").arg(&myst_dir);
    run(reverse, "quarto2myst converted article-template");

    // RT-14 / Hermetic CI: Seed the offline CSL-JSON cache so myst build does not make live network calls
    let cache_src = root.join("tests/fixtures/csl_cache");
    let cache_dst = myst_dir.join("_build/cache");
    if cache_src.exists() {
        fs::create_dir_all(&cache_dst).unwrap();
        for entry in fs::read_dir(&cache_src).unwrap().flatten() {
            let _ = fs::copy(entry.path(), cache_dst.join(entry.file_name()));
        }
    }

    let mut myst = Command::new("myst");
    myst.current_dir(&myst_dir)
        .arg("build")
        .arg("article.md")
        .arg("--md")
        .arg("--force");
    let myst_log = run(myst, "myst build");
    assert!(
        !myst_log.contains("Unable to resolve")
            && !myst_log.contains("not found")
            && !myst_log.contains("Could not link citation")
            && !myst_log.contains("unexpected option"),
        "myst build reported unresolved references or options:\n{myst_log}"
    );

    cleanup(&tmp);
}

#[test]
fn converting_article_template_twice_is_byte_identical_without_nesting() {
    let tmp = tempdir("idempotent");
    let quarto_dir = tmp.join("quarto");
    let source = workspace_root().join("article-template");

    let mut first = Command::new(env!("CARGO_BIN_EXE_myst2quarto"));
    first.arg(&source).arg("-o").arg(&quarto_dir);
    run(first, "first article-template conversion");
    let before = tree_snapshot(&quarto_dir);

    let mut second = Command::new(env!("CARGO_BIN_EXE_myst2quarto"));
    second
        .arg(&source)
        .arg("-o")
        .arg(&quarto_dir)
        .arg("--force");
    run(second, "second article-template conversion");
    let after = tree_snapshot(&quarto_dir);

    assert_eq!(before, after, "second conversion changed output bytes");
    assert!(
        !quarto_dir.join("quarto").exists(),
        "second conversion must not nest the output inside itself"
    );

    cleanup(&tmp);
}

// ---------------------------------------------------------------------------
// End-to-end project harness (`tests/e2e/<fixture>/`).
//
// Each fixture is converted in its natural direction, the output rendered
// with the real renderer, converted back, rendered again, and diffed
// against the original. Every problem is collected, not just the first, so
// one failing run lists everything that is still wrong.
// ---------------------------------------------------------------------------

use std::collections::BTreeSet;
use std::process::Stdio;
use std::time::{Duration, Instant};

use mystquarto_core::yaml::{parse_mapping, YamlValue};

const RENDER_TIMEOUT: Duration = Duration::from_secs(240);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Myst,
    Quarto,
}

impl Dialect {
    fn other(self) -> Self {
        match self {
            Dialect::Myst => Dialect::Quarto,
            Dialect::Quarto => Dialect::Myst,
        }
    }
    fn config(self) -> &'static str {
        match self {
            Dialect::Myst => "myst.yml",
            Dialect::Quarto => "_quarto.yml",
        }
    }
    /// The `mystquarto` subcommand that converts *into* this dialect.
    fn subcommand_into(self) -> &'static str {
        match self {
            Dialect::Myst => "to-myst",
            Dialect::Quarto => "to-quarto",
        }
    }
}

struct Fixture {
    name: &'static str,
    source: Dialect,
    /// Content files of the source fixture that must survive a round trip.
    content: &'static [&'static str],
}

/// Runs `cmd` with stdin closed and `CI=1`, killing it after
/// [`RENDER_TIMEOUT`]. Returns `(success, combined output)`.
fn run_bounded(mut cmd: Command) -> (bool, String) {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let log_path = std::env::temp_dir().join(format!(
        "mystquarto-e2e-log-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let log = fs::File::create(&log_path).unwrap();
    let mut child = cmd
        .env("CI", "1")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap_or_else(|e| panic!("could not spawn {cmd:?}: {e}"));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if start.elapsed() > RENDER_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let text = fs::read_to_string(&log_path).unwrap_or_default();
    let _ = fs::remove_file(&log_path);
    match status {
        Some(s) => (s.success(), text),
        None => (false, format!("{text}\n<killed after {RENDER_TIMEOUT:?}>")),
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git must be available")
}

fn git_init_commit(dir: &Path) {
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "e2e@example.org"],
        &["config", "user.name", "e2e"],
        &["add", "-A"],
        &["commit", "-q", "-m", "fixture"],
    ] {
        assert!(git(dir, args).status.success(), "git {args:?} failed");
    }
}

fn relative_files(root: &Path) -> Vec<String> {
    if !root.exists() {
        return Vec::new();
    }
    tree_snapshot(root)
        .into_iter()
        .map(|(p, _)| p.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn read_list(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// MyST log lines that fail the harness: any error, plus the warnings that
/// mean content was lost or mis-parsed.
fn myst_problems(log: &str) -> Vec<String> {
    const FATAL_WARNINGS: &[&str] = &[
        "unknown directive",
        "unknown role",
        "unknown option",
        "unexpected option",
        "not found",
        "could not find",
        "unexpected content",
        "invalid",
        "could not link",
        "cannot be resolved",
        "unable to resolve",
    ];
    log.lines()
        .filter(|l| {
            let lower = l.to_lowercase();
            l.contains('⛔')
                || (l.contains('⚠') && FATAL_WARNINGS.iter().any(|w| lower.contains(w)))
        })
        .map(|l| l.trim().to_string())
        .collect()
}

fn quarto_problems(log: &str) -> Vec<String> {
    log.lines()
        .filter(|l| {
            let lower = l.to_lowercase();
            l.contains("ERROR")
                || (lower.contains("warn")
                    && ((lower.contains("citation") && lower.contains("not found"))
                        || lower.contains("unable to resolve crossref")))
        })
        .map(|l| l.trim().to_string())
        .collect()
}

/// Renders a *copy* of `dir` so the tree under test is never polluted with
/// renderer output, returning every problem line.
fn render(dialect: Dialect, dir: &Path, scratch: &Path, label: &str) -> Vec<String> {
    if !dir.exists() {
        return vec![format!(
            "[{label}] nothing to render: {} missing",
            dir.display()
        )];
    }
    let copy = scratch.join(format!("render-{}", label.replace(' ', "-")));
    copy_dir(dir, &copy);
    let mut cmd = match dialect {
        Dialect::Myst => {
            let mut c = Command::new("myst");
            c.args(["build", "--html"]);
            c
        }
        Dialect::Quarto => {
            let mut c = Command::new("quarto");
            c.arg("render");
            c
        }
    };
    cmd.current_dir(&copy);
    let (ok, log) = run_bounded(cmd);
    let mut problems = match dialect {
        Dialect::Myst => myst_problems(&log),
        Dialect::Quarto => quarto_problems(&log),
    };
    if !ok {
        problems.push(format!(
            "renderer exited non-zero; log tail:\n{}",
            tail(&log, 15)
        ));
    }
    // `myst build --html` exits 0 without writing a page when the config
    // has no site; that is a failed build too.
    if dialect == Dialect::Myst && ok && html_text(&copy.join("_build/html")).is_empty() {
        problems.push("myst build --html produced no HTML".into());
    }
    problems
        .into_iter()
        .map(|p| format!("[{label}] {p}"))
        .collect()
}

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn convert(into: Dialect, input: &Path, output: &Path, label: &str) -> Vec<String> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mystquarto"));
    cmd.arg(into.subcommand_into())
        .arg(input)
        .arg("-o")
        .arg(output);
    let (ok, log) = run_bounded(cmd);
    if ok {
        Vec::new()
    } else {
        vec![format!(
            "[{label}] conversion exited non-zero:\n{}",
            tail(&log, 15)
        )]
    }
}

/// Output must contain every exact entry of `expected-closure.txt` and
/// nothing outside it (a trailing `/` entry allows a whole subtree).
fn closure_problems(output: &Path, expected: &[String], label: &str) -> Vec<String> {
    let files = relative_files(output);
    let mut problems = Vec::new();
    for f in &files {
        let allowed = expected.iter().any(|e| {
            if e.ends_with('/') {
                f.starts_with(e.as_str())
            } else {
                f == e
            }
        });
        if !allowed {
            problems.push(format!(
                "[{label}] output outside the manuscript closure: {f}"
            ));
        }
    }
    for e in expected.iter().filter(|e| !e.ends_with('/')) {
        if !files.contains(e) {
            problems.push(format!("[{label}] closure file missing from output: {e}"));
        }
    }
    problems
}

fn normalized_lines(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn content_diff_problems(
    original: &Path,
    round_tripped: &Path,
    files: &[&str],
    allowed: &[String],
) -> Vec<String> {
    let mut problems = Vec::new();
    for rel in files {
        let before = fs::read_to_string(original.join(rel)).unwrap();
        let Ok(after) = fs::read_to_string(round_tripped.join(rel)) else {
            problems.push(format!("[round-trip] {rel} missing after round trip"));
            continue;
        };
        let (before, after) = (normalized_lines(&before), normalized_lines(&after));
        let diff = similar::TextDiff::from_lines(&before, &after);
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                similar::ChangeTag::Equal => continue,
                similar::ChangeTag::Delete => '-',
                similar::ChangeTag::Insert => '+',
            };
            let line = change.value().trim();
            if allowed.iter().any(|a| a == &format!("{rel}: {line}")) {
                continue;
            }
            problems.push(format!("[round-trip] {rel} {sign} {line}"));
        }
    }
    problems
}

/// Sorts mapping keys recursively so two configs compare by meaning, not
/// by key order or comments.
fn canonical_yaml(value: YamlValue) -> YamlValue {
    match value {
        YamlValue::Mapping(mut pairs) => {
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            YamlValue::Mapping(
                pairs
                    .into_iter()
                    .map(|(k, v)| (k, canonical_yaml(v)))
                    .collect(),
            )
        }
        YamlValue::Sequence(items) => {
            YamlValue::Sequence(items.into_iter().map(canonical_yaml).collect())
        }
        YamlValue::BlockLiteral(s) => YamlValue::String(s),
        other => other,
    }
}

fn config_diff_problems(original: &Path, round_tripped: &Path, name: &str) -> Vec<String> {
    let parse = |dir: &Path| {
        fs::read_to_string(dir.join(name))
            .ok()
            .and_then(|t| parse_mapping(&t).ok())
    };
    let (Some(a), Some(b)) = (parse(original), parse(round_tripped)) else {
        return vec![format!(
            "[round-trip] {name} missing or unparseable after round trip"
        )];
    };
    let keys: BTreeSet<&String> = a.iter().chain(b.iter()).map(|(k, _)| k).collect();
    let find = |m: &[(String, YamlValue)], key: &str| {
        m.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| canonical_yaml(v.clone()))
    };
    keys.into_iter()
        .filter_map(|key| {
            let (before, after) = (find(&a, key), find(&b, key));
            (before != after).then(|| {
                format!(
                    "[round-trip] {name} `{key}` changed:\n    before: {before:?}\n    after:  {after:?}"
                )
            })
        })
        .collect()
}

fn renderer_versions() -> String {
    let v = |bin: &str| {
        Command::new(bin)
            .arg("--version")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "unavailable".into())
    };
    format!("quarto {} / myst {}", v("quarto"), v("myst"))
}

fn run_e2e(fixture: &Fixture) {
    println!("e2e {} with {}", fixture.name, renderer_versions());
    let fixture_dir = workspace_root().join("tests/e2e").join(fixture.name);
    let tmp = tempdir(&format!("e2e-{}", fixture.name));
    let src = tmp.join("src");
    let converted = tmp.join("converted");
    let round_tripped = tmp.join("round-tripped");
    let target = fixture.source.other();
    let mut problems = Vec::new();

    copy_dir(&fixture_dir, &src);
    for meta in ["expected-closure.txt", "allowed-diff.txt"] {
        let _ = fs::remove_file(src.join(meta));
    }
    // A gitignored raw-data tier the converter must never read or copy. The
    // path is assembled at runtime so no literal of it lives in the repo.
    let raw_tier = ["data", "raw"].join("/");
    fs::create_dir_all(src.join(&raw_tier)).unwrap();
    fs::write(src.join(&raw_tier).join("secret.csv"), "id,secret\n1,x\n").unwrap();
    let gitignore = src.join(".gitignore");
    let mut ignore_text = fs::read_to_string(&gitignore).unwrap_or_default();
    ignore_text.push_str(&format!("{raw_tier}/\n"));
    fs::write(&gitignore, ignore_text).unwrap();
    git_init_commit(&src);

    problems.extend(render(fixture.source, &src, &tmp, "source render"));

    problems.extend(convert(target, &src, &converted, "convert"));
    problems.extend(closure_problems(
        &converted,
        &read_list(&fixture_dir.join("expected-closure.txt")),
        "closure",
    ));
    if relative_files(&converted)
        .iter()
        .any(|f| f.starts_with(&raw_tier))
    {
        problems.push("[closure] raw-data tier copied into output".into());
    }
    problems.extend(render(target, &converted, &tmp, "converted render"));

    problems.extend(convert(
        fixture.source,
        &converted,
        &round_tripped,
        "convert back",
    ));
    problems.extend(render(
        fixture.source,
        &round_tripped,
        &tmp,
        "round-trip render",
    ));
    problems.extend(content_diff_problems(
        &src,
        &round_tripped,
        fixture.content,
        &read_list(&fixture_dir.join("allowed-diff.txt")),
    ));
    problems.extend(config_diff_problems(
        &src,
        &round_tripped,
        fixture.source.config(),
    ));

    let status = git(&src, &["status", "--porcelain"]);
    let dirty = String::from_utf8_lossy(&status.stdout).trim().to_string();
    if !dirty.is_empty() {
        problems.push(format!(
            "[source] conversion modified the source tree:\n{dirty}"
        ));
    }

    if problems.is_empty() {
        cleanup(&tmp);
    } else {
        panic!(
            "{} e2e problem(s) for {} (scratch kept at {}):\n{}",
            problems.len(),
            fixture.name,
            tmp.display(),
            problems.join("\n")
        );
    }
}

#[test]
fn e2e_ask_manuscript_converts_renders_and_round_trips_cleanly() {
    run_e2e(&Fixture {
        name: "ask-manuscript",
        source: Dialect::Quarto,
        content: &["index.qmd", "manuscript/sections/_intro.qmd"],
    });
}

#[test]
fn e2e_myst_native_converts_renders_and_round_trips_cleanly() {
    run_e2e(&Fixture {
        name: "myst-native",
        source: Dialect::Myst,
        content: &["index.md", "chapter.md"],
    });
}

/// Every `.html` file under `dir`, concatenated.
fn html_text(dir: &Path) -> String {
    if !dir.exists() {
        return String::new();
    }
    tree_snapshot(dir)
        .into_iter()
        .filter(|(p, _)| p.extension().is_some_and(|e| e == "html"))
        .map(|(_, bytes)| String::from_utf8_lossy(&bytes).into_owned())
        .collect()
}

/// A single manuscript file in a repo without a project config, whose
/// include, figure and bibliography sit in sibling folders reached with
/// `../`. Converting just that file must produce a project that builds.
#[test]
fn single_file_mode_builds_on_its_own_in_both_directions() {
    let tmp = tempdir("single-file");
    let src = tmp.join("src");
    let ask = workspace_root().join("tests/e2e/ask-manuscript");
    fs::create_dir_all(src.join("manuscript/sections")).unwrap();
    copy_dir(&ask.join("images"), &src.join("images"));
    copy_dir(&ask.join("literature"), &src.join("literature"));
    fs::write(
        src.join("manuscript/sections/_intro.qmd"),
        "# Introduction {#sec-intro}\n\nBackground text citing [see @smith2020, p. 3].\n",
    )
    .unwrap();
    fs::write(
        src.join("manuscript/index.qmd"),
        "---\ntitle: Single file\nbibliography: ../literature/references.bib\n---\n\n\
         {{< include sections/_intro.qmd >}}\n\n# Methods\n\nSee @fig-main and @sec-intro.\n\n\
         ![Main result figure](../images/fig1.png){#fig-main}\n",
    )
    .unwrap();
    git_init_commit(&src);
    let mut problems = Vec::new();

    let myst = tmp.join("myst");
    let mut to_myst = Command::new(env!("CARGO_BIN_EXE_quarto2myst"));
    to_myst
        .arg(src.join("manuscript/index.qmd"))
        .arg("-o")
        .arg(&myst);
    let (ok, log) = run_bounded(to_myst);
    assert!(ok, "quarto2myst single file failed:\n{log}");
    assert!(myst.join("images/fig1.png").exists(), "figure not copied");
    assert!(myst.join("myst.yml").exists(), "no config synthesized");
    problems.extend(render(Dialect::Myst, &myst, &tmp, "single myst"));
    let html = html_text(&tmp.join("render-single-myst/_build/html"));
    if !html.contains("Background text") {
        problems.push("[single myst] the included Introduction is missing".into());
    }
    if !html.contains("fig1") {
        problems.push("[single myst] the figure image is missing".into());
    }

    let quarto = tmp.join("quarto");
    let mut to_quarto = Command::new(env!("CARGO_BIN_EXE_myst2quarto"));
    to_quarto
        .arg(myst.join("manuscript/index.md"))
        .arg("-o")
        .arg(&quarto);
    let (ok, log) = run_bounded(to_quarto);
    assert!(ok, "myst2quarto single file failed:\n{log}");
    problems.extend(render(Dialect::Quarto, &quarto, &tmp, "single quarto"));

    if problems.is_empty() {
        cleanup(&tmp);
    } else {
        panic!(
            "single-file problems (scratch kept at {}):\n{}",
            tmp.display(),
            problems.join("\n")
        );
    }
}
