# Open work

Open work is tracked exclusively in GitHub Issues:

<https://github.com/reisi007/gba-native/issues>

The issue tracker is the authoritative task list. This file is a pointer, not a
backlog: it intentionally does not copy issue titles, statuses or checked-off
history. Lasting decisions live in `docs/decisions.md`; completed work is
recorded in Git.

- #1 — current first-milestone verification.
- #2 — quality foundation (Ratchet, 500-line limit, short/full scripts, CI).
- #3–#19 — hardware, frontend, packaging, REA, compatibility, performance and
  artifact-trust work owned by `reisi007`.

#17 (committed *native-harness* self-modifying-RAM regression) stays open until
it is implemented and independently verified. The reference interpreter already
has a committed regression in
`crates/core/tests/cpu.rs::ram_code_is_decoded_fresh_after_self_modification`; only the
native differential case is missing.

An issue is a record, not a commission. Work still needs an explicit user
commission, and an issue is closed only after independent verification, the
repository gates and CI pass.
