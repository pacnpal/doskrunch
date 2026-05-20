//! Phase 4 verify gate (PLAN.md §10): the source mtime's UTC
//! broken-down components survive the pack → DOSBox-X → host
//! `fs::metadata` round-trip as the extracted file's LOCAL broken-down
//! components. The two instants are NOT equal in non-UTC environments —
//! DOS treats FAT dates as wall-clock LOCAL with no timezone concept,
//! while pack-side `unix_to_fat` decomposes the UTC mtime, so an
//! unshifted round-trip lands with extracted-LOCAL = source-UTC. This
//! is the FAT-2-second-resolution invariant PLAN.md asks for, expressed
//! in the reference frame the OS actually uses.
//!
//! The pack-time path (`host/src/pack.rs::pack` ⇒
//! `fat_time::unix_to_fat` when `--preserve-timestamps` is set) and the
//! stub-side path (`stubs/src/stub.c::main` ⇒ `_dos_setftime` when
//! the per-file timestamp is nonzero) both shipped in Phase 2. This
//! gate is the end-to-end DOSBox-X verification that the two halves
//! match. The gate doesn't depend on the depacker variant, so pinning
//! `cputype=8086` is enough.
//!
//! Two cases:
//!   1. A known, fixed source mtime (deliberately *not* "now") round-
//!      trips through pack → DOSBox-X → host fs::metadata. Comparison
//!      is on the LOCAL broken-down components (year/month/day/
//!      hour/min/sec÷2), not on raw Unix-epoch seconds: DOS interprets
//!      FAT dates as LOCAL with no timezone concept, so the extracted
//!      file's Unix mtime is shifted by the host's local-time offset.
//!      Decomposing via `libc::localtime_r` lets us assert exact
//!      equality on the FAT components while tolerating any
//!      world timezone.
//!   2. A pre-1980 source mtime is clamped by `fat_time::unix_to_fat`
//!      to the 1980-01-01 FAT epoch endpoint — a non-zero timestamp
//!      that the stub WILL `_dos_setftime` to. The extracted file's
//!      LOCAL broken-down components should be exactly
//!      (1980, 1, 1, 0, 0, 0) — defends the clamp end-to-end against
//!      regressions in either `fat_time` or the stub's per-file
//!      timestamp wiring.
//!
//! Source-file layout: each test creates its source file in a
//! separate `srcdir` outside the DOSBox-X-mounted run directory. If
//! we left the source alongside the extracted file, the case-
//! insensitive lookup `locate_case_insensitive(rundir, "STAMPED.TXT")`
//! could match the original `stamped.txt` on a case-sensitive host
//! filesystem and accidentally pass without actually verifying what
//! the stub wrote.
//!
//! Unix-only: `libc::localtime_r` is the lowest-friction way to get
//! LOCAL broken-down components; the DOSBox-X tests are unix-only in
//! practice anyway. `#[ignore]`-gated so contributors without
//! `dosbox-x` aren't blocked; runs in CI's `dosbox-x-integration`
//! job via `cargo test -- --ignored`.

#![cfg(unix)]

use std::fs;
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use filetime::{set_file_mtime, FileTime};

mod common;
use common::{locate_case_insensitive, wait_with_timeout, WaitError};

const DOSBOX_TIMEOUT: Duration = Duration::from_secs(120);

/// Fixed, deterministic source mtime — chosen as a value that is
/// (a) on a FAT 2-second boundary (so truncation is a no-op) and
/// (b) well clear of 1980/2107 boundaries so the round-trip can't
/// accidentally land on a clamp. 2024-05-16 12:34:56 UTC =
/// 1715862896 — same instant the `fat_time::tests::known_timestamp`
/// unit test pins, so a regression in fat_time would also fail this
/// gate. 1715862896 % 2 == 0, so no FAT truncation slop either.
const PINNED_MTIME_SECS: i64 = 1_715_862_896;

/// LOCAL broken-down components, mirroring `struct tm` minus the
/// fields we don't compare (wday, yday, isdst, gmtoff, zone).
#[derive(Debug, PartialEq, Eq)]
struct LocalParts {
    year: i32,
    month: u32, // 1..=12
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
}

/// Decompose a Unix-epoch second count into local broken-down time
/// via libc's reentrant localtime_r. Returns None if libc rejects
/// the value (overflow on time_t platforms with 32-bit time_t).
fn local_parts(unix_secs: i64) -> Option<LocalParts> {
    // `libc::time_t` is 64-bit on the unix tier-1/tier-2 Rust targets
    // (x86_64, aarch64-{linux,darwin}). It's still 32-bit on some
    // tier-3 unix targets (e.g. some 32-bit BSD or musl variants), but
    // every fixed test instant here (PINNED_MTIME_SECS, the FAT epoch)
    // fits in a 32-bit signed time_t, so the narrowing cast is safe in
    // practice. The cast avoids clippy's useless_conversion lint on
    // 64-bit targets where time_t is already i64.
    let t: libc::time_t = unix_secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::localtime_r(&t, &mut tm) };
    if ok.is_null() {
        return None;
    }
    Some(LocalParts {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        min: tm.tm_min as u32,
        sec: tm.tm_sec as u32,
    })
}

/// Decompose a Unix-epoch second count into UTC broken-down time via
/// libc's reentrant gmtime_r. Used to derive the expected LOCAL
/// components of the extracted file independently of `unix_to_fat`:
/// because DOS treats FAT timestamps as LOCAL (no timezone concept)
/// and `unix_to_fat` decomposes UTC unconditionally, a round-trip that
/// preserves the source mtime correctly should land with the
/// extracted file's LOCAL components equal to the SOURCE's UTC
/// components. Using gmtime_r as the oracle catches a pack-side bug
/// that accidentally applies a timezone shift (which would still
/// agree with `fat_parts(unix_to_fat(...))`).
fn utc_parts(unix_secs: i64) -> Option<LocalParts> {
    let t: libc::time_t = unix_secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::gmtime_r(&t, &mut tm) };
    if ok.is_null() {
        return None;
    }
    Some(LocalParts {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        min: tm.tm_min as u32,
        sec: tm.tm_sec as u32,
    })
}

/// One run of `dosbox-x` against the SFX in `rundir_path`, panicking on
/// timeout/non-zero exit. Caller is responsible for laying out the
/// `dosbox.conf` and putting the SFX in place.
fn run_dosbox(rundir_path: &std::path::Path, conf_path: &std::path::Path, tag: &str) {
    let mut dosbox = Command::new("dosbox-x")
        .arg("-conf")
        .arg(conf_path)
        .arg("-exit")
        .arg("-nogui")
        .env("SDL_VIDEODRIVER", "dummy")
        .current_dir(rundir_path)
        .spawn()
        .expect("spawn dosbox-x");
    let status = match wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT) {
        Ok(s) => s,
        Err(WaitError::Timeout) => {
            panic!("dosbox-x did not exit within {DOSBOX_TIMEOUT:?} ({tag}); child was killed")
        }
        Err(WaitError::Wait(e)) => {
            panic!("waiting on dosbox-x failed: {e} ({tag}); child was killed")
        }
    };
    assert!(
        status.success(),
        "dosbox-x exited non-zero ({tag}): {status:?}"
    );
}

fn write_dosbox_conf(path: &std::path::Path, mount: &std::path::Path) {
    fs::write(
        path,
        format!(
            concat!(
                "[cpu]\n",
                "cputype=8086\n",
                "core=normal\n",
                "cycles=max\n",
                "[dosbox]\n",
                "memsize=4\n",
                "[sdl]\n",
                "output=surface\n",
                "[autoexec]\n",
                "mount c \"{mount}\"\n",
                "c:\n",
                "OUT.EXE\n",
                "exit\n",
            ),
            mount = mount.display(),
        ),
    )
    .expect("write dosbox.conf");
}

/// Pin TZ to a fixed non-UTC value for the duration of the test
/// process so `localtime_r` (and DOSBox-X, which inherits TZ) interpret
/// FAT dates as Eastern time. Without this, CI runners that default
/// to UTC would have `localtime_r` and `gmtime_r` return the same
/// components — and a pack-side regression that accidentally uses
/// LOCAL instead of UTC during `unix_to_fat` would slip past both
/// timestamp gates. America/New_York has DST so the offset varies,
/// which is fine: the invariant is "extracted LOCAL == source UTC
/// modulo whatever offset", and the test compares broken-down
/// components after the same TZ pass.
///
/// `std::env::set_var` is `unsafe` since Rust 1.78 — concurrent libc
/// env reads in OTHER tests in the same binary can race with our
/// mutation. `cargo test` runs tests in a binary in parallel by
/// default; both timestamp tests call `pin_test_tz`, so we serialize
/// them with `TZ_LOCK` to keep the env-mutation interval bounded by
/// the test function's scope.
/// `tzset()` isn't bound in the `libc` crate, so declare the extern
/// inline — it's a POSIX function on every relevant target.
static TZ_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn pin_test_tz() -> std::sync::MutexGuard<'static, ()> {
    // Take the lock for the duration of the test. Dropping the
    // guard at test exit doesn't unset TZ (no need — every
    // timestamp-sensitive test in this file goes through
    // pin_test_tz and gets the same value), it just lets the next
    // serialized test proceed without overlapping our
    // set_var/tzset window.
    extern "C" {
        fn tzset();
    }
    let guard = TZ_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    unsafe {
        std::env::set_var("TZ", "America/New_York");
        tzset();
    }
    guard
}

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn preserves_pinned_source_mtime_through_dos_extraction() {
    let _tz_guard = pin_test_tz();
    // Two complementary checks:
    //   1. Pack-side: re-open the archive bytes with `inspect` and
    //      confirm the stored per-file timestamp equals
    //      `unix_to_fat(PINNED_MTIME_SECS)`. This is the exact-bytes
    //      gate that the host wrote what we asked it to.
    //   2. Stub-side: pack → run under DOSBox-X → decompose the
    //      extracted file's mtime into LOCAL broken-down components
    //      and compare them exactly to the FAT components the host
    //      wrote into the archive. This catches off-by-day, wrong-
    //      time-of-day, or lost-FAT-2s-truncation regressions that a
    //      coarse "within 24 h" check would miss.
    //
    // Source file lives in `srcdir`, OUTSIDE the DOSBox-X mount, so
    // the case-insensitive lookup below can't accidentally match the
    // original `stamped.txt` instead of the extracted `STAMPED.TXT`.
    let srcdir = tempfile::tempdir().expect("create srcdir");
    let rundir = tempfile::tempdir().expect("create rundir");
    let rundir_path = rundir.path();

    let src = srcdir.path().join("stamped.txt");
    fs::write(&src, b"stamped content\n").expect("write source");
    set_file_mtime(&src, FileTime::from_unix_time(PINNED_MTIME_SECS, 0)).expect("set source mtime");

    let sfx_path = rundir_path.join("OUT.EXE");
    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .arg(&src)
        .args([
            "--algo",
            "aplib",
            "--target",
            "8086",
            "--preserve-timestamps",
        ])
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "pack failed: {status:?}");

    let archive = doskrunch::unpack::load_archive(&sfx_path).expect("load archive for inspect");
    let entry = archive
        .files
        .iter()
        .find(|f| f.display_name().eq_ignore_ascii_case("STAMPED.TXT"))
        .expect("STAMPED.TXT in archive");
    let want_fat_packed = doskrunch::fat_time::unix_to_fat(PINNED_MTIME_SECS);
    assert_eq!(
        entry.timestamp, want_fat_packed,
        "archive timestamp {:#010x} doesn't match expected FAT-encoded \
         pinned mtime {:#010x}",
        entry.timestamp, want_fat_packed
    );

    // Stub-side check: round-trip through DOSBox-X, then assert the
    // extracted file's LOCAL broken-down time matches an INDEPENDENT
    // oracle: the SOURCE mtime's UTC broken-down components (via
    // libc::gmtime_r). The end-to-end invariant is that DOS treats
    // FAT timestamps as LOCAL with no timezone concept, and pack-side
    // unix_to_fat decomposes the source mtime as UTC; therefore an
    // unshifted round-trip lands with extracted LOCAL == source UTC.
    // Using gmtime_r as the oracle (not fat_parts(unix_to_fat(...)))
    // means a pack-side bug that accidentally applies a TZ shift
    // would be caught here — the archive's stored value would agree
    // with the shifted oracle but disagree with the unshifted one.
    let conf_path = rundir_path.join("dosbox.conf");
    write_dosbox_conf(&conf_path, rundir_path);
    run_dosbox(rundir_path, &conf_path, "pinned-mtime");

    let extracted =
        locate_case_insensitive(rundir_path, "STAMPED.TXT").expect("missing STAMPED.TXT");
    // Byte-identity check: defends the timestamp gate against
    // accepting a content regression that happens to preserve the
    // mtime. Per the repo coding guideline "all dosbox_*.rs gates
    // must assert byte-identical extraction".
    let extracted_body = fs::read(&extracted).expect("read extracted");
    let source_body = fs::read(&src).expect("read source");
    assert_eq!(
        extracted_body, source_body,
        "extracted body differs from source — content regression masked by a correct mtime"
    );
    let got_mtime = fs::metadata(&extracted)
        .expect("stat extracted")
        .modified()
        .expect("mtime");
    let got_unix = got_mtime
        .duration_since(UNIX_EPOCH)
        .expect("post-epoch")
        .as_secs() as i64;

    let got_local = local_parts(got_unix).expect("decompose extracted mtime via localtime_r");
    // Independent oracle: decompose the SOURCE mtime as UTC. Not
    // routed through unix_to_fat / fat_parts, so a shift bug there
    // can't make the assertion tautological.
    let want_local =
        utc_parts(PINNED_MTIME_SECS).expect("decompose source mtime as UTC via gmtime_r");

    assert_eq!(
        got_local, want_local,
        "extracted LOCAL broken-down time doesn't match source UTC components \
         (extracted_unix={got_unix})",
    );
}

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn pre_1980_source_mtime_clamps_exactly_to_fat_epoch_endpoint() {
    let _tz_guard = pin_test_tz();
    // Source mtime: 1979-06-15 17:42:00 UTC — deliberately NOT
    // Jan 1 / midnight so we actually verify the clamp zeroes
    // month/day/time as well as the year. fat_time::unix_to_fat
    // clamps the whole instant to 1980-01-01 00:00:00 and returns a
    // NON-ZERO packed timestamp — so the stub calls _dos_setftime —
    // and the extracted file's LOCAL broken-down time must equal
    // (1980, 1, 1, 0, 0, 0) exactly. A year-only clamp regression in
    // fat_time would produce (1980, 6, 15, 17, 42, 0) and fail this
    // gate. The stub-side wiring is verified end-to-end with no ±24 h
    // slack.
    //
    // The "zero-skip" path on the stub side (`if (dos_date != 0 ||
    // dos_time != 0)`) isn't exercised under --preserve-timestamps
    // because fat_time::unix_to_fat returns a non-zero value for any
    // input (clamped to >=1980). The zero-skip path runs in the
    // default reproducible-mode pack where the timestamp field is
    // explicitly set to 0; that path is exercised implicitly by
    // every other dosbox_* gate. Keep this test focused on the
    // pre-1980 clamp end-to-end.
    //
    // Same srcdir-vs-rundir layout as the pinned test — see the
    // module docstring for why.
    let srcdir = tempfile::tempdir().expect("create srcdir");
    let rundir = tempfile::tempdir().expect("create rundir");
    let rundir_path = rundir.path();

    // 1979-06-15 17:42:00 UTC = 298_316_520. Non-Jan-1, non-midnight —
    // distinguishes a true endpoint clamp from a year-only clamp.
    let src = srcdir.path().join("old.txt");
    fs::write(&src, b"old\n").expect("write source");
    set_file_mtime(&src, FileTime::from_unix_time(298_316_520, 0)).expect("set source mtime");

    let sfx_path = rundir_path.join("OUT.EXE");
    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .arg(&src)
        .args([
            "--algo",
            "aplib",
            "--target",
            "8086",
            "--preserve-timestamps",
        ])
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "pack failed: {status:?}");

    let conf_path = rundir_path.join("dosbox.conf");
    write_dosbox_conf(&conf_path, rundir_path);
    run_dosbox(rundir_path, &conf_path, "pre-1980-mtime");

    let extracted = locate_case_insensitive(rundir_path, "OLD.TXT").expect("missing OLD.TXT");
    // Byte-identity check (see the pinned-mtime test above for the
    // rationale).
    let extracted_body = fs::read(&extracted).expect("read extracted");
    let source_body = fs::read(&src).expect("read source");
    assert_eq!(
        extracted_body, source_body,
        "extracted body differs from source — content regression masked by a correct mtime"
    );
    let got_mtime = fs::metadata(&extracted)
        .expect("stat extracted")
        .modified()
        .expect("mtime");
    let got_unix = got_mtime
        .duration_since(UNIX_EPOCH)
        .expect("post-epoch")
        .as_secs() as i64;

    let got_local = local_parts(got_unix).expect("decompose extracted mtime via localtime_r");
    let want_local = LocalParts {
        year: 1980,
        month: 1,
        day: 1,
        hour: 0,
        min: 0,
        sec: 0,
    };
    assert_eq!(
        got_local, want_local,
        "extracted LOCAL broken-down time doesn't match the FAT epoch endpoint \
         (extracted_unix={got_unix}); pre-1980 clamp path may be broken",
    );
}
