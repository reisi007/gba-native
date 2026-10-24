# REA status — blocked, not reverse-engineered

Observed 2026-10-24 using the connected REA MCP server. This document does not
change the repository-wide status: experimental, no playable retail game, no GPU
vendor integration, no REA Evidence, not a formal separate-team legal clean-room.

1. `binary_session({})` reported no active target.
2. Ghidra was unavailable: `GHIDRA_INSTALL_DIR` not configured.
3. Hopper was unavailable: launcher not present at its configured app path.
4. `open_binary` on the workspace's supplied `.gba` ROM returned
   `target_unavailable`, reason **unsupported binary format**, non-retryable.

No ROM instructions were disassembled or decompiled through REA. No successful
artifact-analysis Evidence ID exists for this investigation; none is invented.
These are tool diagnostics, not successful native-analysis Evidence records.

The first hybrid AOT milestone did **not** retry REA, install providers, download
analysis engines or open a reference emulator. Implementation used public
hardware documentation only. No provider, system configuration or ROM was
modified. No analysis target was successfully opened, so no native session needs
closing.

## Follow-up boundary

Installing Ghidra alone does not establish that REA can import raw GBA images.
A future investigation must first establish a supported, correctly mapped ARMv4T
target (ROM base `0x08000000`) and an available provider. A derived container,
if needed, must retain the original ROM digest and explicit mapping/provenance;
do not mislabel it as the original artifact. Do not select a reference emulator
or download an analysis engine without agreement on that scope.

If REA later supplies genuine native observations, export the Evidence record
separately and preserve its limitations. Behavioral requirements may be handed to
an implementer only under the separation rules in `protocol.md`; static
observations are not runtime proof of all hardware behavior.
