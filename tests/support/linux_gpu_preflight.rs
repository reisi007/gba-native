//! Contracts for the Linux A/B preflight and the matrix's invalid/partial
//! inputs.
//!
//! Effective Variant A must contain no `--disable-vulkan-surface` token; Variant
//! B then appends exactly one. An override that already carries one or two
//! tokens fails closed before any browser or standalone probe, while a custom
//! override without the token follows the same derivation.

use super::linux_ab_fixture;
use super::linux_gpu_matrix::{
    BASE_ARGS, SURFACE_FLAG, assert_no_calls, effective_args, newest_diagnostics_dir, read,
    run_fixture, smoke_lines,
};

const PREFLIGHT_LABEL: &str = "preflight-variant-a";

fn assert_partial_override_controls() {
    let (_temp, root, bin, calls) = linux_ab_fixture();
    let partial = "--use-webgpu-adapter=swiftshader";
    let output = run_fixture(&root, &bin, &calls, partial, None);
    assert!(
        output.status.success(),
        "an override without the surface token must derive A/B and stay non-gating:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let partial_args = effective_args(partial);
    let reference_args = format!("{partial_args} {SURFACE_FLAG}");
    let ab = read(&calls);
    let ab_lines: Vec<_> = ab.lines().collect();
    assert_eq!(ab_lines.len(), 2, "A and B must still run: {ab}");
    let (a_args, _) = ab_lines[0].split_once('\t').expect("A call fields");
    let (b_args, _) = ab_lines[1].split_once('\t').expect("B call fields");
    assert_eq!(a_args, partial_args);
    assert_eq!(
        b_args, reference_args,
        "Variant B appends exactly one {SURFACE_FLAG} to the custom override"
    );

    let diagnostics = newest_diagnostics_dir(&root);
    let smokes = smoke_lines(&calls);
    assert_eq!(
        smokes.len(),
        5,
        "two smoke probes, the reference baseline and the one formable omission pair: {smokes:?}"
    );
    for (label, omitted) in [
        ("omit-enable-features-vulkan", "--enable-features=Vulkan"),
        ("omit-use-angle-swiftshader", "--use-angle=swiftshader"),
        ("omit-use-vulkan-swiftshader", "--use-vulkan=swiftshader"),
    ] {
        for mode in ["canvas", "app"] {
            let dir = diagnostics.join(format!("matrix-{label}-{mode}"));
            let invalid = read(&dir.join("invalid-input.txt"));
            assert!(invalid.contains(omitted), "{invalid}");
            assert!(invalid.contains("reference preset flags"), "{invalid}");
            assert!(invalid.contains("0 times"), "{invalid}");
            assert_eq!(read(&dir.join("exit-status.txt")).trim_end(), "126");
            assert_eq!(
                read(&dir.join("launch-args.txt")).trim_end(),
                reference_args
            );
            assert!(!dir.join("playwright.log").exists());
            assert!(!dir.join("result.json").exists());
            let status = read(&dir.join("process-status.json"));
            assert!(status.contains("\"outcome\":\"invalid-input\""), "{status}");
            assert!(status.contains("\"resultWritten\":false"), "{status}");
            assert!(status.contains("\"nonGating\":true"), "{status}");
            assert!(
                status.contains(&format!("\"launchArgs\":\"{reference_args}\"")),
                "{status}"
            );
        }
    }
    for mode in ["canvas", "app"] {
        let dir = diagnostics.join(format!("matrix-omit-disable-vulkan-surface-{mode}"));
        assert!(dir.join("result.json").is_file(), "{}", dir.display());
        assert_eq!(read(&dir.join("launch-args.txt")).trim_end(), partial_args);
    }
    assert!(
        diagnostics
            .join("matrix-baseline-reference-app")
            .join("result.json")
            .is_file()
    );
    let summary = read(&diagnostics.join("summary.txt"));
    assert!(
        !summary.contains("matrix-preflight"),
        "a valid override records no preflight row: {summary}"
    );
    assert!(
        summary.contains("matrix-omit-enable-features-vulkan-canvas=126\n"),
        "{summary}"
    );
}

fn assert_invalid_variant_a_preflight(launch_args: &str, count: usize) {
    let (_temp, root, bin, calls) = linux_ab_fixture();
    let output = run_fixture(&root, &bin, &calls, launch_args, None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "a Variant A with the surface token must fail closed"
    );
    assert!(
        stderr.contains(&format!(
            "require the effective Variant A flags to contain no {SURFACE_FLAG} token (found {count})"
        )),
        "{stderr}"
    );
    assert_no_calls(&calls);

    let diagnostics = newest_diagnostics_dir(&root);
    let summary = read(&diagnostics.join("summary.txt"));
    assert!(
        summary.contains("matrix-preflight=invalid-input\n"),
        "{summary}"
    );
    assert!(summary.contains("matrix-non-gating=true\n"), "{summary}");
    let matrix = read(&diagnostics.join("matrix.txt"));
    assert_eq!(matrix.lines().count(), 2, "{matrix}");
    for mode in ["canvas", "app"] {
        let dir = diagnostics.join(format!("matrix-{PREFLIGHT_LABEL}-{mode}"));
        for artifact in [
            "launch-args.txt",
            "surface-token-count.txt",
            "invalid-input.txt",
            "run-info.txt",
            "exit-status.txt",
            "process-status.json",
        ] {
            assert!(dir.join(artifact).is_file(), "missing {mode}/{artifact}");
        }
        assert!(
            !dir.join("omitted-flag.txt").exists(),
            "the preflight is not an omission: {}",
            dir.display()
        );
        assert_eq!(
            read(&dir.join("launch-args.txt")).trim_end(),
            effective_args(launch_args),
            "the preflight keeps the exact effective Variant A flags"
        );
        assert_eq!(
            read(&dir.join("surface-token-count.txt")).trim_end(),
            count.to_string()
        );
        let invalid = read(&dir.join("invalid-input.txt"));
        assert!(
            invalid.contains(&format!("appears {count} times")),
            "{invalid}"
        );
        assert_eq!(read(&dir.join("exit-status.txt")).trim_end(), "126");
        let run_info = read(&dir.join("run-info.txt"));
        assert!(
            run_info.contains(&format!("label={PREFLIGHT_LABEL}")),
            "{run_info}"
        );
        assert!(
            run_info.contains("control=variant-a-preflight"),
            "{run_info}"
        );
        assert!(run_info.contains("non_gating=true"), "{run_info}");
        let entry = if mode == "app" {
            "app:/?autostart=1&backend=webgpu"
        } else {
            "standalone:/__linux_webgpu_smoke__"
        };
        assert!(run_info.contains(&format!("entry={entry}")), "{run_info}");
        assert!(run_info.contains("origin=http://127.0.0.1:"), "{run_info}");
        let status = read(&dir.join("process-status.json"));
        assert!(status.contains("\"outcome\":\"invalid-input\""), "{status}");
        assert!(status.contains("\"nonGating\":true"), "{status}");
        assert!(status.contains("\"resultWritten\":false"), "{status}");
        assert!(
            status.contains(&format!("\"entry\":\"{entry}\"")),
            "{status}"
        );
        assert!(
            status.contains(&format!("\"surfaceTokenCount\":{count}")),
            "{status}"
        );
        assert!(!dir.join("playwright.log").exists());
        assert!(!dir.join("result.json").exists());
        assert!(
            summary.contains(&format!("matrix-{PREFLIGHT_LABEL}-{mode}=126\n")),
            "{summary}"
        );
        assert!(
            matrix.contains(&format!(
                "{PREFLIGHT_LABEL}/{mode} surface-token-count={count} exit=126 outcome=invalid-input nonGating=true"
            )),
            "{matrix}"
        );
    }
}

pub fn assert_contract() {
    // Present (one token) and duplicated (two tokens) surface flags are both
    // invalid for the promoted Variant A.
    assert_invalid_variant_a_preflight(&format!("{BASE_ARGS} {SURFACE_FLAG}"), 1);
    assert_invalid_variant_a_preflight(&format!("{BASE_ARGS} {SURFACE_FLAG} {SURFACE_FLAG}"), 2);
    assert_partial_override_controls();
}
