### Changed
- `cargo xtask verify`: a change to the GitHub configuration that no check reads (the workflows,
  the Rust setup action, the C++ setup action's definition, the pull-request template,
  `CODEOWNERS`, and `dependabot.yml`) runs only `spec`, as documentation does, instead of the Rust
  checks or every check (VER-056).
