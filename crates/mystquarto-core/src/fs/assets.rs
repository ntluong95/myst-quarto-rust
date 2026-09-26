//! Copies a given list of non-Markdown, non-config files from an input tree
//! to an output tree — the images, `.bib` files, notebooks, and other static content a
//! `quarto render`/`myst build` needs alongside the converted `.qmd`/`.md`
//! files.
//!
//! Three policies, each stated once here rather than scattered across call
//! sites:
//!
//! 1. **Never dereference a symlink.** Every entry is classified with
//!    [`std::fs::symlink_metadata`] (never [`std::fs::metadata`], which
//!    follows symlinks) before it is touched. A symlink — to a file or a
//!    directory — is never copied and never descended into; it is recorded
//!    in the returned [`AssetCopyReport::skipped_symlinks`] instead. This is
//!    the fix for the reproduced hazard: `shutil.copy2` in the Python
//!    implementation follows symlinks, so a symlink planted in an input
//!    tree pointing at a secrets file outside it gets that file's *content*
//!    copied into the output tree as an ordinary file. Skipping (rather
//!    than recreating the symlink at the destination) is the safer default
//!    — see this phase's report for why recreation was not chosen.
//! 2. **Copy only what the caller names.** Which files are assets is
//!    [`crate::closure`]'s decision (the manuscript closure, or a filtered
//!    walk), which also keeps the output root out of the file set (the D16
//!    fix). This module never walks a directory itself.
//! 3. **Refresh-on-change policy.** A destination that already exists is
//!    left alone only if its mtime is *exactly* equal to the source's (the
//!    cheap, common no-op case — nothing has changed since the last run).
//!    Any other case — the mtimes differ, or either file's mtime cannot be
//!    read on this platform — falls back to a full content-hash comparison
//!    before deciding whether to skip. This replaces the Python
//!    implementation's `if not os.path.exists(dst): copy` check, which
//!    never refreshes a destination that already exists no matter how the
//!    source has since changed.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hasher;
use std::io;
use std::path::{Path, PathBuf};

/// Build, cache and tool folders no file set ever includes (used by
/// [`crate::closure`]'s walk).
pub const ASSET_SKIP_DIRS: &[&str] = &[
    "_build",
    ".git",
    ".hg",
    "__pycache__",
    "node_modules",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    "_site",
    "_manuscript",
    ".quarto",
    // mystquarto's own sidecar directory (labels.json, preserved.json):
    // never a generic asset. Walking into it here would let a stale copy in
    // the input tree overwrite the freshly-written sidecar in the output
    // tree — those writers own this directory exclusively.
    ".mystquarto",
];

/// What happened during one [`copy_assets`] call, broken down so a caller
/// can report skipped symlinks as a diagnostic without treating them as a
/// hard error.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AssetCopyReport {
    /// Destination paths that were written (new file, or refreshed because
    /// the source changed).
    pub copied: Vec<PathBuf>,
    /// Destination paths that already matched the source and were left
    /// alone.
    pub unchanged: Vec<PathBuf>,
    /// Source paths that were symlinks and were therefore skipped entirely
    /// — never copied, never dereferenced.
    pub skipped_symlinks: Vec<PathBuf>,
}

/// Error from [`copy_assets`]. Each variant names the path being operated
/// on so a caller can report exactly which asset failed.
#[derive(Debug, thiserror::Error)]
pub enum AssetCopyError {
    #[error("failed to read directory {path}: {source}")]
    ReadDir { path: PathBuf, source: io::Error },
    #[error("failed to stat {path}: {source}")]
    Stat { path: PathBuf, source: io::Error },
    #[error("failed to create directory {path}: {source}")]
    CreateDir { path: PathBuf, source: io::Error },
    #[error("failed to copy {src} to {dst}: {source}")]
    Copy {
        src: PathBuf,
        dst: PathBuf,
        source: io::Error,
    },
}

/// Copies each of `files` (absolute paths under `input_root`) to the
/// matching relative path under `output_root`, applying the policies
/// documented on this module. `output_root` need not exist yet; directories
/// are created as needed.
///
/// # Errors
/// Returns the first I/O failure (stat, directory creation, or copy).
pub fn copy_files(
    input_root: &Path,
    output_root: &Path,
    files: &[PathBuf],
) -> Result<AssetCopyReport, AssetCopyError> {
    let mut report = AssetCopyReport::default();
    for path in files {
        let meta = fs::symlink_metadata(path).map_err(|source| AssetCopyError::Stat {
            path: path.clone(),
            source,
        })?;
        if meta.file_type().is_symlink() {
            report.skipped_symlinks.push(path.clone());
            continue;
        }
        if meta.is_dir() {
            continue;
        }
        let rel = path.strip_prefix(input_root).unwrap_or(path);
        let dst = output_root.join(rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|source| AssetCopyError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        if needs_refresh(path, &dst)? {
            fs::copy(path, &dst).map_err(|source| AssetCopyError::Copy {
                src: path.clone(),
                dst: dst.clone(),
                source,
            })?;
            report.copied.push(dst);
        } else {
            report.unchanged.push(dst);
        }
    }
    Ok(report)
}

fn needs_refresh(src: &Path, dst: &Path) -> Result<bool, AssetCopyError> {
    let dst_meta = match fs::metadata(dst) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(source) => {
            return Err(AssetCopyError::Stat {
                path: dst.to_path_buf(),
                source,
            })
        }
    };
    let src_meta = fs::metadata(src).map_err(|source| AssetCopyError::Stat {
        path: src.to_path_buf(),
        source,
    })?;

    if let (Ok(s), Ok(d)) = (src_meta.modified(), dst_meta.modified()) {
        if s == d {
            return Ok(false);
        }
    }

    content_differs(src, dst)
}

fn content_differs(a: &Path, b: &Path) -> Result<bool, AssetCopyError> {
    Ok(hash_file(a)? != hash_file(b)?)
}

fn hash_file(path: &Path) -> Result<u64, AssetCopyError> {
    let bytes = fs::read(path).map_err(|source| AssetCopyError::Stat {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = DefaultHasher::new();
    hasher.write(&bytes);
    Ok(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("mystquarto-assets-test-{label}-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_secret_outside_input_root_is_never_dereferenced() {
        let tmp = tempdir("symlink-secret");
        let input_root = tmp.join("input");
        let output_root = tmp.join("output");
        let outside = tmp.join("outside");
        fs::create_dir_all(&input_root).unwrap();
        fs::create_dir_all(&outside).unwrap();

        let secret_path = outside.join("secret.txt");
        fs::write(&secret_path, "TOP SECRET").unwrap();
        let link = input_root.join("link.txt");
        std::os::unix::fs::symlink(&secret_path, &link).unwrap();

        let report = copy_files(&input_root, &output_root, std::slice::from_ref(&link))
            .expect("an out-of-root symlink is skipped, not an error");

        assert_eq!(report.skipped_symlinks, vec![link]);
        assert!(report.copied.is_empty());
        assert!(
            !output_root.join("link.txt").exists(),
            "a symlink must never be materialized in the output tree, even as a broken link"
        );

        cleanup(&tmp);
    }

    #[test]
    fn changed_source_content_refreshes_the_destination() {
        let tmp = tempdir("refresh");
        let input_root = tmp.join("input");
        let output_root = tmp.join("output");
        fs::create_dir_all(&input_root).unwrap();
        let data = input_root.join("data.csv");

        fs::write(&data, "a,b\n1,2\n").unwrap();
        copy_files(&input_root, &output_root, std::slice::from_ref(&data)).unwrap();
        assert_eq!(
            fs::read_to_string(output_root.join("data.csv")).unwrap(),
            "a,b\n1,2\n"
        );

        // Force a distinct mtime (some filesystems have 1s granularity) so
        // the cheap mtime-equality skip cannot mask the content change.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(&data, "a,b\n3,4\n").unwrap();

        let report = copy_files(&input_root, &output_root, std::slice::from_ref(&data)).unwrap();
        assert_eq!(report.copied, vec![output_root.join("data.csv")]);
        assert_eq!(
            fs::read_to_string(output_root.join("data.csv")).unwrap(),
            "a,b\n3,4\n",
            "changed source content must refresh the destination, not leave it stale"
        );

        cleanup(&tmp);
    }

    #[test]
    fn unchanged_source_is_not_recopied() {
        let tmp = tempdir("unchanged");
        let input_root = tmp.join("input");
        let output_root = tmp.join("output");
        fs::create_dir_all(input_root.join("img")).unwrap();
        let banner = input_root.join("img/banner.png");
        fs::write(&banner, b"fake-png-bytes").unwrap();

        copy_files(&input_root, &output_root, std::slice::from_ref(&banner)).unwrap();
        let report = copy_files(&input_root, &output_root, std::slice::from_ref(&banner)).unwrap();

        assert!(report.copied.is_empty());
        assert_eq!(report.unchanged, vec![output_root.join("img/banner.png")]);

        cleanup(&tmp);
    }
}
