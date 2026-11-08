# AOT runtime measurements

Run `./scripts/aot-measure.sh` from any directory. It uses only the generated
`input-tester` synthetic fixture, builds one host-native AOT executable offline
in release mode with default discovery limits and a fixed 2,000,000-instruction
budget, then executes it seven times. Set `AOT_MEASURE_RUNS` to an integer from
3 through 25 to change the sample count. The script reports OS/architecture,
host triple, Cargo/rustc versions, build options, fixture identity, discovery
counts, generated source/executable byte sizes, and all fields in `artifact.meta`
and `build.meta`. Each sample includes `perf_elapsed_millis`, budget and native /
fallback counters, and final state hashes; samples must exhaust the budget and
match on deterministic counters/state. The summary is min/median/max.

`perf_elapsed_millis` is the generated harness timer for this one bounded guest
execution. It includes `run_program` and image snapshot computation, but excludes
process startup, ROM loading and harness setup, final-state digest/reporting,
optional snapshot-file I/O, fixture generation, AOT discovery/build and output
capture. Source bytes count `generated.rs`; executable bytes count the produced
host release harness file, not its runtime dependencies or total installed
footprint. The build workspace is uniquely created under ignored `output/` and
removed on exit; no known path is overwritten and no generated artifact or ROM
is committed.

These are host-observed measurements for one synthetic bounded run, not a
speedup comparison or reference baseline. Matching samples/metadata and
SHA-256 identify observed inputs/outputs; they do not prove speedup, gameplay,
ISA correctness, retail compatibility, or bit-for-bit independent
reproducibility. Larger workload, discovery-limit, and fallback evidence is
provided by the synthetic scale fixtures documented in
[`clean-room/verification.md`](clean-room/verification.md) for issue #15; those
measurements likewise are not compared against a reference speedup baseline.

## Recorded sample

One run on 2026-11-08 used Linux x86_64, host target
`x86_64-unknown-linux-gnu`, Cargo/rustc 1.99.1, release/offline mode and the
2,000,000-instruction budget. The generated 4,096-byte fixture had SHA-256
`8a7667d43d9ec0333a5a7c0a032837919e101ca1c1893bdf5f95f51b27555539`; default
discovery emitted 12 blocks / 65 instructions without reaching a limit. The
generated source was 49,844 bytes and the host executable 773,976 bytes. Seven
`perf_elapsed_millis` samples were `47.545, 43.315, 80.578, 47.507, 48.244,
79.845, 48.048` (min/median/max `43.315 / 48.048 / 80.578`). Every run used
2,000,000 native instructions, zero fallbacks, and identical state hashes:
CPU `f5b01c856e95ea2f6cc051ffe2723d541ffb8689d90ba2a3a8758bf19b6696ff`, bus
`bb97fcc950e26de2c6afe084954e62e47d068d2f3d8c418f5d476942e3e13ced`, VRAM
`7f73319a30b1ed3d288faffa9e39a8c0e32885cac6f99a1f938588a87cdd278e`, image
`d4e1751dce5a4ab05a8e402e6dc6f5b92102fdd55dc5f04cfa7c29d78137447d`. The
visible spread is host-observed timing variation, not a comparison or
performance claim.
