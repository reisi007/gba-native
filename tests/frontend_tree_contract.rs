#![cfg(unix)]
//! The optional frontend gate must verify the resolved dependency tree, not
//! only the manifest text. These tests use a fake Cargo that emits controlled
//! trees, so the contract stays hermetic and never fetches GPU dependencies.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FAKE_CARGO: &str = "\
#!/bin/sh
case \" $* \" in
  *\" tree \"*)
    printf 'gba-native-desktop v0.1.0 (/fake/apps/desktop)\\n'
    case \"${FRONTEND_TEST_TREE:-core}\" in
      core) printf '|-- gba-native-core v0.1.0 (/fake/core)\\n' ;;
      aot)
        printf '|-- gba-native-core v0.1.0 (/fake/core)\\n'
        printf '|-- gba-native-aot v0.1.0 (/fake/aot)\\n'
        ;;
      root)
        printf '|-- gba-native-core v0.1.0 (/fake/core)\\n'
        printf '|-- gba-native v0.1.0 (/fake/root)\\n'
        ;;
      nocore) printf '|-- some-unrelated-runtime v0.1.0 (/fake/other)\\n' ;;
    esac
    ;;
esac
exit 0
";

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "gba-native-frontend-tree-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("canonical repository root")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn frontend_gate_rejects_missing_core_or_host_tooling_in_the_tree() {
    let temp = TempDir::new("contract");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).expect("fake bin");
    let cargo = bin.join("cargo");
    fs::write(&cargo, FAKE_CARGO).expect("write fake cargo");
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).expect("chmod fake cargo");
    let rustc = bin.join("rustc");
    fs::write(&rustc, "#!/bin/sh\nexit 0\n").expect("write fake rustc");
    fs::set_permissions(&rustc, fs::Permissions::from_mode(0o755)).expect("chmod fake rustc");
    let path = format!("{}:/usr/bin:/bin", bin.display());

    let cases = [
        ("aot", "must not depend on host-side gba-native-aot"),
        ("root", "must not depend on the root gba-native"),
        ("nocore", "missing the gba-native-core"),
    ];
    for (tree, expected) in cases {
        let output = Command::new("bash")
            .arg(repo_root().join("scripts/frontend.sh"))
            .current_dir(repo_root())
            .env("PATH", &path)
            .env("FRONTEND_TEST_TREE", tree)
            .env("CARGO_TARGET_DIR", "target/frontend")
            .env_remove("RATCHET_BIN")
            .output()
            .expect("run frontend gate");
        assert!(
            !output.status.success(),
            "tree {tree} must fail the frontend gate\n{}",
            stderr(&output)
        );
        assert!(
            stderr(&output).contains(expected),
            "tree {tree}: expected {expected:?}, got:\n{}",
            stderr(&output)
        );
    }
}
