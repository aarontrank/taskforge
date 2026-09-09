//! Bakes the build's provenance into the binary: which commit it came from, where that checkout
//! is, and when it was built.
//!
//! This exists because an installed binary was eight commits behind its source for a week and
//! nothing could say so — `--version` read `0.2.0` from both, so features that were written,
//! tested and committed had simply never run. A version number alone cannot detect that: it only
//! moves when someone remembers to move it, and the same forgetting is what caused the problem.
//! The commit is not a substitute for the version, it is what makes the version checkable.
//!
//! Everything here degrades to `unknown` rather than failing the build. Building from a release
//! tarball with no `git` and no `.git` is legitimate; refusing to compile for it would be worse
//! than not knowing the commit.

use std::process::Command;

fn main() {
    let root = repo_root();

    // Without these the hash is baked once and then goes stale inside the binary — the very
    // failure this file exists to detect, reintroduced one level down. `HEAD` alone is not
    // enough: an ordinary commit moves the *branch ref* and leaves `HEAD` reading
    // `ref: refs/heads/main` untouched, so watching only HEAD misses every commit that is not a
    // checkout.
    if let Some(git_dir) = root.as_ref().map(|r| r.join(".git")) {
        rerun_if_changed(&git_dir.join("HEAD"));
        // Loose ref for the current branch, and packed-refs for when it has been packed away.
        if let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD")) {
            if let Some(reference) = head.strip_prefix("ref: ") {
                rerun_if_changed(&git_dir.join(reference.trim()));
            }
        }
        rerun_if_changed(&git_dir.join("packed-refs"));
    }

    let commit = git(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let source = root
        .map(|r| r.display().to_string())
        .unwrap_or_else(|| "unknown".into());

    println!("cargo:rustc-env=TASKFORGE_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=TASKFORGE_SOURCE_DIR={source}");
    println!("cargo:rustc-env=TASKFORGE_BUILD_EPOCH={}", build_epoch());
}

fn rerun_if_changed(p: &std::path::Path) {
    if p.exists() {
        println!("cargo:rerun-if-changed={}", p.display());
    }
}

/// The checkout this build came from, so `doctor` knows where to look for newer commits.
fn repo_root() -> Option<std::path::PathBuf> {
    git(&["rev-parse", "--show-toplevel"]).map(std::path::PathBuf::from)
}

/// Run git in the crate directory, or `None` if it is unavailable or reports failure.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Seconds since the epoch, formatted at runtime rather than here — a build script has no
/// business pulling in a date-formatting dependency for one string.
///
/// `SOURCE_DATE_EPOCH` wins when set, so a reproducible build stays reproducible.
fn build_epoch() -> u64 {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    if let Ok(v) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(n) = v.parse() {
            return n;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
