# Rust lint rationale

This file is the rationale for human readers. An implementing agent does not need it: Clippy enforces the lints, and [AGENTS.md](../../AGENTS.md) and the [Rust profile](rust.md) §5 hold the rules an agent follows. The lints themselves are `[workspace.lints]` in the root `Cargo.toml`, the lint table of `crates/aldis-ffi/Cargo.toml`, and the lint tables of the separate workspaces (the tooling profile of RS-017). The tables below record why each restriction lint and nursery lint is denied, deferred, or not adopted (RS-016 to RS-019).

The `cargo` group is denied at the lowest priority in every lint table. The lints below the restriction block are decided one by one; `undocumented_unsafe_blocks` and `too_many_lines` were already denied. The tooling profile of `xtask` takes the same lints (`else_if_without_else` is `allow` there, the maintainer's decision),; `fuzz/`, `crosscheck/`, `aldis-testkit`, and the reference application's tooling crates keep the tooling profile of RS-017 without them. A lint is relaxed only as RS-016 says, with `#[expect(clippy::<lint>, reason = "…")]` on the smallest item; `allow_attributes` makes any `#[allow]` an error, so `#[expect]` is the only form left.

| Restriction lint | Decision | Reason |
|---|---|---|
| `cargo` (group) | Denied | Manifest hygiene (metadata, wildcard dependency versions, duplicate versions) fails the build instead of a review. No finding when adopted. |
| `allow_attributes` | Denied | RS-016 in code: `#[expect]` fails when the exception becomes unnecessary, `#[allow]` never does. |
| `wildcard_enum_match_arm` | Denied | RS-021: a `match` on an enum names every variant, so a variant added later is a compile error at every match that must decide it. Where the arm is right, `#[expect]` on the smallest item with the reason: a foreign `#[non_exhaustive]` enum (`ErrorDetail`, `DiscardReason` in `aldis-ffi`), or a test that looks at one variant of a JSON value. A test that wants one variant of an enum writes `if let` rather than a `match` with a wildcard. The lint does not look through a wrapper: a wildcard over the enum inside an `Option` or `Result` is not flagged and rests on review. |
| `unnecessary_safety_doc` | Denied | A `# Safety` section belongs to `unsafe fn`, so it stays true to the signature. |
| `unnecessary_safety_comment` | Denied | A `// SAFETY:` comment is a claim about an `unsafe` block (GEN-070). On a `safe fn` in an `unsafe extern` block, or on `#[unsafe(no_mangle)]`, it is not, and a comment that reads like one dilutes the real ones; those places say it in other words. |
| `unnecessary_rest_pattern` | Denied | A `..` that matches nothing hides that a pattern names every field. |
| `shadow_same` | Denied | `let x = x;` and `let mut x = x;` add nothing a `mut` on the first binding or a new name does not: the name of the value keeps one meaning in a scope. |
| `excessive_nesting` | Denied, threshold 6 | `excessive-nesting-threshold` in `clippy.toml`, which every workspace reads. Measured on the code base, all targets and workspaces, counted per block: threshold 4 reported 232, 5 reported 62, 6 reported 15. Six is the smallest value that leaves a small number of real findings: exhaustive parameter sweeps in tests, which carry `#[expect]` with that reason, and a few loops split into helpers. |
| `else_if_without_else` | Denied; `allow` in `xtask` | An `else if` chain ends in an `else` that says what the remaining case is (RS-021); a chain that has none either returns early or gets an `else` with a comment. `xtask` is the tooling profile, which the maintainer chose to leave out. |
| `unwrap_in_result` | Not adopted | It reports `unwrap` in test helpers (functions outside `#[test]` that return `Result` or `Option`, in integration-test crates): about 70 findings with `allow-unwrap-in-tests` set, in the helpers of a dozen integration-test crates. `clippy.toml` has no option that exempts them, and an `#[expect]` per helper is the noise RS-018 avoids. Library code is covered by `unwrap_used` and `panic_in_result_fn`. |
| `shadow_unrelated` | Not adopted | Maintainer's decision: not adopted, so that a value refined from another may keep its name (RS-020). |
| `absolute_paths` | Not adopted | Maintainer's decision: not adopted, so that a path used once may be written in full instead of imported. |

The Clippy `nursery` group holds lints that are still under development and may report false positives, so it is not enabled as a group. Individual nursery lints are adopted once they have been checked against this code base (RS-019):

| Nursery lint | Decision | Reason |
|---|---|---|
| `missing_const_for_fn` | Denied | Extends the `const` constructors of RS-010 and RS-012: functions are `const fn` wherever the language allows. If a function can be `const` only for some feature combinations, it stays non-`const` and carries a `cfg_attr`-gated `#[expect]`, because constness must not depend on a feature. |
| `derive_partial_eq_without_eq` | Denied | Types with total equality derive `Eq`, so callers can use them as keys and in `assert_eq!` of containing types. |
| `too_long_first_doc_paragraph` | Denied | rustdoc shows the first paragraph as the item summary, so it is one short sentence. |
| `redundant_clone` | Denied | A clone whose original is never used again is dead work. |
| `fallible_impl_from` | Denied | `From` must be infallible (RS-010, GEN-020). Fallible conversions implement `TryFrom`. |
| `non_send_fields_in_send_ty` | Denied | Prevents manual `Send` implementations on types whose fields are not `Send` (RS-013, GEN-033). |
| `transmute_undefined_repr` | Denied | Transmuting types without defined representation (`#[repr(C)]`) is immediate undefined behavior (GEN-070, CABI-004). |
| `debug_assert_with_mut_call` | Denied | Mutating calls inside `debug_assert!` diverge between debug and release builds, violating determinism (GEN-032). |
| `needless_pass_by_ref_mut` | Denied | Avoids unnecessary `&mut` parameters that restrict caller concurrency without mutating state. |
| `large_stack_frames` | Denied | Embedded bare-metal targets (Cortex-M, SPARC LEON3) have strictly bounded stack sizes (GEN-011, VER-010). |
| `collection_is_never_read` | Denied | Containers populated but never read waste storage and cycles (GEN-011). |
| `redundant_pub_crate` | Not adopted | Conflicts with `unreachable_pub`: in a private module it asks for `pub`, which `unreachable_pub` then reports. |
| `or_fun_call` | Not adopted | The arguments it flags are `const fn` error constructors with no side effects that return a small `Copy` value (RS-011). Eager construction costs nothing, and a closure adds a coverage region (VER-024). |
| `option_if_let_else` | Not adopted | The suggested `map_or_else` takes closures, which a `const fn` cannot call, so it conflicts with `missing_const_for_fn`. Elsewhere it does not improve readability. |
| `use_self` | Deferred | Style only. The code mostly names the type rather than writing `Self`. Adopting it is a separate, mechanical change. |
| `suspicious_operation_groupings` | Not adopted | False positives on contiguous interval validation chains. |
| `trailing_empty_array` | Not adopted | Flags zero-sized C ABI storage handle types (`aldis_oid_generator_t`). |
| `useless_let_if_seq` | Not adopted | Flags multi-stage conditional state machine parsing in `aldis-spp`. |
