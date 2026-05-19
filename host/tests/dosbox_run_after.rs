//! v1.1 verify gate: stub-side INT 21h/4Bh EXEC for `--run-after`.
//!
//! Packs two files — `HELLO.TXT` (content) and `DONE.COM` (a tiny
//! hand-assembled sentinel program) — with `--run-after "DONE.COM"`.
//! After the SFX runs under DOSBox-X:
//!
//!   1. `HELLO.TXT` must be byte-identical to the source (extraction).
//!   2. `DONE.TXT`  must exist (created by `DONE.COM` via INT 21h/3Ch).
//!
//! Three sub-tests cover the three stub families:
//!   * `run_after_aplib_8086`  — aplib stub, 8086 CPU.
//!   * `run_after_aplib_386`   — aplib stub, 386 CPU; also tests the
//!     args-passing path (`--run-after "DONE.COM /S"`).
//!   * `run_after_lzma_386`    — LZMA stub (stub_lzma.c, compact model),
//!     386 CPU, exercises the compact-model EXEC path.
//!
//! All three tests are `#[ignore]`-gated so contributors without
//! `dosbox-x` aren't blocked. Run with:
//!
//!   SDL_VIDEODRIVER=dummy cargo test --test dosbox_run_after -- --ignored

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{locate_case_insensitive, repo_root, wait_with_timeout, WaitError};

const DOSBOX_TIMEOUT: Duration = Duration::from_secs(120);

/// Build a minimal DOS .COM that creates `DONE.TXT` and exits.
///
/// .COM files load at CS:0x0100 in real mode. Layout:
/// ```
///   +00  B4 3C        MOV AH, 3Ch          ; create file (INT 21h)
///   +02  31 C9        XOR CX, CX           ; normal attributes
///   +04  BA 0E 01     MOV DX, 0x010E       ; DS:DX → filename
///   +07  CD 21        INT 21h              ; call DOS: create
///   +09  B8 00 4C     MOV AX, 4C00h        ; exit with code 0
///   +0C  CD 21        INT 21h              ; call DOS: exit
///   +0E  "DONE.TXT\0"                      ; 9 bytes NUL-terminated name
/// ```
/// Total: 23 bytes.
fn make_done_com() -> Vec<u8> {
    // Code is 14 bytes; filename starts at .COM offset 14 = PSP offset 0x010E.
    let fname_offset: u16 = 0x0100 + 14;
    let lo = (fname_offset & 0xFF) as u8;
    let hi = (fname_offset >> 8) as u8;
    let mut com = vec![
        0xB4, 0x3C, // MOV AH, 3Ch
        0x31, 0xC9, // XOR CX, CX
        0xBA, lo, hi, // MOV DX, fname_offset
        0xCD, 0x21, // INT 21h (create)
        0xB8, 0x00, 0x4C, // MOV AX, 4C00h
        0xCD, 0x21, // INT 21h (exit)
    ];
    com.extend_from_slice(b"DONE.TXT\0");
    com
}

/// Core helper: pack HELLO.TXT + DONE.COM with `--run-after <cmd>`,
/// run the SFX under DOSBox-X at `cpu_type`, then assert extraction
/// (HELLO.TXT byte-identical) and EXEC (DONE.TXT exists).
fn run_after_case(algo: &str, target: &str, cpu_type: &str, run_after_cmd: &str) {
    let root = repo_root();
    let work = tempfile::tempdir().expect("create tempdir");
    let work_path = work.path();

    // Write source files to the temp dir so the pack command finds them.
    let hello_src = b"Hello from doskrunch run-after test\n";
    let hello_path = work_path.join("HELLO.TXT");
    let done_path = work_path.join("DONE.COM");
    fs::write(&hello_path, hello_src).expect("write HELLO.TXT");
    fs::write(&done_path, make_done_com()).expect("write DONE.COM");

    // Pack: HELLO.TXT + DONE.COM, with --run-after pointing at DONE.COM.
    let sfx_path = work_path.join("OUT.EXE");
    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .arg(&hello_path)
        .arg(&done_path)
        .args(["--algo", algo, "--target", target])
        .args(["--run-after", run_after_cmd])
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "doskrunch pack failed: {status:?}");

    // Write DOSBox-X config.
    let conf_path = work_path.join("dosbox.conf");
    fs::write(
        &conf_path,
        format!(
            concat!(
                "[cpu]\n",
                "cputype={cpu}\n",
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
            cpu = cpu_type,
            mount = work_path.display(),
        ),
    )
    .expect("write dosbox.conf");

    let mut dosbox = Command::new("dosbox-x")
        .arg("-conf")
        .arg(&conf_path)
        .arg("-exit")
        .arg("-nogui")
        .env("SDL_VIDEODRIVER", "dummy")
        .spawn()
        .expect("spawn dosbox-x (is it installed?)");
    let dosbox_status = match wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT) {
        Ok(s) => s,
        Err(WaitError::Timeout) => {
            panic!("dosbox-x did not exit within {DOSBOX_TIMEOUT:?}; child was killed")
        }
        Err(WaitError::Wait(e)) => panic!("waiting on dosbox-x failed: {e}; child was killed"),
    };
    assert!(
        dosbox_status.success(),
        "dosbox-x exited non-zero: {dosbox_status:?}"
    );

    // 1. Extraction: HELLO.TXT must be byte-identical.
    let extracted_hello = locate_case_insensitive(work_path, "HELLO.TXT")
        .expect("missing extracted HELLO.TXT");
    let body = fs::read(&extracted_hello)
        .unwrap_or_else(|e| panic!("read extracted HELLO.TXT: {e}"));
    assert_eq!(
        body,
        hello_src,
        "extracted HELLO.TXT differs from source ({algo}/{target})"
    );

    // 2. EXEC: DONE.COM must have created DONE.TXT.
    let _ = locate_case_insensitive(work_path, "DONE.TXT").unwrap_or_else(|| {
        panic!(
            "DONE.TXT not created — EXEC of '{run_after_cmd}' \
             did not fire for {algo}/{target}"
        )
    });

    // Tell the root about which repo we're in (unused, but keeps the
    // import live so the helper doesn't get dead-code-warned away).
    let _: PathBuf = root;
}

// ---------------------------------------------------------------------------
// Test cases
// ---------------------------------------------------------------------------

/// aplib/8086: no-args EXEC path.
#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn run_after_aplib_8086() {
    run_after_case("aplib", "8086", "8086", "DONE.COM");
}

/// aplib/386: EXEC with args (`DONE.COM /S`).  The args-passing code
/// path in stub.c — splitting at the first space, building the counted
/// command line — is exercised.  DONE.COM ignores the `/S` argument
/// but the EXEC must still succeed for DONE.TXT to appear.
#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn run_after_aplib_386() {
    run_after_case("aplib", "386", "386", "DONE.COM /S");
}

/// LZMA/386: exercises stub_lzma.c's compact-model EXEC path.
#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn run_after_lzma_386() {
    run_after_case("lzma", "386", "386", "DONE.COM");
}
