//! Contract for the non-gating Linux GPU flag matrix.
//!
//! The matrix compares the real served app against the standalone canvas probe
//! while removing exactly one Vulkan/ANGLE control token from the documented
//! reference preset at a time. The reference preset is exactly Variant B, i.e.
//! the earlier surface-on Variant A. It is diagnostics-only: statuses are
//! recorded but never gate.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::{host_program, linux_ab_fixture};

pub const SURFACE_FLAG: &str = "--disable-vulkan-surface";

/// Promoted Variant A: the product preset without `--disable-vulkan-surface`.
pub const BASE_ARGS: &str = concat!(
    "--enable-unsafe-webgpu --use-webgpu-adapter=swiftshader ",
    "--enable-dawn-features=allow_unsafe_apis --disable-dawn-features=use_dxc ",
    "--enable-webgpu-developer-features --use-gpu-in-tests ",
    "--enable-accelerated-2d-canvas --enable-features=Vulkan ",
    "--use-angle=swiftshader --use-vulkan=swiftshader"
);

/// Effective flags of a preset, including the diagnostics-only logging tokens.
pub fn effective_args(base: &str) -> String {
    format!("{base} --enable-logging=stderr --log-level=0")
}

/// Omission control and the directory label `scripts/web-e2e.sh` derives.
const OMISSIONS: [(&str, &str); 4] = [
    ("--enable-features=Vulkan", "omit-enable-features-vulkan"),
    ("--use-angle=swiftshader", "omit-use-angle-swiftshader"),
    ("--use-vulkan=swiftshader", "omit-use-vulkan-swiftshader"),
    (SURFACE_FLAG, "omit-disable-vulkan-surface"),
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

/// Invalid Variant A flags must fail before any browser or standalone probe.
pub(super) fn assert_no_calls(calls: &Path) {
    assert!(
        std::fs::read_to_string(calls).map_or(true, |log| log.is_empty()),
        "invalid A/B flags launch no probes"
    );
    let smoke_log = calls.with_extension("smokes.log");
    assert!(std::fs::read_to_string(smoke_log).map_or(true, |log| log.is_empty()));
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
    assert!(removed, "the reference flags must contain {omitted}");
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
    assert!(
        run_info.contains("preset=reference-surface-on"),
        "the matrix must name its reference preset: {run_info}"
    );
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
    let a_tokens: Vec<String> = a_args.split_whitespace().map(str::to_owned).collect();
    let reference_tokens: Vec<String> = b_args.split_whitespace().map(str::to_owned).collect();
    assert_eq!(
        a_args,
        effective_args(BASE_ARGS),
        "Variant A is the promoted preset plus diagnostics logging"
    );
    assert_eq!(
        a_tokens.iter().filter(|t| *t == SURFACE_FLAG).count(),
        0,
        "promoted Variant A must not contain {SURFACE_FLAG}"
    );
    assert_eq!(
        b_args,
        format!("{a_args} {SURFACE_FLAG}"),
        "Variant B appends exactly one {SURFACE_FLAG} to A"
    );
    assert_eq!(
        reference_tokens
            .iter()
            .filter(|t| *t == SURFACE_FLAG)
            .count(),
        1,
        "the reference preset contains exactly one {SURFACE_FLAG}"
    );
    assert_eq!(reference_tokens.len(), a_tokens.len() + 1);
    assert!(a_output.ends_with("variant-a/test-results"), "{a_output}");
    assert!(b_output.ends_with("variant-b/test-results"), "{b_output}");

    let diagnostics = diagnostics_dir(&calls);
    let smokes = smoke_lines(&calls);
    assert_eq!(
        smokes.len(),
        13,
        "two standalone probes, one reference baseline, eight omission probes and two feature-combination probes: {smokes:?}"
    );
    for (probe, mode) in smokes[..2].iter().zip(["canvas", "offscreen"]) {
        assert_eq!(probe.0, mode);
        assert_eq!(probe.1, a_args, "the original smoke keeps exact A flags");
        assert!(
            !probe.1.contains(SURFACE_FLAG),
            "the gated standalone probe must use the promoted Variant A"
        );
        assert!(probe.2.ends_with(&format!("/smoke-{mode}")), "{}", probe.2);
    }

    let mut expected: Vec<(&str, &str, &str, Vec<String>)> =
        vec![("baseline-reference", "app", "-", reference_tokens.clone())];
    for (omitted, label) in OMISSIONS {
        let variant = omit_first(&reference_tokens, omitted);
        assert_eq!(
            variant.len() + 1,
            reference_tokens.len(),
            "{label} must omit exactly one token"
        );
        expected.push((label, "canvas", omitted, variant.clone()));
        expected.push((label, "app", omitted, variant));
    }
    let surface_row = omit_first(&reference_tokens, SURFACE_FLAG).join(" ");
    assert_eq!(
        surface_row, a_args,
        "the surface omission row reproduces the promoted Variant A exactly"
    );
    for (probe, (label, mode, omitted, tokens)) in smokes[2..].iter().zip(expected.iter()) {
        let args = tokens.join(" ");
        assert_eq!(probe.0, *mode, "{label}");
        assert_eq!(
            probe.1, args,
            "matrix {label}/{mode} keeps every other reference token"
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
        matrix.contains(
            "omit-disable-vulkan-surface/canvas omitted=--disable-vulkan-surface exit=0 outcome=completed nonGating=true"
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
    let baseline = diagnostics.join("matrix-baseline-reference-app");
    assert!(baseline.join("playwright.log").is_file());
    assert!(!baseline.join("result.json").exists());
    assert_eq!(read(&baseline.join("exit-status.txt")).trim_end(), "23");
    let status = read(&baseline.join("process-status.json"));
    assert!(
        status.contains("\"outcome\":\"harness-failure\""),
        "{status}"
    );
    assert!(status.contains("\"resultWritten\":false"), "{status}");
    assert!(read(&diagnostics.join("summary.txt")).contains("matrix-baseline-reference-app=23\n"));
}
