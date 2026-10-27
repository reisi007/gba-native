use super::super::linux_ab_fixture;
use super::super::linux_gpu_matrix::{BASE_ARGS, newest_diagnostics_dir, read, run_fixture};

pub fn assert_control_characters_are_json_escaped() {
    let (_temp, root, bin, calls) = linux_ab_fixture();
    let without_feature = BASE_ARGS
        .split_whitespace()
        .filter(|token| *token != "--enable-features=Vulkan")
        .collect::<Vec<_>>()
        .join(" ");
    let launch_args = format!("{without_feature}\n--embedded-control=\u{1}");
    let output = run_fixture(&root, &bin, &calls, &launch_args, None);
    assert!(
        output.status.success(),
        "malformed feature input remains non-gating:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let diagnostics = newest_diagnostics_dir(&root);
    for mode in ["canvas", "app"] {
        let dir = diagnostics.join(format!("matrix-features-vulkan-from-angle-{mode}"));
        let status = read(&dir.join("process-status.json"));
        assert_eq!(status.lines().count(), 1, "invalid JSON line: {status:?}");
        assert!(
            status.contains("\\u000a"),
            "newline must be escaped: {status:?}"
        );
        assert!(
            status.contains("\\u0001"),
            "control byte must be escaped: {status:?}"
        );
        assert!(
            !status.contains('\u{1}'),
            "raw JSON control byte must not be present: {status:?}"
        );
        assert!(status.contains("\"outcome\":\"invalid-input\""), "{status}");
        assert!(status.contains("\"nonGating\":true"), "{status}");
        assert_eq!(read(&dir.join("exit-status.txt")).trim_end(), "126");
    }
}
