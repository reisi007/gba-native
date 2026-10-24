//! Safe writes and fresh workspaces under `<cwd>/output/`.
//!
//! Contract: the CLI creates or writes only paths that are lexically inside
//! `<cwd>/output/` after resolving `.` and `..`, including absolute paths.
//! Existing components may not be symbolic links. Generated directories and
//! files use `create_dir`/`create_new`, so pre-existing files, directories or
//! links are refused instead of reused or followed. This is an ownership check,
//! not a claim of race-freedom against an actively hostile concurrent process
//! mutating the filesystem between checks.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gba_native::error::{Error, Result};

static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);

pub(super) fn write_safely(path: &Path, bytes: &[u8]) -> Result<()> {
    let root = output_root()?;
    write_safely_under(&root, path, bytes)
}

/// Create an explicitly requested `--out` directory. The final path must not
/// exist; its parents may be existing real directories under `output/`.
pub(super) fn create_fresh_directory(path: &Path) -> Result<PathBuf> {
    let root = output_root()?;
    create_fresh_directory_under(&root, path)
}

/// Allocate a new, exclusively created workspace directly under `output/`.
/// Callers never reuse a previous run's directory.
pub(super) fn create_unique_workspace(prefix: &str) -> Result<PathBuf> {
    let root = prepare_output_root(&output_root()?)?;
    for attempt in 0..64 {
        let sequence = NEXT_WORKSPACE.fetch_add(1, Ordering::SeqCst);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let name = format!(
            "{prefix}-{}-{nanos}-{sequence}-{attempt}",
            std::process::id()
        );
        let candidate = root.join(name);
        match std::fs::create_dir(&candidate) {
            Ok(()) => {
                verify_real_directory(&candidate)?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(Error::Io(format!(
        "could not allocate a fresh workspace under {}",
        root.display()
    )))
}

fn output_root() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join("output"))
}

fn write_safely_under(root: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    // Validate the requested location before creating anything, so a rejected
    // path leaves no directory behind outside the allowed root.
    let (root, target) = validated_target(root, path)?;
    prepare_output_root(&root)?;
    create_parent_components(&root, &target)?;

    match std::fs::symlink_metadata(&target) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(redirect_error(&target));
            }
            if metadata.is_dir() {
                return Err(Error::Io(format!(
                    "refusing to replace directory {} with a file",
                    target.display()
                )));
            }
            // Existing regular files inside output/ may be replaced.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&target)?;
            file.write_all(bytes)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
            {
                Ok(mut file) => {
                    file.write_all(bytes)?;
                    Ok(())
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Re-check without following a link that may have appeared.
                    let metadata = std::fs::symlink_metadata(&target)?;
                    if metadata.file_type().is_symlink() || metadata.is_dir() {
                        return Err(redirect_error(&target));
                    }
                    std::fs::write(&target, bytes)?;
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn create_fresh_directory_under(root: &Path, path: &Path) -> Result<PathBuf> {
    let (root, target) = validated_target(root, path)?;
    prepare_output_root(&root)?;
    create_parent_components(&root, &target)?;
    match std::fs::create_dir(&target) {
        Ok(()) => {
            verify_real_directory(&target)?;
            Ok(target)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(Error::Io(format!(
            "refusing to reuse existing directory {}; pass a new --out path or omit --out",
            target.display()
        ))),
        Err(error) => Err(error.into()),
    }
}

fn prepare_output_root(root: &Path) -> Result<PathBuf> {
    let root = absolute(root)?;
    match std::fs::symlink_metadata(&root) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(redirect_error(&root));
            }
            if !metadata.is_dir() {
                return Err(Error::Io(format!(
                    "{} exists and is not a directory",
                    root.display()
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&root)?;
            verify_real_directory(&root)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(root)
}

fn verify_real_directory(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(redirect_error(path));
    }
    Ok(())
}

/// Create the still-missing parent components one level at a time with
/// `create_dir`, refusing symlinks and non-directories on the way.
fn create_parent_components(root: &Path, target: &Path) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| Error::Io(format!("{} has no parent", target.display())))?;
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| outside_error(target))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(outside_error(target));
        };
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(redirect_error(&current));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)?;
                verify_real_directory(&current)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Resolve `.`/`..` lexically and require the result to be strictly below the
/// normalized root. The root itself is not a writable target.
///
/// The resolved forms (longest existing ancestor canonicalized) are used to
/// reject links that lead outside and to accept an absolute path spelled
/// through a system-level alias such as `/var` vs `/private/var`. The name of
/// every component below the root is preserved for the later symlink walk, so
/// a link inside `output/` is still refused rather than silently resolved.
///
/// Returns the canonical root and the target to operate on.
fn validated_target(root: &Path, path: &Path) -> Result<(PathBuf, PathBuf)> {
    let lexical_root = absolute(root)?;
    if std::fs::symlink_metadata(&lexical_root)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(redirect_error(&lexical_root));
    }
    let lexical_target = absolute(path)?;
    let real_root = resolve_with_existing_ancestors(&lexical_root)?;
    let real_target = resolve_with_existing_ancestors(&lexical_target)?;
    if real_target == real_root || !real_target.starts_with(&real_root) {
        let lexical_inside =
            lexical_target != lexical_root && lexical_target.starts_with(&lexical_root);
        return Err(if lexical_inside {
            redirect_error(path)
        } else {
            outside_error(path)
        });
    }
    let target = match lexical_target.strip_prefix(&lexical_root) {
        Ok(relative) => real_root.join(relative),
        Err(_) => real_target,
    };
    Ok((real_root, target))
}

/// Canonicalize the longest existing prefix and append the still-missing
/// components literally. The input must already be absolute and normalized.
fn resolve_with_existing_ancestors(path: &Path) -> Result<PathBuf> {
    let mut probe = path;
    let mut missing: Vec<OsString> = Vec::new();
    loop {
        match std::fs::canonicalize(probe) {
            Ok(mut real) => {
                for component in missing.iter().rev() {
                    real.push(component);
                }
                return Ok(real);
            }
            Err(_) => {
                let name = probe
                    .file_name()
                    .ok_or_else(|| Error::Io(format!("cannot resolve {}", path.display())))?;
                missing.push(name.to_os_string());
                probe = probe
                    .parent()
                    .ok_or_else(|| Error::Io(format!("cannot resolve {}", path.display())))?;
            }
        }
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    normalize_lexically(&joined)
        .ok_or_else(|| Error::Io(format!("cannot normalize path {}", path.display())))
}

fn outside_error(path: &Path) -> Error {
    Error::Io(format!(
        "refusing to create or overwrite {} outside the ignored output/ directory",
        path.display()
    ))
}

fn redirect_error(path: &Path) -> Error {
    Error::Io(format!(
        "refusing to traverse symbolic link or non-directory {} (output/ paths must be real directories)",
        path.display()
    ))
}

fn normalize_lexically(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "gba-native-path-{tag}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn new_nested_output_directories_are_allowed() {
        let temp = TempDir::new("nested");
        let root = temp.path().join("output");
        let path = root.join("a").join("b").join("demo.ppm");
        write_safely_under(&root, &path, b"ppm").expect("nested write");
        assert_eq!(std::fs::read(&path).expect("read"), b"ppm");

        let fresh = root.join("generated").join("crate");
        let created = create_fresh_directory_under(&root, &fresh).expect("fresh directory");
        assert_eq!(
            std::fs::canonicalize(&created).expect("canonical created"),
            std::fs::canonicalize(&fresh).expect("canonical fresh")
        );
        assert!(fresh.is_dir());
        assert!(
            create_fresh_directory_under(&root, &fresh)
                .expect_err("existing directory must be refused")
                .to_string()
                .contains("existing directory")
        );
    }

    #[test]
    fn paths_outside_output_are_refused_before_creation() {
        let temp = TempDir::new("outside");
        let root = temp.path().join("output");
        let outside = temp.path().join("outside").join("fresh.txt");
        let error = write_safely_under(&root, &outside, b"replace").expect_err("must refuse");
        assert!(format!("{error}").contains("outside"));
        assert!(!outside.exists());
        assert!(!root.exists(), "rejected path must not create output/");

        let directory = temp.path().join("outside").join("fresh-dir");
        let error =
            create_fresh_directory_under(&root, &directory).expect_err("must refuse directory");
        assert!(format!("{error}").contains("outside"));
        assert!(!directory.exists());
        assert!(!root.exists());
    }

    #[test]
    fn existing_files_outside_output_are_not_overwritten() {
        let temp = TempDir::new("keep");
        let root = temp.path().join("output");
        let outside = temp.path().join("keep.txt");
        std::fs::write(&outside, b"keep").expect("seed outside file");
        let error = write_safely_under(&root, &outside, b"replace").expect_err("must refuse");
        assert!(format!("{error}").contains("outside"));
        assert_eq!(std::fs::read(&outside).expect("read"), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_output_subdirectory_cannot_redirect_writes() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new("symlink");
        let root = temp.path().join("output");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).expect("output dir");
        std::fs::create_dir_all(&outside).expect("outside dir");
        symlink(&outside, root.join("link")).expect("symlink");

        let target = root.join("link").join("escaped.ppm");
        let error = write_safely_under(&root, &target, b"escape").expect_err("must refuse");
        assert!(format!("{error}").contains("symbolic link"));
        assert!(!outside.join("escaped.ppm").exists());

        let error = create_fresh_directory_under(&root, &root.join("link").join("crate"))
            .expect_err("must refuse generated dir");
        assert!(format!("{error}").contains("symbolic link"));
        assert!(!outside.join("crate").exists());

        // A dangling final-component symlink has exists() == false. It must not
        // be followed into the outside target either.
        let dangling_target = outside.join("created.ppm");
        symlink(&dangling_target, root.join("dangling")).expect("dangling symlink");
        let error =
            write_safely_under(&root, &root.join("dangling"), b"escape").expect_err("must refuse");
        assert!(format!("{error}").contains("symbolic link"));
        assert!(!dangling_target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_output_root_is_refused() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new("root-link");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).expect("outside dir");
        let root = temp.path().join("output");
        symlink(&outside, &root).expect("root symlink");
        let error =
            write_safely_under(&root, &root.join("demo.ppm"), b"escape").expect_err("must refuse");
        assert!(format!("{error}").contains("symbolic link"));
        assert!(!outside.join("demo.ppm").exists());
    }
}
