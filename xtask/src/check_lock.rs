//! The lock of `xtask check`: one run at a time per worktree (VER-069).
//!
//! A second run in the same worktree would overlap the first one's builds and mix their output.
//! The lock is a symlink `xtask-check.lock` in the worktree's own git directory, so that another
//! worktree is not held up. Its target is `<pid>|<start time>` of the holder, the way
//! `scripts/reverify` keeps its locks: creating it is one atomic step, and a pid that was reused
//! has another start time, so a lock left by a killed run is stale and the next run takes it over.
//! Taking over goes through a second symlink, `xtask-check.lock.take`, so that two takers cannot
//! both win.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::repo;

/// Holds the lock until it is dropped.
pub struct Guard {
    lock: PathBuf,
    me: String,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if target(&self.lock).as_deref() == Some(self.me.as_str()) {
            drop(fs::remove_file(&self.lock));
        }
    }
}

/// Takes the lock of this worktree, or fails naming the run that holds it.
pub fn acquire() -> Result<Guard, String> {
    let out = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-dir"])
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err("git rev-parse --git-dir failed".into());
    }
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    acquire_in(
        &Path::new(&dir).join("xtask-check.lock"),
        std::process::id(),
        &proc_start,
    )
}

/// The start time of process `pid`, `None` when it is gone: with its pid, the identity of a holder.
fn proc_start(pid: u32) -> Option<String> {
    // The locale and zone are fixed, so that the same process always prints the same start time.
    let out = Command::new("ps")
        .env("TZ", "UTC")
        .env("LC_ALL", "C")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let start = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !start.is_empty()).then_some(start)
}

fn target(link: &Path) -> Option<String> {
    fs::read_link(link)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Whether the process named by a lock target (`<pid>|<start>`) is still the holder.
fn alive(holder: &str, start_of: &dyn Fn(u32) -> Option<String>) -> bool {
    holder
        .split_once('|')
        .and_then(|(pid, start)| Some((pid.parse().ok()?, start)))
        .is_some_and(|(pid, start): (u32, &str)| start_of(pid).as_deref() == Some(start))
}

fn acquire_in(
    lock: &Path,
    pid: u32,
    start_of: &dyn Fn(u32) -> Option<String>,
) -> Result<Guard, String> {
    let start = start_of(pid).ok_or("cannot read the start time of this process with `ps`")?;
    let me = format!("{pid}|{start}");
    let mut take = OsString::from(lock);
    take.push(".take");
    let take = PathBuf::from(take);
    // Each pass takes the lock, returns an error, or removes a stale lock (or a stale taker's
    // mutex) and goes round again.
    for _ in 0..16 {
        match symlink(&me, lock) {
            Ok(()) => {
                return Ok(Guard {
                    lock: lock.to_path_buf(),
                    me,
                });
            }
            Err(e) if e.kind() != ErrorKind::AlreadyExists => {
                return Err(format!("{}: {e}", lock.display()));
            }
            Err(_) => {}
        }
        let Some(holder) = target(lock) else {
            continue;
        };
        if alive(&holder, start_of) {
            let (pid, since) = holder.split_once('|').unwrap_or((&holder, ""));
            return Err(format!(
                "another `cargo xtask check` is running in this worktree (pid {pid}, started \
                 {since}); wait for it to end instead of starting a second one"
            ));
        }
        if symlink(&me, &take).is_ok() {
            // Between this check and the removal a third run could have taken the lock over and
            // taken it again, and this run would then remove that run's lock. That needs three
            // runs at once after a crash, and the cost is one more run that proceeds beside
            // another in the same worktree, as before the lock existed; a lock file or `flock`
            // would close it, and is not worth a dependency here.
            if target(lock).as_deref() == Some(holder.as_str()) {
                println!(
                    "check: the lock of pid {} is stale; taking it over",
                    holder.split('|').next().unwrap_or("?")
                );
                drop(fs::remove_file(lock));
            }
            drop(fs::remove_file(&take));
        } else {
            if target(&take).is_some_and(|t| !alive(&t, start_of)) {
                drop(fs::remove_file(&take));
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
    Err(format!("could not take the lock {}", lock.display()))
}

#[cfg(test)]
mod tests {
    use super::{acquire_in, alive, target};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    /// A lock path in a fresh directory.
    fn lock() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xtask-check-lock-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("xtask-check.lock")
    }

    /// Every process is alive and started at `t<pid>`, except pid 99.
    fn live(pid: u32) -> Option<String> {
        (pid != 99).then(|| format!("t{pid}"))
    }

    #[test]
    fn a_second_run_is_refused_naming_the_holder() {
        // req: VER-069
        let l = lock();
        let first = acquire_in(&l, 1, &live).unwrap();
        let err = acquire_in(&l, 2, &live).err().unwrap();
        assert!(err.contains("pid 1") && err.contains("started t1"), "{err}");
        drop(first);
    }

    #[test]
    fn dropping_the_guard_frees_the_lock() {
        // req: VER-069
        let l = lock();
        drop(acquire_in(&l, 1, &live).unwrap());
        assert_eq!(target(&l), None);
        assert!(acquire_in(&l, 2, &live).is_ok());
    }

    #[test]
    fn a_lock_of_a_dead_process_is_taken_over() {
        // req: VER-069
        let l = lock();
        // Pid 99 is gone; it left its lock behind.
        std::os::unix::fs::symlink("99|t99", &l).unwrap();
        let g = acquire_in(&l, 2, &live).unwrap();
        assert_eq!(target(&l).as_deref(), Some("2|t2"));
        drop(g);
        // A reused pid (another start time) is stale as well.
        std::os::unix::fs::symlink("1|earlier", &l).unwrap();
        assert!(acquire_in(&l, 2, &live).is_ok());
    }

    #[test]
    fn a_guard_does_not_remove_a_lock_it_no_longer_owns() {
        // req: VER-069
        let l = lock();
        let g = acquire_in(&l, 1, &live).unwrap();
        std::fs::remove_file(&l).unwrap();
        std::os::unix::fs::symlink("5|t5", &l).unwrap();
        drop(g);
        assert_eq!(target(&l).as_deref(), Some("5|t5"));
    }

    #[test]
    fn a_malformed_holder_is_not_alive() {
        // req: VER-069
        assert!(!alive("", &live));
        assert!(!alive("x|t1", &live));
        assert!(!alive("1", &live));
        assert!(alive("1|t1", &live));
    }
}
