//! Minimal, dependency-free static file server for the local browser build.
//!
//! It exists so the gate and the E2E suite serve the bundle with correct
//! `application/wasm` MIME, the narrow CSP and no directory listing. It binds
//! loopback only. Nothing here reads a ROM, save or user file outside the
//! explicitly supplied bundle root.

use std::fmt;
use std::fs;
#[cfg(test)]
use std::io;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

pub const INDEX: &str = "index.html";
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; \
style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; \
base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServeError {
    Forbidden,
    NotFound,
    TooLarge,
    Io(String),
}

impl fmt::Display for ServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Forbidden => formatter.write_str("request path escapes the bundle root"),
            Self::NotFound => formatter.write_str("requested asset does not exist"),
            Self::TooLarge => formatter.write_str("requested asset exceeds the server size cap"),
            Self::Io(message) => write!(formatter, "cannot read asset: {message}"),
        }
    }
}

impl std::error::Error for ServeError {}

#[derive(Debug)]
pub struct Response {
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// Strip the query/fragment, percent-decode and reject traversal.
pub fn normalize_request_path(target: &str) -> Result<PathBuf, ServeError> {
    let path = target
        .split(['?', '#'])
        .next()
        .filter(|path| !path.is_empty())
        .unwrap_or("/");
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\\')
        || path.contains('\0')
    {
        return Err(ServeError::Forbidden);
    }

    let mut decoded = Vec::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            let pair = bytes
                .get(index + 1..index + 3)
                .and_then(|pair| std::str::from_utf8(pair).ok())
                .ok_or(ServeError::Forbidden)?;
            let value = u8::from_str_radix(pair, 16).map_err(|_| ServeError::Forbidden)?;
            if value == 0 || value == b'/' || value == b'\\' {
                return Err(ServeError::Forbidden);
            }
            decoded.push(value);
            index += 3;
        } else {
            decoded.push(byte);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|_| ServeError::Forbidden)?;

    let relative = decoded.trim_start_matches('/');
    if relative.is_empty() {
        return Ok(PathBuf::from(INDEX));
    }
    let mut result = PathBuf::new();
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(':') {
            return Err(ServeError::Forbidden);
        }
        result.push(segment);
    }
    Ok(result)
}

/// Resolve a request inside `root`. The returned path is canonicalised, so a
/// symlink pointing outside the bundle is rejected rather than served.
pub fn resolve_file(root: &Path, target: &str) -> Result<PathBuf, ServeError> {
    let relative = normalize_request_path(target)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|error| ServeError::Io(error.to_string()))?;
    let candidate = canonical_root.join(&relative);
    let canonical = candidate.canonicalize().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ServeError::NotFound
        } else {
            ServeError::Io(error.to_string())
        }
    })?;
    if !canonical.starts_with(&canonical_root) || !canonical.is_file() {
        return Err(ServeError::Forbidden);
    }
    Ok(canonical)
}

pub fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub fn load(root: &Path, target: &str) -> Result<Response, ServeError> {
    let path = resolve_file(root, target)?;
    let metadata = fs::metadata(&path).map_err(|error| ServeError::Io(error.to_string()))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(ServeError::TooLarge);
    }
    let body = fs::read(&path).map_err(|error| ServeError::Io(error.to_string()))?;
    Ok(Response {
        content_type: mime_for(&path),
        body,
    })
}

#[cfg(test)]
struct OwnedTempDir {
    path: PathBuf,
}

#[cfg(test)]
impl OwnedTempDir {
    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
fn create_owned_temp_dir(base: &Path, prefix: &str) -> io::Result<OwnedTempDir> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    create_owned_temp_dir_with(base, prefix, || {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        (time << 64) | u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    })
}

#[cfg(test)]
fn create_owned_temp_dir_with(
    base: &Path,
    prefix: &str,
    mut next_id: impl FnMut() -> u128,
) -> io::Result<OwnedTempDir> {
    for _ in 0..64 {
        let path = base.join(format!("{prefix}-{}-{}", std::process::id(), next_id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(OwnedTempDir { path }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not acquire a unique owned temporary directory after 64 attempts",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn request_paths_are_normalised_and_traversal_is_rejected() {
        assert_eq!(normalize_request_path("/").unwrap(), PathBuf::from(INDEX));
        assert_eq!(
            normalize_request_path("/?backend=webgpu").unwrap(),
            PathBuf::from(INDEX)
        );
        assert_eq!(
            normalize_request_path("/pkg/app.js").unwrap(),
            PathBuf::from("pkg/app.js")
        );
        assert_eq!(
            normalize_request_path("/app%2Ejs").unwrap(),
            PathBuf::from("app.js")
        );
        for forbidden in [
            "/../secret",
            "/pkg/../../secret",
            "//etc/passwd",
            "/a%2Fb",
            "/a%5Cb",
            "/a:b",
            "relative.js",
            "/a\0b",
        ] {
            assert_eq!(
                normalize_request_path(forbidden),
                Err(ServeError::Forbidden),
                "{forbidden}"
            );
        }
    }

    #[test]
    fn mime_types_cover_the_browser_bundle() {
        assert_eq!(
            mime_for(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            mime_for(Path::new("pkg/app.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(mime_for(Path::new("app.css")), "text/css; charset=utf-8");
        assert_eq!(mime_for(Path::new("app.wasm")), "application/wasm");
        assert_eq!(mime_for(Path::new("data.bin")), "application/octet-stream");
    }

    #[test]
    fn resolution_stays_inside_the_root() {
        let owned = create_owned_temp_dir(&std::env::temp_dir(), "gba-web-serve").unwrap();
        let root = owned.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        let mut index = fs::File::create(root.join("index.html")).unwrap();
        index.write_all(b"<html></html>").unwrap();
        fs::File::create(root.join("pkg/app.wasm")).unwrap();

        assert_eq!(
            resolve_file(root, "/").unwrap(),
            root.canonicalize().unwrap().join("index.html")
        );
        assert_eq!(
            load(root, "/pkg/app.wasm").unwrap().content_type,
            "application/wasm"
        );
        assert_eq!(load(root, "/missing.js").unwrap_err(), ServeError::NotFound);
        assert_eq!(
            load(root, "/../index.html").unwrap_err(),
            ServeError::Forbidden
        );
    }

    #[test]
    fn temporary_directory_collision_preserves_foreign_contents_and_retries() {
        let base = create_owned_temp_dir(&std::env::temp_dir(), "gba-web-temp-test").unwrap();
        let foreign = base
            .path()
            .join(format!("collision-{}-42", std::process::id()));
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("sentinel"), b"foreign").unwrap();

        let mut ids = [42_u64, 43_u64].into_iter();
        let owned = create_owned_temp_dir_with(base.path(), "collision", || {
            u128::from(
                ids.next()
                    .expect("the collision retry must use the second id"),
            )
        })
        .unwrap();

        assert_ne!(owned.path(), foreign);
        assert_eq!(fs::read(foreign.join("sentinel")).unwrap(), b"foreign");
        assert!(owned.path().is_dir());
    }
}
