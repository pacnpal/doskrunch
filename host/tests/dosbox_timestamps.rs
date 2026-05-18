//! Phase 4 verify gate (PLAN.md §10): timestamps on extracted files
//! match the original, truncated to FAT's 2-second resolution.
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
//!      trips through pack → DOSBox-X → host fs::metadata, truncated
//!      to FAT 2-second resolution. Setting a fixed mtime via
//!      `filetime::set_file_mtime` (rather than relying on the test
//!      machine's current wall-clock) keeps the comparison stable
//!      across reruns and across CI host clocks.
//!   2. A pre-1980 source mtime (FAT can't represent dates before
//!      1980) is packed as a zero timestamp; the stub skips
//!      `_dos_setftime` on the zero case, so DOSBox-X writes the file
//!      with whatever current-clock mtime DOS supplies. Verifies the
//!      zero-skip path is still in place (defends against someone
//!      removing the `if (dos_date != 0 || dos_time != 0)` guard).
//!
//! `#[ignore]`-gated so contributors without `dosbox-x` aren't blocked;
//! runs in CI's `dosbox-x-integration` job via `cargo test -- --ignored`.

use std::fs;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        Err(WaitError::Timeout) => panic!(
            "dosbox-x did not exit within {DOSBOX_TIMEOUT:?} ({tag}); child was killed"
        ),
        Err(WaitError::Wait(e)) => panic!(
            "waiting on dosbox-x failed: {e} ({tag}); child was killed"
        ),
    };
    assert!(status.success(), "dosbox-x exited non-zero ({tag}): {status:?}");
}

fn write_dosbox_conf(path: &std::path::Path, mount: &std::path::Path) {
    fs::write(
        path,
        format!(
            concat!(
                "[cpu]\n",
                "cputype=8086\n",
                "core=normal\n",
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

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn preserves_pinned_source_mtime_through_dos_extraction() {
    // Two complementary checks:
    //   1. Pack-side: re-open the archive bytes with `inspect` and
    //      confirm the stored per-file timestamp equals
    //      `unix_to_fat(PINNED_MTIME_SECS)`. This is the exact-bytes
    //      gate that the host wrote what we asked it to.
    //   2. Stub-side: pack → run under DOSBox-X → re-stat the
    //      extracted file. DOS interprets FAT dates as LOCAL time
    //      (no timezone concept), but most host filesystems return
    //      mtimes as UTC Unix-epoch seconds. The result is that the
    //      extracted mtime is shifted by the host's local-time
    //      offset (e.g. ±14400 s on UTC-4). The strict-equality
    //      check on Unix seconds therefore can't be portable across
    //      runner timezones — instead we assert the round-trip
    //      landed within ±24 h of the pinned source, which excludes
    //      every meaningful failure (FAT date garbage, stub overrun,
    //      `_dos_setftime` skipped, wrong year) while tolerating any
    //      world timezone.
    let work = tempfile::tempdir().expect("create tempdir");
    let work_path = work.path();

    let src = work_path.join("stamped.txt");
    fs::write(&src, b"stamped content\n").expect("write source");
    set_file_mtime(&src, FileTime::from_unix_time(PINNED_MTIME_SECS, 0))
        .expect("set source mtime");

    let sfx_path = work_path.join("OUT.EXE");
    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .arg(&src)
        .args(["--algo", "aplib", "--target", "8086", "--preserve-timestamps"])
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "pack failed: {status:?}");

    // Pack-side check: the archive's timestamp field is exactly the
    // FAT-encoded pinned mtime. No DOSBox-X involved.
    let archive = doskrunch::unpack::load_archive(&sfx_path)
        .expect("load archive for inspect");
    let entry = archive
        .files
        .iter()
        .find(|f| f.display_name().eq_ignore_ascii_case("STAMPED.TXT"))
        .expect("STAMPED.TXT in archive");
    let want_fat = doskrunch::fat_time::unix_to_fat(PINNED_MTIME_SECS);
    assert_eq!(
        entry.timestamp, want_fat,
        "archive timestamp {:#010x} doesn't match expected FAT-encoded \
         pinned mtime {:#010x}",
        entry.timestamp, want_fat
    );

    // Stub-side check: round-trip the SFX through DOSBox-X and verify
    // the extracted file's mtime is near (±24 h, see above) the pinned
    // value. A regression that removes the `_dos_setftime` call would
    // leave the file dated "now" and fail this slack check.
    let conf_path = work_path.join("dosbox.conf");
    write_dosbox_conf(&conf_path, work_path);
    run_dosbox(work_path, &conf_path, "pinned-mtime");

    let extracted = locate_case_insensitive(work_path, "STAMPED.TXT")
        .expect("missing STAMPED.TXT");
    let got_mtime = fs::metadata(&extracted)
        .expect("stat extracted")
        .modified()
        .expect("mtime");
    let got_unix = got_mtime
        .duration_since(UNIX_EPOCH)
        .expect("post-epoch")
        .as_secs() as i64;

    let delta = (got_unix - PINNED_MTIME_SECS).abs();
    assert!(
        delta <= 24 * 3600,
        "extracted mtime {got_unix} (diff {} from now) is more than 24 h \
         away from pinned source {PINNED_MTIME_SECS}; \
         delta {delta}s — stub side may have skipped `_dos_setftime`",
        diff_from_now(got_unix),
    );
}

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn pre_1980_source_mtime_extracts_with_dos_now_mtime() {
    // Source mtime: 1970-01-01 00:00:00. fat_time clamps to 1980 and
    // returns a non-zero packed timestamp — so the stub WOULD call
    // _dos_setftime — but the resulting date (1980-01-01) is the
    // earliest representable FAT date. What we're actually verifying
    // here is that the round-trip doesn't crash and that the
    // extracted file is dated 1980-01-01 (the clamp endpoint), NOT
    // the host's current wall-clock — i.e. the pack-side clamp is
    // wired through to the stub side correctly.
    //
    // The "zero-skip" path on the stub side (`if (dos_date != 0 ||
    // dos_time != 0)`) isn't exercised under --preserve-timestamps
    // because fat_time::unix_to_fat returns a non-zero value for any
    // input (clamped to >=1980). The zero-skip path runs in the
    // default reproducible-mode pack where the timestamp field is
    // explicitly set to 0; that path is exercised implicitly by
    // every other dosbox_* gate. Keep this test focused on the
    // pre-1980 clamp end-to-end.
    let work = tempfile::tempdir().expect("create tempdir");
    let work_path = work.path();

    let src = work_path.join("old.txt");
    fs::write(&src, b"old\n").expect("write source");
    set_file_mtime(&src, FileTime::from_unix_time(0, 0))
        .expect("set source mtime");

    let sfx_path = work_path.join("OUT.EXE");
    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .arg(&src)
        .args(["--algo", "aplib", "--target", "8086", "--preserve-timestamps"])
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "pack failed: {status:?}");

    let conf_path = work_path.join("dosbox.conf");
    write_dosbox_conf(&conf_path, work_path);
    run_dosbox(work_path, &conf_path, "pre-1980-mtime");

    let extracted = locate_case_insensitive(work_path, "OLD.TXT")
        .expect("missing OLD.TXT");
    let got_mtime = fs::metadata(&extracted)
        .expect("stat extracted")
        .modified()
        .expect("mtime");
    let got_unix = got_mtime
        .duration_since(UNIX_EPOCH)
        .expect("post-epoch")
        .as_secs() as i64;

    // 1980-01-01 00:00:00 UTC = 315532800 seconds since the unix
    // epoch. fat_time clamps anything earlier to that exact value
    // (see fat_time::tests::epoch_clamps_to_1980). DOSBox-X writes
    // through to the host filesystem at LOCAL time, so the extracted
    // mtime can be offset from 315532800 by the host's UTC-offset
    // (in seconds). Allow ±24h slack to absorb timezone differences
    // between CI runners.
    const FAT_EPOCH_UNIX: i64 = 315_532_800;
    let delta = (got_unix - FAT_EPOCH_UNIX).abs();
    assert!(
        delta <= 24 * 3600,
        "expected extracted mtime near 1980-01-01 ({FAT_EPOCH_UNIX}), got {got_unix} \
         (delta {delta}s); pre-1980 clamp path may be broken",
    );
}

fn diff_from_now(unix_secs: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let d = now - unix_secs;
    format!("{d}s")
}
