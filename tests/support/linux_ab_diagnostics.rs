//! Gating contract for the Linux A/B diagnostics. The two standalone probes and
//! the full Variant A suite gate the job; Variant B still executes its full suite
//! and records its result, logs and artifacts as a non-gating diagnostic.

use std::path::{Path, PathBuf};

use super::{
    linux_ab_fixture, linux_gpu_matrix, linux_webgpu_smoke, repo_root, run_linux_ab_fixture,
};

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

pub fn assert_contract() {
    linux_webgpu_smoke::assert_probe_source(&repo_root());
    let (temp, root, bin, calls) = linux_ab_fixture();
    let launch_args = linux_gpu_matrix::BASE_ARGS;

    let a_failure = run_linux_ab_fixture(&root, &bin, &calls, launch_args, Some("A"));
    assert!(!a_failure.status.success());
    let output_text = String::from_utf8_lossy(&a_failure.stderr);
    assert!(
        output_text.contains("canvas=19, offscreen=0, A=17"),
        "{output_text}"
    );
    assert!(
        !output_text.contains("variant B failed"),
        "variant B must not be blamed for the A failure: {output_text}"
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

    // A Variant B failure is retained and reported, but must not change the
    // overall gate while both standalone probes and Variant A pass.
    linux_gpu_matrix::reset_calls(&calls);
    let b_failure = run_linux_ab_fixture(&root, &bin, &calls, launch_args, Some("B"));
    assert!(
        b_failure.status.success(),
        "a Variant B failure must not gate when the probes and A pass:\n{}",
        String::from_utf8_lossy(&b_failure.stderr)
    );
    let output_text = String::from_utf8_lossy(&b_failure.stderr);
    assert!(output_text.contains("A=0 (gated) B=18"), "{output_text}");
    assert!(
        output_text.contains("variant B failed with status 18"),
        "the retained B failure must stay visible: {output_text}"
    );
    let b_calls = std::fs::read_to_string(&calls).expect("read both variant calls");
    assert_eq!(
        b_calls.lines().count(),
        2,
        "variant B must still execute its full Playwright run: {b_calls}"
    );
    let second_dir = newest_linux_ab_dir(&root);
    let summary =
        std::fs::read_to_string(second_dir.join("summary.txt")).expect("read B-failure summary");
    assert!(
        summary.contains("smoke-canvas=0\nsmoke-offscreen=0\nA=0\nB=18\n"),
        "{summary}"
    );
    assert!(
        summary.contains("A-gated=true\nB-non-gating=true\n"),
        "{summary}"
    );
    let b_variant = second_dir.join("variant-b");
    assert_eq!(
        std::fs::read_to_string(b_variant.join("exit-status.txt"))
            .expect("read retained B exit status")
            .trim_end(),
        "18"
    );
    for artifact in ["launch-args.txt", "playwright.log", "exit-status.txt"] {
        assert!(b_variant.join(artifact).is_file(), "missing B {artifact}");
    }
    assert!(b_variant.join("test-results").is_dir());
    let b_command = std::fs::read_to_string(b_variant.join("test-results/command.txt"))
        .expect("read retained B Playwright command");
    assert!(
        b_command
            .trim_end()
            .ends_with("test -c playwright.config.ts"),
        "variant B must invoke the full Playwright suite: {b_command}"
    );
    assert!(
        !b_command.contains("--grep") && !b_command.contains("--shard"),
        "variant B must not filter or shard the fixture suite: {b_command}"
    );
    assert!(
        std::fs::read_to_string(b_variant.join("playwright.log"))
            .expect("read retained B log")
            .contains("fake Playwright run 2"),
        "variant B must run the fixture suite"
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
