//! Filesystem scan for maintained Rust sources.
//!
//! The walk is deterministic (sorted afterwards) and fails closed: a directory
//! or file that cannot be read is an error, never a silent zero. A symlinked
//! directory is refused rather than followed, because following it can leave
//! the repository or loop; a symlinked file is read through and counted.
//!
//! Only directories directly under the scan root may be artifacts. A nested
//! directory that happens to be called `target`, `output` or `.git` is ordinary
//! source-tree structure and is scanned, because a name collision must never
//! hide a maintained file.

use std::path::{Path, PathBuf};

/// Root-level generated build/runtime directories. These are not maintained
/// sources. The names apply only to entries directly under the scan root; see
/// the module documentation.
const ROOT_ARTIFACT_DIRECTORIES: &[&str] = &[".git", "target", "output"];

pub fn rust_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let metadata = std::fs::metadata(root)
        .map_err(|error| format!("cannot read {}: {error}", root.display()))?;
    if !metadata.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }

    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), true)];
    while let Some((directory, at_root)) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| format!("cannot list {}: {error}", directory.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("cannot list {}: {error}", directory.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;

            if file_type.is_symlink() {
                let target = std::fs::metadata(&path)
                    .map_err(|error| format!("cannot follow {}: {error}", path.display()))?;
                if target.is_dir() {
                    return Err(format!(
                        "{} is a symlinked directory; refusing to scan outside the tree",
                        path.display()
                    ));
                }
                if target.is_file() && is_rust(&path) {
                    files.push(path);
                }
                continue;
            }

            if file_type.is_dir() {
                let skip = at_root
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| ROOT_ARTIFACT_DIRECTORIES.contains(&name));
                if !skip {
                    pending.push((path, false));
                }
                continue;
            }

            if file_type.is_file() && is_rust(&path) {
                files.push(path);
            }
        }
    }

    Ok(files)
}

/// Physical lines: blank lines and comments count, and a trailing newline adds
/// no phantom line. An empty file has zero lines.
pub fn physical_lines(path: &Path) -> Result<u64, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.is_empty() {
        return Ok(0);
    }
    let mut lines = bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
    if !bytes.ends_with(b"\n") {
        lines += 1;
    }
    Ok(lines)
}

pub fn relative_name(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn is_rust(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "rs")
}
