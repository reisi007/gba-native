use super::super::linux_ab_fixture;
use super::super::linux_gpu_matrix::{
    BASE_ARGS, SURFACE_FLAG, newest_diagnostics_dir, read, run_fixture,
};

/// The preflight records the raw invalid Variant A flags in JSON, so newline and
/// control characters must be escaped instead of producing a malformed line.
pub fn assert_control_characters_are_json_escaped() {
    let (_temp, root, bin, calls) = linux_ab_fixture();
    let launch_args = format!("{BASE_ARGS} {SURFACE_FLAG}\n--embedded-control=\u{1}");
    let output = run_fixture(&root, &bin, &calls, &launch_args, None);
    assert!(
        !output.status.success(),
        "an override that carries the surface token fails the preflight"
    );
    assert!(
        std::fs::read_to_string(&calls).map_or(true, |log| log.is_empty()),
        "an invalid override launches no A/B probe"
    );

    let diagnostics = newest_diagnostics_dir(&root);
    for mode in ["canvas", "app"] {
        let dir = diagnostics.join(format!("matrix-preflight-variant-a-{mode}"));
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
