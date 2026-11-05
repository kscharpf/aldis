//! `cargo xtask layouts`: the bit layout of each frame and packet header, derived from the
//! encoders and decoders of the codec crates and written to `docs/reference/header-layouts.md`
//! (VER-072).
//!
//! The probe ([`probe`]) encodes a header, changes one field at a time, and records which bits
//! change; [`headers`] lists the fields to probe; [`render`] writes the Markdown. `cargo xtask
//! layouts` fails when the file differs from a fresh derivation, and `--write` regenerates it.

mod headers;
mod probe;
mod render;

use crate::repo;

/// The generated document.
pub const DOC: &str = "docs/reference/header-layouts.md";

/// The document the current codecs give, or the first header the probe cannot lay out.
fn generate() -> Result<String, String> {
    let layouts = headers::all()
        .iter()
        .map(probe::probe)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(render::document(&layouts))
}

/// Checks that the document is current, or writes it (VER-072); returns the number of problems.
pub fn run(write: bool) -> Result<usize, String> {
    let generated = generate()?;
    let path = repo::root().join(DOC);
    if write {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", repo::rel(dir)))?;
        }
        std::fs::write(&path, &generated).map_err(|e| format!("{DOC}: {e}"))?;
        println!("wrote {DOC}");
        return Ok(0);
    }
    let current = std::fs::read_to_string(&path).ok();
    let problems = usize::from(current.as_deref() != Some(generated.as_str()));
    if problems > 0 {
        println!(
            "error: {DOC} differs from the header layouts of the codecs; run `cargo xtask layouts --write`"
        );
    }
    println!("layouts: {problems} problem(s)");
    Ok(problems)
}
