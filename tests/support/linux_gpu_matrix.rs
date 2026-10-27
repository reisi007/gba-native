//! Contract for the non-gating Linux GPU flag matrix.
//!
//! The matrix compares the real served app against the standalone canvas probe
//! while removing exactly one Vulkan/ANGLE control token from the exact Variant
//! A launch flags at a time. It is diagnostics-only: statuses are recorded but
//! never gate.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::{host_program, linux_ab_fixture};

pub const BASE_ARGS: &str = concat!(
    "--enable-unsafe-webgpu --use-webgpu-adapter=swiftshader ",
    "--enable-dawn-features=allow_unsafe_apis --disable-dawn-features=use_dxc ",
    "--enable-webgpu-developer-features --use-gpu-in-tests ",
    "--enable-accelerated-2d-canvas --enable-features=Vulkan ",
    "--use-angle=swiftshader --use-vulkan=swiftshader --disable-vulkan-surface"
);

/// Omission control and the directory label `scripts/web-e2e.sh` derives.
const OMISSIONS: [(&str, &str); 4] = [
    ("--enable-features=Vulkan", "omit-enable-features-vulkan"),
    ("--use-angle=swiftshader", "omit-use-angle-swiftshader"),
    ("--use-vulkan=swiftshader", "omit-use-vulkan-swiftshader"),
    ("--disable-vulkan-surface", "omit-disable-vulkan-surface"),
];

const MATRIX_ARTIFACTS: [&str; 7] = [
    "launch-args.txt",
    "omitted-flag.txt",
    "run-info.txt",
    "exit-status.txt",
    "process-status.json",
    "playwright.log",
    "result.json",
];

pub(super) fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

pub(super) fn run_fixture(
    root: &Path,
    bin: &Path,
    calls: &Path,
    launch_args: &str,
    harness_fail_mode: Option<&str>,
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
        .env("GITHUB_STEP_SUMMARY", root.join("step-summary.md"))
        .env("CI", "true")
        .env("WEB_LINUX_AB_DIAGNOSTICS", "1")
        .env("WEB_E2E_LAUNCH_ARGS", launch_args)
        .env("AB_CALL_LOG", calls)
        .env("SMOKE_CALL_LOG", calls.with_extension("smokes.log"))
        .env_remove("AB_FAIL_VARIANT");
    if let Some(mode) = harness_fail_mode {
        command.env("SMOKE_HARNESS_FAIL_MODE", mode);
    } else {
        command.env_remove("SMOKE_HARNESS_FAIL_MODE");
    }
    command.output().expect("run Linux GPU matrix fixture")
}

pub(super) fn reset_calls(calls: &Path) {
    std::fs::write(calls, "").expect("reset fixture calls");
    for stale in [
        calls.with_extension("log.count"),
        calls.with_extension("smokes.log"),
    ] {
        if stale.exists() {
            std::fs::remove_file(&stale).expect("reset fixture call log");
        }
    }
}

/// Invalid A/B flags must fail before any browser probe is launched.
pub(super) fn assert_no_calls(calls: &Path) {
    assert!(read(calls).is_empty(), "invalid A/B flags launch no probes");
}

/// The diagnostics directory of a run, taken from the exact A/B output path the
/// script passes to Playwright instead of guessing the newest timestamp.
pub(super) fn diagnostics_dir(calls: &Path) -> PathBuf {
    let log = read(calls);
    let (_, output_dir) = log
        .lines()
        .next()
        .expect("A/B call log has an A row")
        .split_once('\t')
        .expect("A/B call fields");
    Path::new(output_dir)
        .parent()
        .and_then(Path::parent)
        .expect("A/B test-results parent")
        .to_path_buf()
}

pub(super) fn newest_diagnostics_dir(root: &Path) -> PathBuf {
    let mut directories: Vec<_> = std::fs::read_dir(root.join("output"))
        .expect("read fixture output")
        .map(|entry| entry.expect("read fixture output entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("web-linux-ab-"))
        })
        .collect();
    directories.sort();
    directories.pop().expect("Linux diagnostic directory")
}

pub(super) fn smoke_lines(calls: &Path) -> Vec<(String, String, String)> {
    read(&calls.with_extension("smokes.log"))
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            (
                fields.next().expect("probe mode").to_owned(),
                fields.next().expect("probe launch args").to_owned(),
                fields.next().expect("probe output directory").to_owned(),
            )
        })
        .collect()
}

pub(super) fn omit_first(tokens: &[String], omitted: &str) -> Vec<String> {
    let mut remaining = Vec::new();
    let mut removed = false;
    for token in tokens {
        if !removed && token == omitted {
            removed = true;
            continue;
        }
        remaining.push(token.clone());
    }
    assert!(removed, "the Variant A flags must contain {omitted}");
    remaining
}

fn assert_app_source(root: &Path) {
    let source = read(&root.join("scripts/web-linux-gpu-smoke.cjs"));
    for contract in [
        "APP_PATH = '/?autostart=1&backend=webgpu'",
        "async function inPageAppProbe",
        "page.evaluate(inPageAppProbe)",
        "APP_STARTUP_WAIT_MS = 30_000",
        "APP_POST_WORK_WAIT_MS = 7_000",
        "state === 'empty'",
        "const settled = ['ready', 'error', 'lost'].includes(state)",
        "const appOutcomeObserved",
        "snapshot('initial')",
        "snapshot('post-work')",
        "if (html !== null)",
        "if (page && MODE !== 'app')",
        "document.body?.dataset?.appState",
        "window.__gbaNative",
        "deviceLossObserved",
        "errorStateObserved",
    ] {
        assert!(
            source.contains(contract),
            "missing app-mode probe contract: {contract}"
        );
    }
    assert!(
        !source.contains("fallback="),
        "the app probe must not install a fallback"
    );
}

fn assert_completed_matrix_probe(dir: &Path, args: &str, omitted: &str, mode: &str) {
    for artifact in MATRIX_ARTIFACTS {
        assert!(
            dir.join(artifact).is_file(),
            "missing {}/{artifact}",
            dir.display()
        );
    }
    assert!(
        !dir.join("test-results").exists(),
        "matrix probes must not use Playwright's test-results directory"
    );
    assert_eq!(read(&dir.join("launch-args.txt")).trim_end(), args);
    assert_eq!(read(&dir.join("omitted-flag.txt")).trim_end(), omitted);
    assert_eq!(read(&dir.join("exit-status.txt")).trim_end(), "0");
    let entry = if mode == "app" {
        "entry=app:/?autostart=1&backend=webgpu"
    } else {
        "entry=standalone:/__linux_webgpu_smoke__"
    };
    let run_info = read(&dir.join("run-info.txt"));
    assert!(run_info.contains(entry), "{run_info}");
    assert!(run_info.contains("non_gating=true"), "{run_info}");
    assert!(
        !run_info.contains("opengl"),
        "the matrix must not add an opengl control: {run_info}"
    );
    let status = read(&dir.join("process-status.json"));
    assert!(status.contains("\"outcome\":\"completed\""), "{status}");
    assert!(status.contains("\"nonGating\":true"), "{status}");
    assert!(status.contains("\"resultWritten\":true"), "{status}");
    assert!(
        status.contains(&format!("\"omittedFlag\":\"{omitted}\"")),
        "{status}"
    );
    assert!(
        status.contains(&format!("\"launchArgs\":\"{}\"", args)),
        "{status}"
    );
}

pub fn assert_contract(root: &Path) {
    assert_app_source(root);
    let (_temp, fixture_root, bin, calls) = linux_ab_fixture();

    let output = run_fixture(&fixture_root, &bin, &calls, BASE_ARGS, None);
    assert!(
        output.status.success(),
        "matrix diagnostics must not gate a clean run:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ab = read(&calls);
    let ab_lines: Vec<_> = ab.lines().collect();
    assert_eq!(ab_lines.len(), 2, "A and B must still run: {ab}");
    let (a_args, a_output) = ab_lines[0].split_once('\t').expect("A call fields");
    let (b_args, b_output) = ab_lines[1].split_once('\t').expect("B call fields");
    assert_eq!(
        a_args.replace("--use-angle=swiftshader", "--use-angle=vulkan"),
        b_args,
        "the existing A/B keeps changing only the ANGLE token"
    );
    assert!(b_args.contains("--use-angle=vulkan"));
    assert!(!a_args.contains("--use-angle=vulkan"));
    assert!(a_output.ends_with("variant-a/test-results"), "{a_output}");
    assert!(b_output.ends_with("variant-b/test-results"), "{b_output}");

    let diagnostics = diagnostics_dir(&calls);
    let smokes = smoke_lines(&calls);
    assert_eq!(
        smokes.len(),
        13,
        "two standalone probes, one app baseline, eight omission probes and two feature-combination probes: {smokes:?}"
    );
    for (probe, mode) in smokes[..2].iter().zip(["canvas", "offscreen"]) {
        assert_eq!(probe.0, mode);
        assert_eq!(probe.1, a_args, "the original smoke keeps exact A flags");
        assert!(probe.2.ends_with(&format!("/smoke-{mode}")), "{}", probe.2);
    }

    let base_tokens: Vec<String> = a_args.split_whitespace().map(str::to_owned).collect();
    let mut expected: Vec<(&str, &str, &str, Vec<String>)> =
        vec![("baseline", "app", "-", base_tokens.clone())];
    for (omitted, label) in OMISSIONS {
        let variant = omit_first(&base_tokens, omitted);
        assert_eq!(
            variant.len() + 1,
            base_tokens.len(),
            "{label} must omit exactly one token"
        );
        expected.push((label, "canvas", omitted, variant.clone()));
        expected.push((label, "app", omitted, variant));
    }
    for (probe, (label, mode, omitted, tokens)) in smokes[2..].iter().zip(expected.iter()) {
        let args = tokens.join(" ");
        assert_eq!(probe.0, *mode, "{label}");
        assert_eq!(
            probe.1, args,
            "matrix {label}/{mode} keeps every other A token"
        );
        assert!(
            !probe.1.contains("opengl"),
            "matrix {label}/{mode} added opengl"
        );
        let dir = diagnostics.join(format!("matrix-{label}-{mode}"));
        assert_eq!(
            probe.2,
            dir.to_string_lossy().as_ref(),
            "matrix {label}/{mode} directory"
        );
        assert_completed_matrix_probe(&dir, &args, omitted, mode);
    }

    let summary = read(&diagnostics.join("summary.txt"));
    assert!(
        summary.contains("smoke-canvas=0\nsmoke-offscreen=0\nA=0\nB=0\n"),
        "{summary}"
    );
    assert!(summary.contains("matrix-non-gating=true\n"), "{summary}");
    for (label, mode, _, _) in &expected {
        assert!(
            summary.contains(&format!("matrix-{label}-{mode}=0\n")),
            "missing matrix summary row {label}/{mode}"
        );
    }
    let matrix = read(&diagnostics.join("matrix.txt"));
    assert!(
        matrix.contains(
            "omit-use-angle-swiftshader/canvas omitted=--use-angle=swiftshader exit=0 outcome=completed nonGating=true"
        ),
        "{matrix}"
    );
    assert!(
        matrix.lines().all(|line| line.contains("nonGating=true")),
        "{matrix}"
    );
    for variant in ["variant-a", "variant-b"] {
        assert!(diagnostics.join(variant).join("playwright.log").is_file());
    }
    let step = read(&fixture_root.join("step-summary.md"));
    assert!(
        step.contains("### Non-gating GPU flag matrix (diagnostics only)"),
        "{step}"
    );
    assert!(step.contains("**non-gating**"), "{step}");
    for (label, mode, _, _) in &expected {
        assert!(
            step.contains(&format!("| `{label}` | {mode} |")),
            "missing step-summary row {label}/{mode}"
        );
    }

    // A base preset that lacks an omission control is an explicit invalid-input
    // result for that combination; every other combination is still attempted
    // and the run stays non-gating.
    reset_calls(&calls);
    let partial = "--use-angle=swiftshader --use-webgpu-adapter=swiftshader";
    let output = run_fixture(&fixture_root, &bin, &calls, partial, None);
    assert!(
        output.status.success(),
        "invalid omission controls stay non-gating:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = diagnostics_dir(&calls);
    let smokes = smoke_lines(&calls);
    assert_eq!(
        smokes.len(),
        5,
        "two smoke probes, the app baseline and the one formable omission pair: {smokes:?}"
    );
    let partial_args = format!("{partial} --enable-logging=stderr --log-level=0");
    for (label, omitted) in [
        ("omit-enable-features-vulkan", "--enable-features=Vulkan"),
        ("omit-use-vulkan-swiftshader", "--use-vulkan=swiftshader"),
        ("omit-disable-vulkan-surface", "--disable-vulkan-surface"),
    ] {
        for mode in ["canvas", "app"] {
            let dir = diagnostics.join(format!("matrix-{label}-{mode}"));
            let invalid = read(&dir.join("invalid-input.txt"));
            assert!(invalid.contains(omitted), "{invalid}");
            assert_eq!(read(&dir.join("exit-status.txt")).trim_end(), "126");
            assert_eq!(read(&dir.join("launch-args.txt")).trim_end(), partial_args);
            assert!(!dir.join("playwright.log").exists());
            assert!(!dir.join("result.json").exists());
            let status = read(&dir.join("process-status.json"));
            assert!(status.contains("\"outcome\":\"invalid-input\""), "{status}");
            assert!(status.contains("\"resultWritten\":false"), "{status}");
            assert!(status.contains("\"nonGating\":true"), "{status}");
            assert!(
                status.contains(&format!("\"launchArgs\":\"{partial_args}\"")),
                "{status}"
            );
        }
    }
    for label in ["omit-use-angle-swiftshader", "baseline"] {
        for mode in ["canvas", "app"] {
            let dir = diagnostics.join(format!("matrix-{label}-{mode}"));
            if dir.exists() {
                assert!(dir.join("result.json").is_file(), "{}", dir.display());
            }
        }
    }
    let summary = read(&diagnostics.join("summary.txt"));
    assert!(
        summary.contains("matrix-omit-enable-features-vulkan-canvas=126\n"),
        "{summary}"
    );
    assert!(
        summary.contains("matrix-omit-use-angle-swiftshader-canvas=0\n"),
        "{summary}"
    );

    // A probe whose process fails without writing result.json is recorded as an
    // explicit harness failure and still does not change the gating result.
    reset_calls(&calls);
    let output = run_fixture(&fixture_root, &bin, &calls, BASE_ARGS, Some("app"));
    assert!(
        output.status.success(),
        "matrix harness failures are recorded, not gated:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = diagnostics_dir(&calls);
    let baseline = diagnostics.join("matrix-baseline-app");
    assert!(baseline.join("playwright.log").is_file());
    assert!(!baseline.join("result.json").exists());
    assert_eq!(read(&baseline.join("exit-status.txt")).trim_end(), "23");
    let status = read(&baseline.join("process-status.json"));
    assert!(
        status.contains("\"outcome\":\"harness-failure\""),
        "{status}"
    );
    assert!(status.contains("\"resultWritten\":false"), "{status}");
    assert!(read(&diagnostics.join("summary.txt")).contains("matrix-baseline-app=23\n"));

    let without_angle = BASE_ARGS.replace("--use-angle=swiftshader ", "");
    assert_invalid_angle_preflight(&fixture_root, &bin, &calls, &without_angle, 0);
    let duplicate_angle = BASE_ARGS.replace(
        "--use-angle=swiftshader",
        "--use-angle=swiftshader --use-angle=swiftshader",
    );
    assert_invalid_angle_preflight(&fixture_root, &bin, &calls, &duplicate_angle, 2);
}

fn assert_invalid_angle_preflight(
    root: &Path,
    bin: &Path,
    calls: &Path,
    launch_args: &str,
    count: usize,
) {
    reset_calls(calls);
    let output = run_fixture(root, bin, calls, launch_args, None);
    assert!(
        !output.status.success(),
        "the existing A/B preflight still fails"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "exactly one --use-angle=swiftshader token (found {count})"
        )),
        "{stderr}"
    );
    assert!(read(calls).is_empty(), "invalid A/B flags launch no probes");

    let diagnostics = newest_diagnostics_dir(root);
    let expected_args = format!("{launch_args} --enable-logging=stderr --log-level=0");
    let summary = read(&diagnostics.join("summary.txt"));
    assert!(
        summary.contains("matrix-preflight=invalid-input\n"),
        "{summary}"
    );
    assert!(summary.contains("matrix-non-gating=true\n"), "{summary}");
    let matrix = read(&diagnostics.join("matrix.txt"));
    assert_eq!(matrix.lines().count(), 2, "{matrix}");
    for mode in ["canvas", "app"] {
        let dir = diagnostics.join(format!("matrix-omit-use-angle-swiftshader-{mode}"));
        for artifact in [
            "launch-args.txt",
            "omitted-flag.txt",
            "invalid-input.txt",
            "run-info.txt",
            "exit-status.txt",
            "process-status.json",
        ] {
            assert!(dir.join(artifact).is_file(), "missing {mode}/{artifact}");
        }
        assert_eq!(read(&dir.join("launch-args.txt")).trim_end(), expected_args);
        assert!(read(&dir.join("invalid-input.txt")).contains(&format!("{count} times")));
        assert_eq!(read(&dir.join("exit-status.txt")).trim_end(), "126");
        let status = read(&dir.join("process-status.json"));
        assert!(status.contains("\"outcome\":\"invalid-input\""), "{status}");
        assert!(status.contains("\"nonGating\":true"), "{status}");
        let entry = if mode == "app" {
            "app:/?autostart=1&backend=webgpu"
        } else {
            "standalone:/__linux_webgpu_smoke__"
        };
        assert!(status.contains(entry), "{status}");
        assert!(!dir.join("playwright.log").exists());
        assert!(!dir.join("result.json").exists());
    }
}
