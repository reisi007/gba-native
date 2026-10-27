#![cfg(unix)]
//! Failure paths of the documented short/full gate scripts.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "support/linux_gpu_features.rs"]
mod linux_gpu_features;
#[path = "support/linux_gpu_matrix.rs"]
mod linux_gpu_matrix;
#[path = "support/linux_gpu_preflight.rs"]
mod linux_gpu_preflight;
#[path = "support/linux_webgpu_smoke.rs"]
mod linux_webgpu_smoke;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-scripts-{tag}-{}-{sequence}",
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

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn path_with_cargo(extra: &Path) -> String {
    let cargo = Path::new(env!("CARGO"));
    let cargo_dir = cargo.parent().expect("cargo directory");
    format!(
        "{}:{}:{}",
        extra.display(),
        cargo_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn host_program(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").expect("host PATH is set");
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("host PATH has no {name}");
}

fn executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, contents).expect("write fake executable");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn linux_ab_fixture() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let temp = TempDir::new("linux-ab");
    let root = temp.path().to_path_buf();
    let script_dir = root.join("scripts");
    let dist = root.join("output/web-dist");
    let server_dir = root.join("target/web/debug");
    let node_dir = root.join("target/tools/node-pinned/bin");
    let playwright_dir = root.join("target/web-tools/npm/node_modules/@playwright/test");
    let browser_cache = root.join("home/Library/Caches/ms-playwright/chromium-test-revision");
    let apps_web = root.join("apps/web");
    let bin = root.join("bin");
    for directory in [
        &script_dir,
        &dist,
        &server_dir,
        &node_dir,
        &playwright_dir,
        &browser_cache,
        &apps_web,
        &bin,
    ] {
        std::fs::create_dir_all(directory).expect("create web E2E fixture directory");
    }

    let repo = repo_root();
    for script in [
        "lib.sh",
        "web-e2e.sh",
        "web-gpu-launch-args.sh",
        "web-linux-gpu-smoke.cjs",
    ] {
        std::fs::copy(repo.join("scripts").join(script), script_dir.join(script))
            .expect("copy web E2E script");
    }
    for source in [
        "playwright.config.ts",
        "playwright.screenshots.config.ts",
        "tests/functional.spec.ts",
    ] {
        let destination = apps_web.join(source);
        std::fs::create_dir_all(destination.parent().expect("source parent"))
            .expect("create copied source parent");
        std::fs::copy(repo.join("apps/web").join(source), destination)
            .expect("copy Playwright config or suite");
    }
    std::fs::write(dist.join("index.html"), "fixture").expect("write fixture bundle");
    std::fs::write(dist.join("app.css"), "fixture").expect("write fixture CSS");
    std::fs::write(
        playwright_dir.join("cli.js"),
        "// fake Playwright entry point\n",
    )
    .expect("write fake Playwright entry point");

    executable(
        &server_dir.join("gba-native-web-serve"),
        "#!/bin/sh\nprintf 'listening on http://127.0.0.1:43210\\n'\nexec sleep 900\n",
    );
    executable(
        &node_dir.join("node"),
        r##"#!/bin/sh
if [ "${1:-}" = "-e" ]; then
  printf 'test-revision\n'
  exit 0
fi
if [ "${1##*/}" = "web-linux-gpu-smoke.cjs" ]; then
  mode="$2"
  printf '%s\t%s\t%s\n' "$mode" "$WEB_E2E_LAUNCH_ARGS" "$WEBGPU_SMOKE_OUTPUT_DIR" >> "$SMOKE_CALL_LOG"
  printf '{"mode":"%s","ok":true}\n' "$mode" > "$WEBGPU_SMOKE_OUTPUT_DIR/result.json"
  printf 'fake WebGPU smoke %s\n' "$mode"
  if [ "${AB_FAIL_VARIANT:-}" = A ] && [ "$mode" = canvas ]; then exit 19; fi
  if [ "${SMOKE_HARNESS_FAIL_MODE:-}" = "$mode" ]; then rm -f "$WEBGPU_SMOKE_OUTPUT_DIR/result.json"; exit 23; fi
  exit 0
fi
count=0
if [ -f "$AB_CALL_LOG.count" ]; then read count < "$AB_CALL_LOG.count"; fi
count=$((count + 1))
printf '%s\n' "$count" > "$AB_CALL_LOG.count"
printf '%s\t%s\n' "$WEB_E2E_LAUNCH_ARGS" "$WEB_E2E_OUTPUT_DIR" >> "$AB_CALL_LOG"
printf 'fake Playwright run %s\n' "$count"
if [ "${AB_FAIL_VARIANT:-}" = A ] && [ "$count" -eq 1 ]; then exit 17; fi
if [ "${AB_FAIL_VARIANT:-}" = B ] && [ "$count" -eq 2 ]; then exit 18; fi
exit 0
"##,
    );
    executable(&bin.join("uname"), "#!/bin/sh\nprintf 'Linux\\n'\n");
    linux_webgpu_smoke::install_xvfb_stub(&bin);

    let calls = root.join("calls.log");
    (temp, root, bin, calls)
}

fn run_linux_ab_fixture(
    root: &Path,
    bin: &Path,
    calls: &Path,
    launch_args: &str,
    fail_variant: Option<&str>,
) -> Output {
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new(host_program("bash"));
    command
        .arg(root.join("scripts/web-e2e.sh"))
        .arg("ci")
        .current_dir(root)
        .env("PATH", path)
        .env("HOME", root.join("home"))
        .env_remove("DISPLAY")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env("CI", "true")
        .env("WEB_LINUX_AB_DIAGNOSTICS", "1")
        .env("WEB_E2E_LAUNCH_ARGS", launch_args)
        .env("AB_CALL_LOG", calls)
        .env("SMOKE_CALL_LOG", calls.with_extension("smokes.log"));
    if let Some(variant) = fail_variant {
        command.env("AB_FAIL_VARIANT", variant);
    } else {
        command.env_remove("AB_FAIL_VARIANT");
    }
    command.output().expect("run Linux A/B E2E fixture")
}

fn newest_linux_ab_dir(root: &Path) -> PathBuf {
    let mut diagnostics: Vec<_> = std::fs::read_dir(root.join("output"))
        .expect("read fixture output directory")
        .map(|entry| entry.expect("read output entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("web-linux-ab-"))
        })
        .collect();
    diagnostics.sort();
    diagnostics.pop().expect("Linux A/B diagnostics directory")
}

fn assert_linux_ab_diagnostic_contract() {
    linux_webgpu_smoke::assert_probe_source(&repo_root());
    let (temp, root, bin, calls) = linux_ab_fixture();
    let launch_args = linux_gpu_matrix::BASE_ARGS;

    let a_failure = run_linux_ab_fixture(&root, &bin, &calls, launch_args, Some("A"));
    assert!(!a_failure.status.success());
    let output_text = String::from_utf8_lossy(&a_failure.stderr);
    assert!(
        output_text.contains("canvas=19, offscreen=0, A=17, B=0"),
        "{output_text}"
    );
    let first_calls = std::fs::read_to_string(&calls).expect("read both variant calls");
    let first_lines: Vec<_> = first_calls.lines().collect();
    assert_eq!(
        first_lines.len(),
        2,
        "both variants must run: {first_calls}"
    );
    let (a_args, a_output) = first_lines[0].split_once('\t').expect("A call fields");
    let (b_args, b_output) = first_lines[1].split_once('\t').expect("B call fields");
    let omitted = linux_gpu_matrix::SURFACE_FLAG;
    assert!(
        !a_args.split_whitespace().any(|arg| arg == omitted),
        "promoted Variant A must not contain {omitted}"
    );
    assert_eq!(
        b_args
            .split_whitespace()
            .filter(|token| *token == omitted)
            .count(),
        1,
        "B appends exactly one {omitted}"
    );
    assert_eq!(
        b_args,
        format!("{a_args} {omitted}"),
        "B appends {omitted} to the exact A flags"
    );
    for required in [
        "--use-webgpu-adapter=swiftshader",
        "--enable-features=Vulkan",
        "--use-angle=swiftshader",
        "--use-vulkan=swiftshader",
        "--enable-logging=stderr",
        "--log-level=0",
    ] {
        assert!(
            a_args.split_whitespace().any(|arg| arg == required),
            "A missing {required}"
        );
    }
    assert!(a_output.ends_with("variant-a/test-results"), "{a_output}");
    assert!(b_output.ends_with("variant-b/test-results"), "{b_output}");
    let first_dir = newest_linux_ab_dir(&root);
    linux_webgpu_smoke::assert_output(&calls, &first_dir, a_args);
    for variant in ["variant-a", "variant-b"] {
        assert!(first_dir.join(variant).join("playwright.log").is_file());
        assert!(first_dir.join(variant).join("launch-args.txt").is_file());
        assert!(first_dir.join(variant).join("test-results").is_dir());
    }

    linux_gpu_matrix::reset_calls(&calls);
    let b_failure = run_linux_ab_fixture(&root, &bin, &calls, launch_args, Some("B"));
    assert!(
        !b_failure.status.success(),
        "variant B must keep gating the browser result"
    );
    let output_text = String::from_utf8_lossy(&b_failure.stderr);
    assert!(
        output_text.contains("canvas=0, offscreen=0, A=0, B=18"),
        "{output_text}"
    );
    let second_dir = newest_linux_ab_dir(&root);
    assert!(
        std::fs::read_to_string(second_dir.join("summary.txt"))
            .expect("read B-failure summary")
            .contains("smoke-canvas=0\nsmoke-offscreen=0\nA=0\nB=18\n")
    );

    for count in [1usize, 2] {
        linux_gpu_matrix::reset_calls(&calls);
        let mut override_args = format!("{launch_args} {omitted}");
        for _ in 1..count {
            override_args.push(' ');
            override_args.push_str(omitted);
        }
        let invalid = run_linux_ab_fixture(&root, &bin, &calls, &override_args, None);
        assert!(
            !invalid.status.success(),
            "an override with {count} surface tokens must fail closed"
        );
        let output_text = String::from_utf8_lossy(&invalid.stderr);
        assert!(
            output_text.contains(&format!(
                "to contain no --disable-vulkan-surface token (found {count})"
            )),
            "{output_text}"
        );
        linux_gpu_matrix::assert_no_calls(&calls);
    }
    drop(temp);
}

fn fake_ratchet(directory: &Path, version: &str, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = directory.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nprintf 'ratchet {version}\\n'\n"))
        .expect("write fake ratchet");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

fn run(script: &str, path: &str, ratchet: Option<&Path>) -> Output {
    let mut command = Command::new("bash");
    command
        .arg(repo_root().join("scripts").join(script))
        .current_dir(repo_root())
        .env("PATH", path)
        .env_remove("RATCHET_BIN");
    if let Some(ratchet) = ratchet {
        command.env("RATCHET_BIN", ratchet);
    }
    command.output().expect("run gate script")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn short_fails_closed_when_ratchet_is_missing() {
    let temp = TempDir::new("missing-ratchet");
    let missing = temp.path().join("does-not-exist");
    let output = run("short.sh", &path_with_cargo(temp.path()), Some(&missing));
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(message.contains("RATCHET_BIN"), "{message}");
    assert!(message.contains("not an executable"), "{message}");
}

#[test]
fn the_ratchet_version_is_pinned_and_a_mismatch_fails() {
    let temp = TempDir::new("wrong-ratchet");
    let wrong = fake_ratchet(temp.path(), "9.9.9", "ratchet");
    let output = run("short.sh", &path_with_cargo(temp.path()), Some(&wrong));
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(message.contains("expected ratchet 0.5.0"), "{message}");
    assert!(message.contains("9.9.9"), "{message}");
}

#[test]
fn full_fails_closed_when_cargo_is_missing() {
    let output = run("full.sh", "/usr/bin:/bin", None);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("cargo is not on PATH"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn web_bootstrap_self_test_runs_before_clean_checkout_provisioning() {
    let output = Command::new("bash")
        .arg(repo_root().join("tests/bootstrap-web-tools-clean-checkout.sh"))
        .current_dir(repo_root())
        .output()
        .expect("run clean-checkout bootstrap regression");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn browser_gpu_launch_args_and_linux_ab_diagnostics_are_contractual() {
    let root = repo_root();
    let helper = root.join("scripts/web-gpu-launch-args.sh");
    let metadata = std::fs::metadata(&helper).expect("WebGPU launch-args helper exists");
    assert!(metadata.is_file());
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(
        metadata.permissions().mode() & 0o111,
        0,
        "launch-args helper must be executable"
    );

    let common = [
        "--enable-unsafe-webgpu",
        "--use-webgpu-adapter=swiftshader",
        "--enable-dawn-features=allow_unsafe_apis",
        "--disable-dawn-features=use_dxc",
        "--enable-webgpu-developer-features",
        "--use-gpu-in-tests",
        "--enable-accelerated-2d-canvas",
    ];
    let linux = Command::new(&helper)
        .arg("Linux")
        .output()
        .expect("run Linux launch-args contract");
    assert!(linux.status.success());
    let linux_args: Vec<_> = String::from_utf8(linux.stdout)
        .expect("UTF-8 launch args")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        linux_args,
        common
            .into_iter()
            .chain([
                "--enable-features=Vulkan",
                "--use-angle=swiftshader",
                "--use-vulkan=swiftshader",
            ])
            .map(str::to_owned)
            .collect::<Vec<_>>()
    );
    assert!(
        !linux_args
            .iter()
            .any(|arg| arg == linux_gpu_matrix::SURFACE_FLAG),
        "the promoted Linux preset must not contain {}",
        linux_gpu_matrix::SURFACE_FLAG
    );

    for platform in ["Darwin", "MINGW64_NT-10.0"] {
        let output = Command::new(&helper)
            .arg(platform)
            .output()
            .expect("run non-Linux launch-args contract");
        assert!(output.status.success());
        let args: Vec<_> = String::from_utf8(output.stdout)
            .expect("UTF-8 launch args")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        assert_eq!(args, common, "platform {platform}");
    }

    for script in ["web-e2e.sh", "web-screenshots.sh"] {
        let contents = std::fs::read_to_string(root.join("scripts").join(script))
            .expect("read browser launch script");
        assert!(
            contents.contains("$ROOT/scripts/web-gpu-launch-args.sh"),
            "{script} must use the shared platform-aware launch preset"
        );
        assert!(
            contents.contains("${WEB_E2E_LAUNCH_ARGS:-$DEFAULT_LAUNCH_ARGS}"),
            "{script} must preserve non-empty user override semantics"
        );
        assert!(
            contents.contains("WEB_E2E_LAUNCH_ARGS=\"$LAUNCH_ARGS\""),
            "{script} must pass its final launch args to Playwright"
        );
    }

    let workflow =
        std::fs::read_to_string(root.join(".github/workflows/web.yml")).expect("browser workflow");
    let foundation_step = workflow
        .split("- name: Browser foundation gate")
        .nth(1)
        .expect("browser foundation gate step")
        .split("\n      - name:")
        .next()
        .expect("browser foundation gate step body");
    assert!(
        foundation_step.contains("WEB_LINUX_AB_DIAGNOSTICS: \"1\""),
        "only the Web foundation step opts into Linux A/B diagnostics"
    );
    assert!(workflow.contains("actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02"));
    assert!(workflow.contains("name: web-linux-ab-${{ github.run_id }}-${{ github.run_attempt }}"));
    assert!(workflow.contains("path: output/web-linux-ab-*/"));
    assert_linux_ab_diagnostic_contract();
    linux_webgpu_smoke::assert_display_contract(&root);
    linux_gpu_matrix::assert_contract(&repo_root());
    linux_gpu_features::assert_contract(&repo_root());
    linux_gpu_preflight::assert_contract();
}
