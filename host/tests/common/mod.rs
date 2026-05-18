//! Shared helpers for the DOSBox-X integration test files.
//!
//! Each `host/tests/dosbox_*.rs` file is its own integration-test binary,
//! so anything they share has to live in a `mod common;` they include
//! manually. The Cargo convention of putting that under `tests/common/mod.rs`
//! avoids the directory being treated as another test binary.
//!
//! Phase 3 had 6 dosbox_*.rs files plus benchmark_tiers.rs, all repeating
//! the same WaitError / wait_with_timeout / locate_case_insensitive trio.
//! Phase 4 adds three more dosbox files (`dosbox_2mb_memsize2`,
//! `dosbox_timestamps`, `dosbox_stored_max_chunk`) for a total of 9
//! dosbox callers + 1 benchmark caller — well past the threshold where
//! the duplication starts costing more than the indirection. Each
//! caller still picks its own DOSBOX_TIMEOUT and writes its own
//! `dosbox.conf` inline, because those vary per test.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

/// Reason `wait_with_timeout` had to kill the child. Distinguishing
/// timeout from a `try_wait` syscall failure lets the caller surface a
/// precise panic message in CI logs. Per the Phase 3 PR review thread,
/// always match both arms separately — never collapse to `Err(_)`.
#[allow(dead_code)]
pub enum WaitError {
    Timeout,
    Wait(std::io::Error),
}

/// Poll `try_wait` until the child exits or `timeout` elapses. On
/// timeout or `try_wait` error, send SIGKILL and reap so the test
/// fails fast instead of hanging until GitHub's job-level cap.
#[allow(dead_code)]
pub fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Result<ExitStatus, WaitError> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(WaitError::Timeout);
                }
                thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(WaitError::Wait(e));
            }
        }
    }
}

/// DOSBox-X writes uppercase 8.3 names on the host; some host
/// filesystems (HFS+ case-insensitive) keep the original case. Look
/// up a file in `dir` matching `name` regardless of case.
#[allow(dead_code)]
pub fn locate_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            return Some(entry.path());
        }
    }
    None
}

/// Path to the repo root (one level up from `host/`).
#[allow(dead_code)]
pub fn repo_root() -> PathBuf {
    let host = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    host.parent().expect("host has a parent").to_path_buf()
}
