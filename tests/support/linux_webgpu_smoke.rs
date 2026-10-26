use std::path::Path;

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
