//! Contract for the additive Linux feature-combination diagnostic.
//!
//! This is the non-gating `features-vulkan-from-angle` candidate next to the
//! one-flag omission matrix. It substitutes exactly the single
//! `--enable-features=Vulkan` argv token, keeps every other token in order and
//! probes both the real served app and the standalone canvas document.

use std::path::{Path, PathBuf};

use super::linux_ab_fixture;
use super::linux_gpu_matrix::{
    BASE_ARGS, diagnostics_dir, omit_first, read, run_fixture, smoke_lines,
};

#[path = "linux_gpu_json.rs"]
mod linux_gpu_json;

const LABEL: &str = "features-vulkan-from-angle";
const FROM: &str = "--enable-features=Vulkan";
const TO: &str = "--enable-features=Vulkan,VulkanFromANGLE,DefaultANGLEVulkan";
const CHANGE: &str = "changed=--enable-features=Vulkan -> --enable-features=Vulkan,VulkanFromANGLE,DefaultANGLEVulkan";

fn substitute_feature(tokens: &[String]) -> Vec<String> {
    let mut substituted = Vec::new();
    let mut replaced = false;
    for token in tokens {
        if !replaced && token == FROM {
            substituted.push(TO.to_owned());
            replaced = true;
        } else {
            substituted.push(token.clone());
        }
    }
    assert!(replaced, "the Variant A flags must contain {FROM}");
    substituted
}

fn feature_probes(smokes: &[(String, String, String)], args: &str) -> Vec<(String, String)> {
    smokes
        .iter()
        .filter(|probe| probe.1 == args)
        .map(|probe| (probe.0.clone(), probe.2.clone()))
        .collect()
}

fn assert_feature_probe(dir: &Path, mode: &str, args: &str) {
    for artifact in [
        "launch-args.txt",
        "changed-arg.txt",
        "run-info.txt",
        "exit-status.txt",
        "process-status.json",
    ] {
        assert!(
            dir.join(artifact).is_file(),
            "missing {}/{artifact}",
            dir.display()
        );
    }
    assert!(
        !dir.join("omitted-flag.txt").exists(),
        "the feature control is not an omission: {}",
        dir.display()
    );
    assert_eq!(read(&dir.join("launch-args.txt")).trim_end(), args);
    assert_eq!(read(&dir.join("changed-arg.txt")).trim_end(), CHANGE);
    assert_eq!(read(&dir.join("exit-status.txt")).trim_end(), "0");
    assert!(dir.join("playwright.log").is_file());
    assert!(dir.join("result.json").is_file());

    let entry = if mode == "app" {
        "entry=app:/?autostart=1&backend=webgpu"
    } else {
        "entry=standalone:/__linux_webgpu_smoke__"
    };
    let run_info = read(&dir.join("run-info.txt"));
    assert!(run_info.contains(&format!("label={LABEL}")), "{run_info}");
    assert!(run_info.contains(&format!("mode={mode}")), "{run_info}");
    assert!(
        run_info.contains("control=feature-substitution"),
        "{run_info}"
    );
    assert!(
        run_info.contains(&format!("changed_flag={FROM}")),
        "{run_info}"
    );
    assert!(
        run_info.contains(&format!("replacement_flag={TO}")),
        "{run_info}"
    );
    assert!(run_info.contains(entry), "{run_info}");
    assert!(run_info.contains("non_gating=true"), "{run_info}");

    let status = read(&dir.join("process-status.json"));
    let status_entry = if mode == "app" {
        "\"entry\":\"app:/?autostart=1&backend=webgpu\"".to_owned()
    } else {
        "\"entry\":\"standalone:/__linux_webgpu_smoke__\"".to_owned()
    };
    for field in [
        format!("\"label\":\"{LABEL}\""),
        format!("\"mode\":\"{mode}\""),
        "\"control\":\"feature-substitution\"".to_owned(),
        format!("\"changedFlag\":\"{FROM}\""),
        format!("\"replacementFlag\":\"{TO}\""),
        format!("\"launchArgs\":\"{args}\""),
        "\"processExitStatus\":0".to_owned(),
        "\"resultWritten\":true".to_owned(),
        "\"outcome\":\"completed\"".to_owned(),
        "\"nonGating\":true".to_owned(),
        status_entry,
    ] {
        assert!(status.contains(&field), "missing {field}: {status}");
    }
}

fn assert_invalid_feature_probe(
    dir: &Path,
    mode: &str,
    launch_args: &str,
    count: usize,
    reason_fragment: &str,
) {
    for artifact in [
        "launch-args.txt",
        "changed-arg.txt",
        "invalid-input.txt",
        "run-info.txt",
        "exit-status.txt",
        "process-status.json",
    ] {
        assert!(
            dir.join(artifact).is_file(),
            "missing {}/{artifact}",
            dir.display()
        );
    }
    assert_eq!(read(&dir.join("launch-args.txt")).trim_end(), launch_args);
    assert_eq!(read(&dir.join("changed-arg.txt")).trim_end(), CHANGE);
    let invalid = read(&dir.join("invalid-input.txt"));
    assert!(invalid.contains(reason_fragment), "{invalid}");
    assert!(invalid.contains(&format!("{count} times")), "{invalid}");
    assert_eq!(read(&dir.join("exit-status.txt")).trim_end(), "126");
    assert!(!dir.join("playwright.log").exists());
    assert!(!dir.join("result.json").exists());

    let entry = if mode == "app" {
        "app:/?autostart=1&backend=webgpu"
    } else {
        "standalone:/__linux_webgpu_smoke__"
    };
    let run_info = read(&dir.join("run-info.txt"));
    assert!(run_info.contains(&format!("label={LABEL}")), "{run_info}");
    assert!(run_info.contains(&format!("mode={mode}")), "{run_info}");
    assert!(run_info.contains(entry), "{run_info}");
    assert!(run_info.contains("non_gating=true"), "{run_info}");

    let status = read(&dir.join("process-status.json"));
    assert!(status.contains("\"outcome\":\"invalid-input\""), "{status}");
    assert!(status.contains("\"nonGating\":true"), "{status}");
    assert!(status.contains("\"resultWritten\":false"), "{status}");
    assert!(
        status.contains(&format!("\"launchArgs\":\"{launch_args}\"")),
        "{status}"
    );
}

fn assert_source_contract(root: &Path) {
    let source = read(&root.join("scripts/web-e2e.sh"));
    for contract in [
        "FEATURE_CONTROL_LABEL=\"features-vulkan-from-angle\"",
        "FEATURE_CONTROL_FROM=\"--enable-features=Vulkan\"",
        "FEATURE_CONTROL_TO=\"--enable-features=Vulkan,VulkanFromANGLE,DefaultANGLEVulkan\"",
        "run_feature_control_probe canvas",
        "run_feature_control_probe app",
    ] {
        assert!(
            source.contains(contract),
            "missing feature control contract: {contract}"
        );
    }
    assert!(
        !source.contains("--use-gl=angle"),
        "the feature control must not add --use-gl=angle"
    );
    let feature_arm = source
        .split("# Additive feature-combination candidate")
        .nth(1)
        .expect("feature-combination control arm")
        .split("SHARED_IMAGE_LOGS=(")
        .next()
        .expect("feature-combination control body");
    for contract in [
        "record_feature_invalid_input canvas",
        "record_feature_invalid_input app",
        "run_feature_control_probe canvas",
        "run_feature_control_probe app",
    ] {
        assert!(
            feature_arm.contains(contract),
            "the feature arm must reach both entries and invalid input: {contract}"
        );
    }
    assert!(
        !feature_arm.contains("fail "),
        "the feature arm must never fail the browser gate"
    );
}

fn assert_prior_matrix_preserved(diagnostics: &Path, base_tokens: &[String]) {
    let baseline = diagnostics.join("matrix-baseline-app");
    assert!(baseline.join("result.json").is_file());
    assert_eq!(read(&baseline.join("exit-status.txt")).trim_end(), "0");
    for (omitted, label) in [
        (FROM, "omit-enable-features-vulkan"),
        ("--use-angle=swiftshader", "omit-use-angle-swiftshader"),
        ("--use-vulkan=swiftshader", "omit-use-vulkan-swiftshader"),
        ("--disable-vulkan-surface", "omit-disable-vulkan-surface"),
    ] {
        let expected = omit_first(base_tokens, omitted).join(" ");
        for mode in ["canvas", "app"] {
            let dir = diagnostics.join(format!("matrix-{label}-{mode}"));
            assert!(dir.join("result.json").is_file(), "{}", dir.display());
            assert_eq!(read(&dir.join("launch-args.txt")).trim_end(), expected);
            assert_eq!(read(&dir.join("exit-status.txt")).trim_end(), "0");
        }
    }
}

pub fn assert_contract(root: &Path) {
    assert_source_contract(root);

    let (_temp, fixture_root, bin, calls) = linux_ab_fixture();
    let output = run_fixture(&fixture_root, &bin, &calls, BASE_ARGS, None);
    assert!(
        output.status.success(),
        "the feature control must stay non-gating:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ab = read(&calls);
    let ab_lines: Vec<_> = ab.lines().collect();
    assert_eq!(ab_lines.len(), 2, "A and B must still run: {ab}");
    let (a_args, a_output) = ab_lines[0].split_once('\t').expect("A call fields");
    let (b_args, b_output) = ab_lines[1].split_once('\t').expect("B call fields");
    let base_tokens: Vec<String> = a_args.split_whitespace().map(str::to_owned).collect();
    assert_eq!(
        base_tokens
            .iter()
            .filter(|token| *token == "--disable-vulkan-surface")
            .count(),
        1,
        "Variant A must contain exactly one --disable-vulkan-surface token"
    );
    assert!(
        !b_args
            .split_whitespace()
            .any(|token| token == "--disable-vulkan-surface"),
        "Variant B must not contain --disable-vulkan-surface"
    );
    assert_eq!(
        omit_first(&base_tokens, "--disable-vulkan-surface").join(" "),
        b_args,
        "Variant B is Variant A with exactly the one --disable-vulkan-surface token removed"
    );
    assert!(b_output.ends_with("variant-b/test-results"), "{b_output}");
    assert!(a_output.ends_with("variant-a/test-results"), "{a_output}");

    let smokes = smoke_lines(&calls);
    assert_eq!(
        smokes.len(),
        13,
        "two standalone probes, one baseline, eight omission and two feature probes: {smokes:?}"
    );
    let substituted = substitute_feature(&base_tokens);
    assert_eq!(
        substituted.len(),
        base_tokens.len(),
        "the feature control substitutes one token in place"
    );
    assert_eq!(
        substituted.iter().filter(|token| *token == TO).count(),
        1,
        "exactly one additive feature token"
    );
    assert_eq!(
        substituted
            .iter()
            .filter(|token| token.starts_with("--enable-features="))
            .count(),
        1,
        "the original feature token is replaced, never duplicated"
    );
    for (index, (original, changed)) in base_tokens.iter().zip(substituted.iter()).enumerate() {
        if original == FROM {
            assert_eq!(changed, TO, "token {index} must become the additive list");
        } else {
            assert_eq!(
                changed, original,
                "token {index} must keep its exact bytes and position"
            );
        }
    }
    let feature_args = substituted.join(" ");
    assert!(
        !feature_args.contains("--use-gl=angle"),
        "the control must not add --use-gl=angle: {feature_args}"
    );
    assert!(
        !feature_args.contains("--use-angle=opengl"),
        "the control must not switch ANGLE: {feature_args}"
    );

    let diagnostics = diagnostics_dir(&calls);
    let mut probes = feature_probes(&smokes, &feature_args);
    probes.sort();
    assert_eq!(
        probes
            .iter()
            .map(|(mode, _)| mode.as_str())
            .collect::<Vec<_>>(),
        ["app", "canvas"],
        "both entry paths must be probed: {probes:?}"
    );
    for (mode, probe_dir) in &probes {
        let expected_dir: PathBuf = diagnostics.join(format!("matrix-{LABEL}-{mode}"));
        assert_eq!(Path::new(probe_dir), expected_dir.as_path());
        assert_feature_probe(Path::new(probe_dir), mode, &feature_args);
    }
    assert_prior_matrix_preserved(&diagnostics, &base_tokens);

    let summary = read(&diagnostics.join("summary.txt"));
    for mode in ["canvas", "app"] {
        assert!(
            summary.contains(&format!("matrix-{LABEL}-{mode}=0\n")),
            "missing {mode} summary row"
        );
    }
    assert!(summary.contains("matrix-non-gating=true\n"), "{summary}");
    let matrix = read(&diagnostics.join("matrix.txt"));
    assert!(
        matrix.contains(&format!(
            "{LABEL}/canvas {CHANGE} exit=0 outcome=completed nonGating=true"
        )),
        "{matrix}"
    );
    assert!(
        matrix.contains(&format!(
            "{LABEL}/app {CHANGE} exit=0 outcome=completed nonGating=true"
        )),
        "{matrix}"
    );
    let step = read(&fixture_root.join("step-summary.md"));
    assert!(step.contains(&format!("| `{LABEL}` | canvas |")), "{step}");
    assert!(step.contains(&format!("| `{LABEL}` | app |")), "{step}");
    assert!(step.contains("**non-gating**"), "{step}");

    // A missing control token is an explicit invalid-input result for both
    // entries, never a skipped combination, and the rest of the matrix still
    // runs without changing the gating status.
    let (_missing_temp, missing_root, missing_bin, missing_calls) = linux_ab_fixture();
    let override_tokens: Vec<String> = BASE_ARGS.split_whitespace().map(str::to_owned).collect();
    let missing_tokens: Vec<String> = override_tokens
        .iter()
        .filter(|token| *token != FROM)
        .cloned()
        .collect();
    let missing_text = missing_tokens.join(" ");
    let missing_effective = format!("{missing_text} --enable-logging=stderr --log-level=0");
    let output = run_fixture(
        &missing_root,
        &missing_bin,
        &missing_calls,
        &missing_text,
        None,
    );
    assert!(
        output.status.success(),
        "a missing feature token must stay non-gating:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let missing_diagnostics = diagnostics_dir(&missing_calls);
    let missing_smokes = smoke_lines(&missing_calls);
    assert_eq!(
        missing_smokes.len(),
        9,
        "the baseline and the three formable omission pairs still run: {missing_smokes:?}"
    );
    for mode in ["canvas", "app"] {
        let dir = missing_diagnostics.join(format!("matrix-{LABEL}-{mode}"));
        assert_invalid_feature_probe(&dir, mode, &missing_effective, 0, FROM);
        assert!(dir.join("invalid-input.txt").is_file());
    }
    let missing_summary = read(&missing_diagnostics.join("summary.txt"));
    assert!(
        missing_summary.contains(&format!("matrix-{LABEL}-canvas=126\n")),
        "{missing_summary}"
    );
    assert!(
        missing_summary.contains(&format!("matrix-{LABEL}-app=126\n")),
        "{missing_summary}"
    );
    assert!(
        missing_diagnostics
            .join("matrix-baseline-app")
            .join("result.json")
            .is_file(),
        "the baseline still runs when the feature token is missing"
    );
    for label in [
        "omit-use-angle-swiftshader",
        "omit-use-vulkan-swiftshader",
        "omit-disable-vulkan-surface",
    ] {
        assert!(
            missing_diagnostics
                .join(format!("matrix-{label}-canvas"))
                .join("result.json")
                .is_file(),
            "the {label} control still runs when the feature token is missing"
        );
    }

    // A duplicated control token is recorded as invalid-input too, and it does
    // not trip the existing A/B preflight (which only guards
    // --disable-vulkan-surface).
    let (_duplicate_temp, duplicate_root, duplicate_bin, duplicate_calls) = linux_ab_fixture();
    let duplicate_text = override_tokens
        .join(" ")
        .replace(FROM, &format!("{FROM} {FROM}"));
    let duplicate_effective = format!("{duplicate_text} --enable-logging=stderr --log-level=0");
    let output = run_fixture(
        &duplicate_root,
        &duplicate_bin,
        &duplicate_calls,
        &duplicate_text,
        None,
    );
    assert!(
        output.status.success(),
        "a duplicated feature token must stay non-gating:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let duplicate_diagnostics = diagnostics_dir(&duplicate_calls);
    assert_eq!(
        smoke_lines(&duplicate_calls).len(),
        9,
        "the other matrix probes still run with a duplicated feature token"
    );
    for mode in ["canvas", "app"] {
        let dir = duplicate_diagnostics.join(format!("matrix-{LABEL}-{mode}"));
        assert_invalid_feature_probe(&dir, mode, &duplicate_effective, 2, FROM);
    }
    assert!(
        duplicate_diagnostics
            .join("matrix-omit-use-angle-swiftshader-canvas")
            .join("result.json")
            .is_file(),
        "the omission controls still run with a duplicated feature token"
    );

    // A feature probe that exits without result.json is recorded as an explicit
    // harness failure, while the other feature entry still completes.
    let (_harness_temp, harness_root, harness_bin, harness_calls) = linux_ab_fixture();
    let output = run_fixture(
        &harness_root,
        &harness_bin,
        &harness_calls,
        BASE_ARGS,
        Some("app"),
    );
    assert!(
        output.status.success(),
        "feature harness failures are recorded, not gated:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let harness_diagnostics = diagnostics_dir(&harness_calls);
    let app_dir = harness_diagnostics.join(format!("matrix-{LABEL}-app"));
    assert!(app_dir.join("playwright.log").is_file());
    assert!(!app_dir.join("result.json").exists());
    assert_eq!(read(&app_dir.join("exit-status.txt")).trim_end(), "23");
    let status = read(&app_dir.join("process-status.json"));
    assert!(
        status.contains("\"outcome\":\"harness-failure\""),
        "{status}"
    );
    assert!(status.contains("\"resultWritten\":false"), "{status}");
    assert!(status.contains("\"nonGating\":true"), "{status}");
    let canvas_dir = harness_diagnostics.join(format!("matrix-{LABEL}-canvas"));
    assert!(
        canvas_dir.join("result.json").is_file(),
        "both feature entries are attempted even when one fails"
    );
    assert_eq!(read(&canvas_dir.join("exit-status.txt")).trim_end(), "0");

    linux_gpu_json::assert_control_characters_are_json_escaped();
}
