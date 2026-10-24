# Decision log

One line for each decision the maintainer takes, under the [decision rights](decision-rights.md) (VER-062). The line is an index: the decision itself is written where the rights say, in the OI, the plan, the requirement, or the pull request named on the line.

**The coordinator appends a line whenever the maintainer decides something**, in the pull request that records the decision, or, for a decision that has none, in the next pull request it opens. Keep the lines in date order: a line whose date is earlier than the last goes in its place, not at the end. Lines are added, never edited or removed; a correction is a new line. Git merges the log with `merge=union` ([`.gitattributes`](../../.gitattributes)), so when two pull requests add lines at the same place it keeps both sides instead of stopping at a conflict; `cargo xtask decisions` then reports a line the union left out of date order, or a line it kept twice, and the merge fixes it. Merges themselves are not logged (decision rights §2.10). `cargo xtask decisions` checks the format, in `cargo xtask ci` and the verify check `spec`; `cargo xtask metrics` counts the lines.

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
- 2026-10-23 · spec · #91 · OI-41 (1): a TC frame needs at least one User Data octet, with or without a Segment Header and SDLS fields (CMN-041)
- 2026-10-23 · spec · #91 · OI-41 (2): a valid header for TC delimiting is at least five octets with TFVN `00` and a Frame Length of at least six; otherwise the rest of the unit is Fill
- 2026-10-23 · api · #91 · Field code 54 `user_len` is shared by SPP and TC (one code per field, M2 D2)
- 2026-10-23 · api · #91 · Field code 68 `frame_len` is one code shared by TM (the Transfer Frame Length parameter) and TC (the Frame Length field), kind `field`; AOS and USLP share it too
- 2026-10-23 · plan · chat · Continue the Kani formal-verification spike (OI-42) found uncommitted in a worktree, and open it as a pull request
- 2026-10-23 · process · #92 · OI-42 (a) as far as a non-gating check: the verify check `kani` runs the Kani harnesses in the verification image, which installs Kani 0.68.0 with its bundle checked against GitHub's asset digest; gating and a verification method stay open
- 2026-10-24 · process · chat · An independent reviewer does not re-run the merge gate's checks; it reads the published record (`cargo xtask verify check`) and reviews what no check covers (AGENTS.md, Independent review)
- 2026-10-24 · api · #94 · An array of versioned C ABI configuration structs uses the first element's `struct_size` as its stride; every element repeats it, at least the library's size and a multiple of its alignment (c-abi.md §11)
- 2026-10-24 · api · #94 · A C ABI master channel configuration embeds its physical channel configuration; each per-VC call takes one master channel, and the Rust lookup over several master channels has no C form (c-abi.md §11)
- 2026-10-24 · api · #94 · C ABI decoders and validation without a suffix mean no SDLS; the `_with` variants take each VC's security layout from the master channel; no lookup-mode argument (c-abi.md §11)
- 2026-10-24 · api · #94 · A stateless `_writer_finish` over a writer whose `begin` zero-fills calls a public `resume` of the codec (`TmFrameWriter::resume`), not FECF or header code in `aldis-ffi` (c-abi.md §11)
- 2026-10-24 · api · #94 · aldis_tm_encode takes the five frame-content slices in one aldis_tm_content_t (review finding 2); the VC array stays as arguments
- 2026-10-24 · api · #94 · One aldis_tm_validate, which uses the master channel's security layouts; the _with pairs exist only for decoders, whose plain form takes the physical channel (review finding 3, c-abi.md §11)
- 2026-10-24 · spec · #98 · OI-44 (1): on VCID 63 a USLP decoder checks the OID header (MAP ID 0, rule `001`, UPID Only Idle Data, LVO at the last TFDZ octet): `InvalidField` with reason 17 in strict mode, the anomaly `OidHeaderMismatch` (bit 1) in lenient mode
- 2026-10-24 · spec · #98 · OI-44 (2): a USLP TFDZ holds at least one octet, as OI-41 (1) for TC
- 2026-10-24 · spec · #98 · OI-44 (3): a valid header for USLP delimiting is TFVN `1100` and either a truncated header or at least seven octets with Frame Length + 1 ≥ 9 + the VCF Count Length, as OI-41 (2) for TC
- 2026-10-24 · process · chat · After a PR merges, the coordinator merges main into the next ready PR in the merge order and starts its verify run at once, so the maintainer does not wait on the up-to-date run (VER-058)
- 2026-10-24 · process · chat · The workflow definitions and the other .github files no check reads become inputs of spec only (VER-056); the xtask module split of the inputs is not adopted
- 2026-10-24 · spec · #97 · OI-43 (1): an OID frame on VCID 63 carries neither an OCF nor SDLS fields; it is the Primary Header, the optional Insert Zone, the OID data, and the optional FECF (732.0 Table 5-3, §4.1.4.2.3.4 note 1)
- 2026-10-24 · api · #97 · Field codes 93 `vcf_count` and 97 `insert_zone_len` are each one code shared by USLP and AOS (one code per field, M2 D2); the AOS codes are 100–106
- 2026-10-24 · process · chat · Standing process review: the coordinator flags an order inversion (an approved pull request blocked by an unreviewed one it depends on) as soon as it happens, and after each nightly report (#87) recommends changes only on process-created waits (inversions, update runs after approval, wasted runs, rework, repeated conflicts); review latency counts only in the maintainer's active time and is never alone a reason
- 2026-10-24 · process · chat · The Kani check runs only in the nightly verification run (`verify --all`, VER-059), not in pull-request runs; it stays non-gating (OI-42)
- 2026-10-24 · process · #101 · VER-056: a run that cannot publish MAY stop at its first gating failure; a run in which no gating check needs a container MAY start none, the non-gating checks it would run reported as not run
- 2026-10-24 · process · #101 · VER-057: a run without a container records that no image was used, with the commit's image-input hash
- 2026-10-24 · process · #101 · VER-060: a run that needs no image neither pulls nor builds one
- 2026-10-24 · process · #101 · A nightly-only check (Kani) that does not pass in the nightly run fails the job and opens the nightly-failure issue (VER-059)
- 2026-10-24 · process · #101 · verify --publish stores a passing record even when main moved during the run, without setting aldis/verify; such a record may be a reuse source; the merge gate is unchanged (VER-057, VER-058)
- 2026-10-24 · process · #101 · The VER-056, VER-057, and VER-058 wording for records of commits behind main accepted as drafted; verify --publish exits with a failure when it publishes such a record, and verify check fails on any head behind main
- 2026-10-24 · process · #102 · VER-063 reports each pull request's stages, raw and in the maintainer's active time, and the process-created waits, compared with fixed thresholds and the previous period in a "Recommendations input" section (wording accepted as drafted)
- 2026-10-24 · process · #102 · Agents end every GitHub comment or review with the line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`, so that metrics can tell the maintainer's words from theirs (AGENTS.md)
- 2026-10-24 · process · #102 · Pushes and workflow dispatches do not count as the maintainer's activity in VER-063's active time; the thresholds of `xtask/src/metrics/waits.rs` stay as built for a few nightly reports, then are tuned
- 2026-10-24 · process · chat · Standing practice: no commit only to name a pull request in a plan row (name the branch, or add the number with other pending changes), and a pull request's decision-log lines are committed together, just before its final verification run
- 2026-10-24 · api · chat · D2 amended: field codes 1–106 stay; each new crate gets a reserved block of 50 codes when its plan is accepted (aldis-cop1 150–199, aldis-link 200–249), and fields shared by several crates take the next code of the common block 107–149; cargo xtask fields checks the blocks
- 2026-10-24 · process · chat · docs/process/decisions.md merges with merge=union (.gitattributes)
- 2026-10-24 · process · chat · VER-063 reports the share of pull requests, commits, and verify minutes spent on process rather than product, and counts the bookkeeping-only commits
- 2026-10-24 · spec · chat · RS-021: state machines make invalid states and transitions unrepresentable; typestate always where the caller's call sequence decides the next state, and a closed enum with a transition function generated from or checked against the state table, exhaustive and without a wildcard, where an external event decides it (AGENTS.md)
- 2026-10-24 · plan · chat · M2 WP-7 adds a mechanical RS-020 check in `cargo xtask` and a property test (and, where feasible, a nightly Kani harness) for every restricted type; the M3 row of the roadmap carries RS-021 for COP-1
- 2026-10-24 · api · #109 · C ABI decoders that need the channel table (TC, AOS, USLP) have no _with pair; a validation reason crosses as a uint32_t *reason out-parameter; delimiters are stateless with a unit and offset (c-abi.md §11)
- 2026-10-24 · api · #109 · C ABI channel shapes: TC passes a master channel with VC and MAP arrays; AOS and TM carry the SCID only in the master channel; USLP, which has no master-channel type, passes a physical channel with flat SCID-tagged VC and MAP arrays (c-abi.md §11)
- 2026-10-24 · api · #109 · AOS: one aldis_aos_writer_finish for data and OID frames, one ALDIS_AOS_POINTER_* set for M_PDU and B_PDU, the edition as a uint8_t; USLP: one aldis_uslp_frame_t with a truncated tag
- 2026-10-24 · api · #109 · New public items TcFrame::peek_header and aldis_uslp::encode_oid_galois; the writers' resume for TC, AOS, and USLP under the #94 rule; capacities [tc] max_maps = 64, [uslp] max_vcs = 16 and max_maps = 32
- 2026-10-24 · process · chat · Standing rule: before opening pull requests, the coordinator evaluates whether pending branches of the same kind that touch overlapping or generated files can be combined into one, and says what it decided; the TC, AOS, and USLP C ABI modules go in one pull request
- 2026-10-24 · spec · chat · TV-023: an independent model is written blind, by an author who has not read the implementation it checks (AGENTS.md)
- 2026-10-24 · plan · chat · M2 WP-7 adds three gates: a worst-case stack bound per target (extends VER-011), loop-bound consistency between code and inspection records (GEN-022), and bit-flip completeness for every codec with an FECF; M2 WP-8 adds an assurance case and a hazard and failure-mode analysis
- 2026-10-24 · plan · chat · Signed verification evidence (records and release tags) is started now, on its own branch
- 2026-10-24 · plan · chat · Signed verification evidence is parked (#107 closed, branch kept, issue #110); revisit with the assurance case at the M2 exit
- 2026-10-24 · spec · chat · GEN-015: Aldis assumes the memory holding its state between calls is protected against single-event upsets, and docs/hardware-assumptions.md states the memory risk without it (an invalid enum discriminant is undefined behaviour; RS-020 values are not checked again) and the integrator's part; the M2-exit hazard analysis includes the upset, and M3 planning decides how to harden engine state
