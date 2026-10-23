# Decision log

One line for each decision the maintainer takes, under the [decision rights](decision-rights.md) (VER-062). The line is an index: the decision itself is written where the rights say, in the OI, the plan, the requirement, or the pull request named on the line.

**The coordinator appends a line whenever the maintainer decides something**, in the pull request that records the decision, or, for a decision that has none, in the next pull request it opens. Keep the lines in date order: a line whose date is earlier than the last goes in its place, not at the end. When two pull requests add lines, the one that merges second resolves the conflict. Merges themselves are not logged (decision rights §2.10). `cargo xtask decisions` checks the format, in `cargo xtask ci` and the verify check `spec`; `cargo xtask metrics` counts the lines.

## Categories

`spec` · `scope` · `plan` · `api` · `vectors` · `review` · `process` · `deps` · `release` · `merge` · `housekeeping` · `approval`

## Format

```
- YYYY-MM-DD · <category> · <#N or chat> · <one line: what was decided>
```

- **Date:** the day the maintainer decided, as the record of the decision gives it.
- **Category:** one slug of the list above, defined in the decision rights §2.
- **Source:** the pull request or issue that records the decision (`#74`), or `chat` when it is recorded in none; when a chat decision is later written into a pull request, cite that pull request.
- **Decision:** one line. The fields are separated by ` · ` (a middle dot between spaces).

## Log

- 2026-10-20 · plan · #56 · M2 plan decisions D1–D17 accepted, after the review revised D2, D5, D11, D14, and D15 and added D17
- 2026-10-20 · approval · #54 · GEN-022/023 re-inspections of aldis-core, aldis-time, aldis-spp, and aldis-epp approved at the M1 exit
- 2026-10-20 · release · #54 · Release 0.2.0-m1 cut and tagged v0.2.0-m1
- 2026-10-20 · release · #57 · Repository made public, with a pre-release notice in README.md (the CodeQL CLI is free only for public repositories, VER-055)
- 2026-10-20 · api · #60 · Field code packet_len stays one code, the packet's total length for both SPP and EPP, not split per codec
- 2026-10-20 · approval · #60 · aldis-core GEN-022/023 revision for error.rs and field.rs (WP-0a) approved
- 2026-10-20 · spec · #61 · OI-05 confirmed: the TFDF pointer is present exactly for construction rules 000–010 (USLP-005 as written)
- 2026-10-20 · spec · #61 · OI-35 item 1: a truncated USLP frame's construction rule and UPID are exposed and not checked beyond USLP-016, in both modes (USLP-002)
- 2026-10-20 · spec · #61 · OI-35 item 8: FrameDelimiter delimits successive frames and takes a truncated frame's length from its VC configuration (USLP-015)
- 2026-10-20 · spec · #61 · AOS-018 keeps both limits: a B_PDU zone of at most 2047 octets, and in Issue 5 a frame of at most 2048 octets on B_PDU channels
- 2026-10-20 · plan · chat · Which tasks start next after the M2 plan was accepted
- 2026-10-21 · process · #63 · Verify reuses the results of unchanged checks, and verify.yml pulls the image from ghcr.io
- 2026-10-21 · release · #63 · GHCR package aldis-verify made public, so that verify.yml can pull it
- 2026-10-21 · approval · #64 · aldis-aos GEN-022/023 inspection, the first for the crate, approved
- 2026-10-21 · vectors · #64 · The FHEC miscorrection case moves to its own file, fhec_miscorrection.json (routine under the decision rights, §2.5)
- 2026-10-21 · release · #66 · The C++ port says it follows aldis-core 0.2.0 (v0.2.0-m1), while ALDIS_VERSION_* stays 0.1.0 until C1
- 2026-10-21 · vectors · #66 · The C++ vector loader stays stricter than the Rust runner; tightening the Rust runner is #67
- 2026-10-21 · api · #68 · Ocf stays closed (not non_exhaustive), the Clcw variant keeps its name and may hold a PLCW on USLP (CMN-010), and InvalidField is kept
- 2026-10-21 · approval · #68 · aldis-core GEN-022/023 revision for ocf.rs approved
- 2026-10-21 · api · #72 · C ABI status constants stay negative, ALDIS_ERR_<KIND> = -code (CABI-020); peek_length returns ALDIS_ERR_INCOMPLETE
- 2026-10-21 · api · #72 · aldis_error_detail_t begins with a caller-set uint32_t struct_size, and the library writes only the members inside it (CABI-021)
- 2026-10-21 · api · #72 · Accepted as built: field code 0 for argument errors, the aldis-spp suspend and resume API, the header check's inputs, and the capacities generator
- 2026-10-21 · review · #72 · Miri over the C ABI becomes a nightly follow-up, #71, instead of part of WP-0c
- 2026-10-21 · process · #74 · Structure batch approved: plan work-package tables, one record file per requirement, cargo xtask consistency, committed vector models, checklists
- 2026-10-21 · process · #74 · GEN-022/023 inspection records are approved once per milestone, at the exit review or before a release (VER-061)
- 2026-10-21 · scope · #74 · TV-023 and VER-061 join the M2 exit table
- 2026-10-21 · process · chat · Standing review watch: after opening a pull request the coordinator watches for its review, then answers and addresses it unasked
- 2026-10-21 · process · chat · Cleanup policy: when a pull request merges, delete its worktree and local branch unless they hold unpushed or uncommitted work
- 2026-10-21 · process · #75 · The xtask crate split is held, and revisited at the M2 exit review with the measurement #75 describes
- 2026-10-21 · plan · chat · Which tasks start next (the day's dispatches, among them the decision rights and the PR metrics)
- 2026-10-21 · process · chat · Decision rights accepted as drafted: the coordinator starts queued work in the order last given; a PR may implement the recommended option of a pending decision but is not readied until it is answered; readings the normative text settles are decided and cited in the PR; one log file, routine items unlogged, the decisions check and the PR-template section kept
- 2026-10-21 · scope · #78 · ACFS D1, D19: one milestone, ACFS, in four phases P1–P4, gating no release
- 2026-10-21 · plan · #78 · ACFS D8: lockstep sequencing by `refapp_step`; cFE TIME runs as shipped, outside the trace, and OI-36 is deferred
- 2026-10-21 · scope · #78 · ACFS D18: the harness side of the node protocol, with both transports, is built in ACFS WP-2 rather than A5 (OI-38)
- 2026-10-21 · deps · #78 · ACFS D3, D5, D14: nasa/cFS `v7.0.1` (Apache-2.0), fetched and built by a nightly job, not vendored
- 2026-10-21 · plan · #78 · ACFS D2, D4, D6, D7, D9–D13, D15–D17 accepted as recommended
- 2026-10-21 · spec · #79 · GEN-071, VER-002, RS-017: `unsafe` is allowed in `crosscheck/src/cfe_msg.rs` only, chosen over a C subprocess driver
- 2026-10-21 · deps · #79 · `crosscheck/` vendors 13 unmodified cFE v7.0.1 files and takes the `cc` 1.9.0 build dependency
- 2026-10-21 · spec · #79 · VER-051 (SEI CERT C) applies to the cFE stubs and `abi_check.c`, not to the vendored cFE files
- 2026-10-21 · spec · #82 · CUC nanosecond conversions take the basic time unit to be the second; TIME-011 and the API docs say so, and that a Level 2 code converts correctly only if the agency's unit is the second (option a)
- 2026-10-21 · review · chat · The maintainer reviews the four `spacepackets` discrepancies (cross-checks.md §3) against 301.0 first, from a packet that withholds Aldis's reasoning
- 2026-10-21 · spec · #80 · OI-39 deferred: CUC P-field octet 2 stays accepted (TIME-010); which of 301.0 §3.1.1 and §3.2.2 governs waits for a real system or an expert (#84)
- 2026-10-21 · spec · #80 · OI-40: nanoseconds convert to CUC fine time by rounding down; TIME-011 and the `cuc_from_ns` vectors change in a follow-up
- 2026-10-21 · plan · chat · Start the Rust LEON3 spike (OI-08); the C++ LEON3 spike waits
- 2026-10-21 · scope · #81 · OI-08: Rust on LEON stays best-effort in v1, with native C++ the primary LEON path; `rust-sparc-leon3` stays non-gating, revisited if the target reaches tier 2
- 2026-10-21 · api · #81 · CABI-004 on SPARC: the `compiler_builtins` `_Q_*` references are a recorded deviation with an integrator note (`--gc-sections`, or supply them); no upstream report
- 2026-10-21 · deps · #81 · The verify image's SPARC binutils and QEMU are pinned to their upstream versions only, not to Ubuntu revisions
- 2026-10-23 · api · #89 · The OCF crosses the C ABI as `(const uint8_t *ocf, size_t ocf_len)`; a length other than 4 is a CABI-022 violation
- 2026-10-23 · api · #89 · `aldis_oid_generator_fill` takes `(out, len)` with no `out_len`: it always fills exactly `len` octets
- 2026-10-23 · api · #89 · `aldis_ocf_check` is public C ABI, the C form of the OCF reports' checked constructors
- 2026-10-23 · api · #90 · TM per-VC security layouts come through `VcLookup::security_layout(scid, vcid)`, implemented by `TmMasterChannelConfig` (holding its at most 8 VCs) and `NoSdls`; `decode` keeps the profile signature, `decode_with` takes a lookup
