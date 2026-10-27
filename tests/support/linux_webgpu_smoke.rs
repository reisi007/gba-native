use std::path::{Path, PathBuf};

pub fn assert_probe_source(root: &Path) {
    let source = std::fs::read_to_string(root.join("scripts/web-linux-gpu-smoke.cjs"))
        .expect("standalone smoke source exists");
    for contract in [
        "page.route(probeUrl",
        "window.isSecureContext",
        "canvas-present.png",
        "deviceLossBeforeTeardown",
        "canvasCreated: false",
        "POST_WORK_WAIT_MS = 7_000",
        "RAF_TIMEOUT_MS = 1_500",
        "function withDeadline",
        "page.goto",
        "page.evaluate(inPageProbe)",
        "page.evaluate(device teardown)",
        "canvas.screenshot",
        "context.close",
        "browser.close",
        "HARNESS_HARD_DEADLINE_MS = 150_000",
        "process.exit(result.processExitStatus)",
        "browserTerminationFallback",
        "MAX_UNCAPTURED_ERRORS = 100",
        "ERROR_TEXT_LIMIT = 2_000",
        "uncapturedErrorsTruncated: false",
        "report.uncapturedErrors.length >= MAX_UNCAPTURED_ERRORS",
        "report.uncapturedErrorsTruncated = true",
        "safeText(error?.name || 'GPUUncapturedError').slice(0, ERROR_TEXT_LIMIT)",
        "safeText(error?.message || error).slice(0, ERROR_TEXT_LIMIT)",
    ] {
        assert!(
            source.contains(contract),
            "missing probe contract: {contract}"
        );
    }
    for app_contract in [
        "APP_PATH = '/?autostart=1&backend=webgpu'",
        "async function inPageAppProbe",
        "page.evaluate(inPageAppProbe)",
        "APP_STARTUP_WAIT_MS = 30_000",
        "APP_POST_WORK_WAIT_MS = 7_000",
        "state === 'empty'",
        "const settled = ['ready', 'error', 'lost'].includes(state)",
        "if (html !== null)",
        "document.body?.dataset?.appState",
        "window.__gbaNative",
        "statusHistory",
        "deviceLossObserved",
        "if (page && MODE !== 'app')",
    ] {
        assert!(
            source.contains(app_contract),
            "missing app probe contract: {app_contract}"
        );
    }
}

pub fn assert_output(calls: &Path, diagnostics: &Path, a_args: &str) {
    let lines: Vec<_> = std::fs::read_to_string(calls.with_extension("smokes.log"))
        .expect("both standalone smoke probes ran")
        .lines()
        .map(str::to_owned)
        .collect();
    assert!(
        lines.len() >= 2,
        "both standalone probes must run: {lines:?}"
    );
    for (line, mode) in lines[..2].iter().zip(["canvas", "offscreen"]) {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields[0], mode);
        assert_eq!(fields[1], a_args, "smoke must use exact A arguments");
        assert!(
            fields[2].ends_with(&format!("/smoke-{mode}")),
            "{}",
            fields[2]
        );
    }
    for line in &lines[2..] {
        assert!(
            line.contains("/matrix-"),
            "non-gating matrix probes must stay in their own directories: {line}"
        );
    }

    let summary = std::fs::read_to_string(diagnostics.join("summary.txt")).expect("read summary");
    assert!(summary.contains("smoke-canvas=19\nsmoke-offscreen=0\nA=17\nB=0\n"));
    assert!(summary.contains("A-gated=true\nB-non-gating=true\n"));
    assert!(summary.contains("SharedImageBackingFactoryFailures=not-observed\n"));
    for mode in ["canvas", "offscreen"] {
        let smoke = diagnostics.join(format!("smoke-{mode}"));
        for artifact in [
            "playwright.log",
            "launch-args.txt",
            "result.json",
            "exit-status.txt",
        ] {
            assert!(smoke.join(artifact).is_file(), "missing {mode}/{artifact}");
        }
        assert!(
            !smoke.join("test-results").exists(),
            "Playwright must not own smoke output"
        );
    }
}

/// Install a deterministic `xvfb-run` stand-in for the Linux CI fixtures. It
/// records the wrapper call, ignores the real Xvfb options, exports a display and
/// then executes the E2E script like the distribution wrapper.
pub fn install_xvfb_stub(bin: &Path) {
    super::executable(
        &bin.join("xvfb-run"),
        r#"#!/bin/sh
printf '%s\n' "$*" >>xvfb.log
while [ "$#" -gt 0 ]; do
  case "$1" in
    -a) shift ;;
    -s) shift 2 ;;
    *) break ;;
  esac
done
DISPLAY=:99
export DISPLAY
exec "$@"
"#,
    );
}

/// Build a PATH directory that has the two commands the wrapper preflight needs
/// (`uname`, `dirname`) but deliberately no `xvfb-run`.
fn install_path_without_xvfb(root: &Path) -> PathBuf {
    let bin = root.join("no-xvfb-bin");
    std::fs::create_dir_all(&bin).expect("create minimal PATH directory");
    super::executable(&bin.join("uname"), "#!/bin/sh\nprintf 'Linux\\n'\n");
    let dirname = super::host_program("dirname");
    super::executable(
        &bin.join("dirname"),
        &format!("#!/bin/sh\nexec {} \"$@\"\n", dirname.display()),
    );
    bin
}

/// Run the E2E script with an explicit mode/CI state instead of the A/B fixture
/// defaults used by `run_linux_ab_fixture`.
fn run_e2e(root: &Path, path: &str, mode: &str, ci: bool, calls: &Path) -> std::process::Output {
    let mut command = std::process::Command::new(super::host_program("bash"));
    command
        .arg(root.join("scripts/web-e2e.sh"))
        .arg(mode)
        .current_dir(root)
        .env("PATH", path)
        .env("HOME", root.join("home"))
        .env_remove("DISPLAY")
        .env_remove("CI")
        .env_remove("WEB_LINUX_AB_DIAGNOSTICS")
        .env("AB_CALL_LOG", calls)
        .env("SMOKE_CALL_LOG", calls.with_extension("smokes.log"));
    if ci {
        command.env("CI", "true");
    }
    command.output().expect("run the browser E2E fixture")
}

fn read_or_empty(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn fixture_path_with_host(bin: &Path) -> String {
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

/// The Linux gate must stay fail-closed on the candidate display wrapper, keep
/// the two standalone probes and Variant A gating while Variant B still runs as
/// a recorded non-gating diagnostic, and keep its documentation accurate.
pub fn assert_display_contract(root: &Path) {
    let script = std::fs::read_to_string(root.join("scripts/web-e2e.sh"))
        .expect("read the functional browser E2E script");
    let preset = std::fs::read_to_string(root.join("scripts/web-gpu-launch-args.sh"))
        .expect("read the shared launch preset");
    let docs = std::fs::read_to_string(root.join("docs/web-frontend.md"))
        .expect("read the browser frontend documentation");

    assert!(
        preset.contains("candidate configuration under test"),
        "the preset must present its Linux flags as a candidate under test"
    );
    assert!(
        preset.contains("ANGLE's SwiftShader backend"),
        "the preset must name ANGLE's SwiftShader backend"
    );
    assert!(
        !preset
            .lines()
            .any(|line| line.trim() == super::linux_gpu_matrix::SURFACE_FLAG),
        "the promoted preset must not disable the Vulkan surface"
    );
    for forbidden in ["--use-gl=", "--enable-gpu"] {
        assert!(
            !preset.contains(forbidden),
            "the preset must not carry {forbidden}"
        );
    }
    assert!(
        !script.contains("--use-angle=opengl"),
        "`opengl` is not a valid --use-angle value and must not return as a control"
    );

    let mode_at = script
        .find("MODE=\"${1:-local}\"")
        .expect("the E2E parses its mode argument");
    let wrapper_at = script
        .find("command -v xvfb-run")
        .expect("the E2E must check for xvfb-run");
    assert!(
        mode_at < wrapper_at,
        "the mode must be parsed and validated before the wrapper check"
    );
    for contract in [
        "[ \"$MODE\" = ci ] && [ \"$(uname -s)\" = Linux ] && [ \"${CI:-}\" = true ] &&",
        "fail \"Linux CI without DISPLAY requires xvfb-run",
        "exec xvfb-run -a -s \"-screen 0 1280x1024x24\" bash \"$ROOT/scripts/web-e2e.sh\" \"$@\"",
        "WEB_E2E_XVFB=1",
        "xvfb-run-wrapper=%s",
        "candidate configuration under test",
        "A-gated=true\\nB-non-gating=true\\n",
        "B_STATUS (variant B diagnostic, non-gating)",
    ] {
        assert!(
            script.contains(contract),
            "missing display contract: {contract}"
        );
    }

    // The final failure condition gates both standalone probes and Variant A,
    // but must never include Variant B.
    let gate_start = script
        .find("if [ \"$SMOKE_CANVAS_STATUS\" -ne 0 ]")
        .expect("the final gate condition starts with the canvas probe");
    let gate_tail = &script[gate_start..];
    let gate_condition = &gate_tail[..gate_tail
        .find("]; then")
        .expect("the final gate condition ends with then")
        + "]; then".len()];
    for gated in ["SMOKE_CANVAS_STATUS", "SMOKE_OFFSCREEN_STATUS", "A_STATUS"] {
        assert!(
            gate_condition.contains(gated),
            "the final gate must include {gated}: {gate_condition}"
        );
    }
    assert!(
        !gate_condition.contains("B_STATUS"),
        "the final gate must exclude the Variant B diagnostic: {gate_condition}"
    );

    for wording in [
        "candidate configuration",
        "Variant A and the two standalone probes gate",
        "Variant B continues to execute",
        "non-gating diagnostic",
        "xvfb-run-wrapper=1",
        "reference preset",
        "The screenshot runner (`scripts/web-screenshots.sh`) is not wrapped",
    ] {
        assert!(docs.contains(wording), "documentation missing: {wording}");
    }
    for wording in [
        "both variants gate the job",
        "both full suites gate",
        "Variant B keeps gating",
        "remains gated as new B",
        "Both variants still have to pass",
        "non-gating comparison",
        "disables GL",
        "Xvfb solves",
        "--enable-gpu",
        "GLES driver",
    ] {
        assert!(
            !docs.contains(wording),
            "documentation must not claim: {wording}"
        );
    }

    // Linux CI without DISPLAY re-executes through the wrapper, keeps the
    // probes and A gated, and records only that the wrapper was used.
    let (_temp, fixture_root, bin, calls) = super::linux_ab_fixture();
    let launch_args = super::linux_gpu_matrix::BASE_ARGS;
    let output = super::run_linux_ab_fixture(&fixture_root, &bin, &calls, launch_args, None);
    assert!(
        output.status.success(),
        "the display-backed gate must run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let xvfb = std::fs::read_to_string(fixture_root.join("xvfb.log"))
        .expect("the E2E re-ran under xvfb-run");
    assert!(xvfb.contains("web-e2e.sh ci"), "{xvfb}");
    let diagnostics = super::linux_gpu_matrix::newest_diagnostics_dir(&fixture_root);
    let summary = std::fs::read_to_string(diagnostics.join("summary.txt")).expect("read summary");
    assert!(summary.contains("xvfb-run-wrapper=1\n"), "{summary}");
    assert!(summary.contains("A=0\nB=0\n"), "{summary}");
    assert!(
        summary.contains("A-gated=true\nB-non-gating=true\n"),
        "the summary must keep A gated and B non-gating: {summary}"
    );
    for variant in ["variant-a", "variant-b"] {
        let dir = diagnostics.join(variant);
        for artifact in ["launch-args.txt", "playwright.log", "exit-status.txt"] {
            assert!(dir.join(artifact).is_file(), "missing {variant}/{artifact}");
        }
        assert!(
            dir.join("test-results").is_dir(),
            "missing {variant}/test-results"
        );
    }
    assert_eq!(
        std::fs::read_to_string(diagnostics.join("variant-b/exit-status.txt"))
            .expect("read variant B status")
            .trim_end(),
        "0"
    );

    // A local Linux run without CI must not be wrapped.
    let (_local_temp, local_root, local_bin, local_calls) = super::linux_ab_fixture();
    let output = run_e2e(
        &local_root,
        &fixture_path_with_host(&local_bin),
        "local",
        false,
        &local_calls,
    );
    assert!(
        output.status.success(),
        "a local run must still work: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !local_root.join("xvfb.log").exists(),
        "local Linux runs must keep the previous no-wrapper behaviour"
    );
    assert_eq!(
        read_or_empty(&local_calls).lines().count(),
        1,
        "the local run executes the plain E2E once"
    );

    // Linux CI without DISPLAY and without xvfb-run fails before any browser
    // probe is launched.
    let (_missing_temp, missing_root, _missing_bin, missing_calls) = super::linux_ab_fixture();
    let minimal = install_path_without_xvfb(&missing_root);
    let output = run_e2e(
        &missing_root,
        minimal.to_str().expect("UTF-8 PATH"),
        "ci",
        true,
        &missing_calls,
    );
    assert!(
        !output.status.success(),
        "a missing xvfb-run must fail the Linux CI gate"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires xvfb-run"),
        "expected the fail-closed diagnostic: {stderr}"
    );
    assert!(
        read_or_empty(&missing_calls).is_empty(),
        "no A/B browser run may start without the wrapper"
    );
    assert!(
        read_or_empty(&missing_calls.with_extension("smokes.log")).is_empty(),
        "no standalone probe may start without the wrapper"
    );
    let diagnostics: Vec<_> = std::fs::read_dir(missing_root.join("output"))
        .expect("read fixture output")
        .map(|entry| entry.expect("read output entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("web-linux-"))
        })
        .collect();
    assert!(
        diagnostics.is_empty(),
        "the fail-closed preflight must not create diagnostics: {diagnostics:?}"
    );
}
