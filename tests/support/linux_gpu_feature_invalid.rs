//! Contract for an invalid `features-vulkan-from-angle` matrix input.

use std::path::Path;

use super::super::linux_gpu_matrix::read;

pub(super) fn assert_invalid_feature_probe(
    dir: &Path,
    mode: &str,
    launch_args: &str,
    count: usize,
    label: &str,
    change: &str,
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
    assert_eq!(read(&dir.join("changed-arg.txt")).trim_end(), change);
    let invalid = read(&dir.join("invalid-input.txt"));
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
    assert!(run_info.contains(&format!("label={label}")), "{run_info}");
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
