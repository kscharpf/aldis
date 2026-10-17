//! `xtask links`: relative Markdown links must resolve inside the repository.

use crate::repo;

/// Checks every relative link in every Markdown file; returns the number of problems.
pub fn check() -> Result<usize, String> {
    let root = repo::root();
    let mut problems = Vec::new();
    let mut count = 0;
    for path in repo::files(".", &["md"]) {
        let text = repo::read(&path)?;
        let dir = path.parent().unwrap_or(&root).to_path_buf();
        for (n, line) in text.lines().enumerate() {
            let mut rest = line;
            while let Some(start) = rest.find("](") {
                let after = &rest[start + 2..];
                let Some(end) = after.find(')') else { break };
                let target = &after[..end];
                rest = &after[end..];
                if target.is_empty()
                    || target.starts_with('#')
                    || target.contains("://")
                    || target.starts_with("mailto:")
                {
                    continue;
                }
                count += 1;
                let file = target.split('#').next().unwrap_or_default();
                let resolved = dir.join(file);
                let normalized = normalize(&resolved);
                let at = format!("{}:{}", repo::rel(&path), n + 1);
                if !normalized.starts_with(&root) {
                    problems.push(format!("{at}: link leaves the repository: {target}"));
                } else if repo::rel(&normalized).starts_with("specs/pdf/") {
                    // Local-only CCSDS documents (specs/README.md); not present in CI.
                } else if !normalized.exists() {
                    problems.push(format!("{at}: broken link: {target}"));
                }
            }
        }
    }
    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "links: {count} relative link(s), {} problem(s)",
        problems.len()
    );
    Ok(problems.len())
}

/// Resolves `.` and `..` lexically (the target may not exist).
fn normalize(path: &std::path::Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}
