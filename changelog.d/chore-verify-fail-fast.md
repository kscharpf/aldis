### Changed
- `cargo xtask verify` runs the checks quickest first (gating, then non-gating, by the typical
  duration each declares), and with `--publish` stops at the first gating check that fails:
  such a run cannot publish, so the checks it did not reach are recorded as not run.
  `--no-fail-fast` and `--fail-fast` override the default (docs/06-verification.md §7, VER-056).
- `cargo xtask verify` starts no container, and pulls or builds no image, when the input rule
  skips or reuses every gating check; the record says that no image was used and names the
  hash of the image inputs (VER-057). `verify list` says whether a run would start one.
- `verify.yml` gets and pushes the image within the run (`verify --push-image`), so a run that
  needs no image fetches none (VER-060).
- `cargo xtask ci` also runs `cargo xtask header`, so a stale `include/aldis/aldis.h` fails it
  in seconds; it needs the pinned `cbindgen`. A step that cannot run counts as a problem, and
  the other steps still run.
