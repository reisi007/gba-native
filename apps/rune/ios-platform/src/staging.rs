use std::io::{Read, Take};
use std::path::Path;

/// Match the shared Rune SaveStore's maximum incoming save size.
pub const MAX_IMPORT_BYTES: usize = 128 * 1024;

/// Process-local proof that a staged path belongs to a completed picker request.
#[derive(Default)]
pub(crate) struct ImportLifecycle {
    active: bool,
    result: Option<Result<(), String>>,
}

impl ImportLifecycle {
    /// Start a new request only after removing any stage left by an old launch.
    pub(crate) fn begin(&mut self, staging_path: &Path) -> Result<(), String> {
        if self.active {
            return Err("the save document picker is already open".to_owned());
        }
        clear_staging(staging_path)?;
        self.result = None;
        self.active = true;
        Ok(())
    }

    pub(crate) fn complete(&mut self, result: Result<(), String>) {
        self.active = false;
        self.result = Some(result);
    }

    /// Only a fresh asynchronous picker completion can authorize consumption.
    pub(crate) fn take_result(&mut self) -> Option<Result<(), String>> {
        self.result.take()
    }
}

pub(crate) fn clear_staging(staging_path: &Path) -> Result<(), String> {
    match std::fs::remove_file(staging_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "cannot clear stale staged save {}: {error}",
            staging_path.display()
        )),
    }
}

/// Validate the selected filename and stage at most the import limit.
pub(crate) fn stage_save<R: Read>(
    source: R,
    selected_path: &Path,
    staging_path: &Path,
) -> Result<usize, String> {
    if !is_sav_file(selected_path) {
        return Err("select a .sav file".to_owned());
    }

    let mut bytes = Vec::with_capacity(MAX_IMPORT_BYTES + 1);
    let mut bounded: Take<R> = source.take((MAX_IMPORT_BYTES + 1) as u64);
    bounded
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read selected save: {error}"))?;
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err(format!("save image exceeds {MAX_IMPORT_BYTES} bytes"));
    }

    if let Some(parent) = staging_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create save staging folder: {error}"))?;
    }
    std::fs::write(staging_path, &bytes)
        .map_err(|error| format!("cannot stage selected save: {error}"))?;
    Ok(bytes.len())
}

fn is_sav_file(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sav"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rune-ios-staging-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    struct CountedReader<'a> {
        bytes: Cursor<Vec<u8>>,
        read: &'a AtomicUsize,
    }

    impl Read for CountedReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = self.bytes.read(buffer)?;
            self.read.fetch_add(count, Ordering::Relaxed);
            Ok(count)
        }
    }

    #[test]
    fn stages_only_sav_files_and_accepts_the_inclusive_size_limit() {
        let root = temp_path("accepted");
        let destination = root.join("saves/import.sav");
        let bytes = vec![0x5a; MAX_IMPORT_BYTES];
        assert_eq!(
            stage_save(Cursor::new(&bytes), Path::new("fixture.SAV"), &destination).unwrap(),
            MAX_IMPORT_BYTES
        );
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        let rejected_destination = root.join("other/import.sav");
        assert!(
            stage_save(
                Cursor::new([1, 2, 3]),
                Path::new("fixture.gba"),
                &rejected_destination
            )
            .is_err()
        );
        assert!(!rejected_destination.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn oversized_selection_reads_only_the_rejection_byte_and_does_not_stage() {
        let root = temp_path("oversized");
        let destination = root.join("saves/import.sav");
        let read = AtomicUsize::new(0);
        let source = CountedReader {
            bytes: Cursor::new(vec![0x33; MAX_IMPORT_BYTES + 64]),
            read: &read,
        };

        assert!(
            stage_save(source, Path::new("fixture.sav"), &destination)
                .unwrap_err()
                .contains("exceeds")
        );
        assert_eq!(read.load(Ordering::Relaxed), MAX_IMPORT_BYTES + 1);
        assert!(!destination.exists());
    }

    #[test]
    fn stale_stage_is_cleared_and_requires_a_fresh_successful_picker_result() {
        let root = temp_path("fresh-result");
        let destination = root.join("saves/import.sav");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(&destination, b"stale stage").unwrap();
        let mut lifecycle = ImportLifecycle::default();

        assert!(lifecycle.take_result().is_none());
        lifecycle.begin(&destination).unwrap();
        assert!(!destination.exists());
        assert!(lifecycle.take_result().is_none());

        stage_save(
            Cursor::new(b"fresh selection"),
            Path::new("fresh.sav"),
            &destination,
        )
        .unwrap();
        lifecycle.complete(Ok(()));
        assert_eq!(lifecycle.take_result(), Some(Ok(())));
        assert_eq!(std::fs::read(&destination).unwrap(), b"fresh selection");
        assert!(lifecycle.take_result().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelled_or_failed_picker_results_do_not_authorize_staged_data() {
        let root = temp_path("failed-result");
        let destination = root.join("saves/import.sav");
        let mut lifecycle = ImportLifecycle::default();
        lifecycle.begin(&destination).unwrap();
        lifecycle.complete(Err("save import cancelled".to_owned()));

        assert_eq!(
            lifecycle.take_result(),
            Some(Err("save import cancelled".to_owned()))
        );
        assert!(lifecycle.take_result().is_none());
        assert!(!destination.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
