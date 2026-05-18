//! Phase 1 verify gate: pack the fixture set, run the SFX under headless
//! DOSBox-X at `cputype=8086`, and confirm the extracted files match the
//! originals byte-for-byte.
//!
//! `#[ignore]`-gated so `cargo test` works on contributors who don't have
//! `dosbox-x` installed. CI's `dosbox-x-integration` job runs it via
//! `cargo test -- --ignored`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

/// Hard cap on how long DOSBox-X is allowed to run. The stub processes
/// 4 tiny fixtures inside an 8086 emulator; even at 4.77 MHz this is
/// well under a second. Two minutes is generous enough that runner
/// flake won't trip it, tight enough that a hung child fails fast
/// instead of stalling until the GH Actions job-level timeout.
const DOSBOX_TIMEOUT: Duration = Duration::from_secs(120);

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is the host crate (`host/`); repo root is one up.
    let host = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    host.parent().expect("host has a parent").to_path_buf()
}

fn fixtures() -> &'static [&'static str] {
    &["hello.txt", "numbers.txt", "random.bin", "empty.bin"]
}

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn extracts_fixtures_under_8086_cputype() {
    let root = repo_root();
    let fixtures_dir = root.join("tests").join("fixtures");

    // dosbox-x mounts a host directory as drive C. We point everything
    // (the SFX, the autoexec.bat that runs it, the extracted files) into
    // this single tempdir so the diff at the end is trivial.
    let work = tempfile::tempdir().expect("create tempdir");
    let work_path = work.path();

    let sfx_path = work_path.join("OUT.EXE");
    let inputs: Vec<PathBuf> = fixtures().iter().map(|f| fixtures_dir.join(f)).collect();

    // Use the binary cargo built for this test crate so we don't depend
    // on a separate `cargo run` invocation (which would recompile under
    // a different profile and may not be in $PATH).
    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .args(&inputs)
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "doskrunch pack failed: {status:?}");

    // dosbox-x.conf: pin CPU to real 8086, give it enough conventional
    // RAM for the 16 KB scratch + DOS overhead, mount the tempdir as C:,
    // run the SFX, then exit.
    let conf_path = work_path.join("dosbox.conf");
    fs::write(
        &conf_path,
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
            mount = work_path.display(),
        ),
    )
    .expect("write dosbox.conf");

    let mut dosbox = Command::new("dosbox-x")
        .arg("-conf")
        .arg(&conf_path)
        .arg("-exit")
        .arg("-nogui")
        // SDL needs a display target; the dummy video driver keeps it
        // happy under a headless GH Actions runner.
        .env("SDL_VIDEODRIVER", "dummy")
        .spawn()
        .expect("spawn dosbox-x (is it installed?)");
    let dosbox_status = wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT)
        .expect("dosbox-x timed out and was killed");
    assert!(
        dosbox_status.success(),
        "dosbox-x exited non-zero: {dosbox_status:?}",
    );

    // DOSBox-X mounts case-insensitively but writes uppercase 8.3 names
    // on the host. Our fixtures are already lowercase 8.3, so the host
    // mangler emits the same name uppercased; check that.
    for fixture in fixtures() {
        let original = fs::read(fixtures_dir.join(fixture))
            .unwrap_or_else(|e| panic!("read fixture {fixture}: {e}"));
        let extracted_name = fixture.to_ascii_uppercase();
        let extracted = locate_case_insensitive(work_path, &extracted_name)
            .unwrap_or_else(|| panic!("missing extracted file: {extracted_name}"));
        let body = fs::read(&extracted)
            .unwrap_or_else(|e| panic!("read extracted {}: {e}", extracted.display()));
        assert_eq!(
            body, original,
            "extracted {} differs from fixture {}",
            extracted.display(),
            fixture
        );
    }
}

/// Poll `try_wait` until the child exits or `timeout` elapses. On
/// timeout, send SIGKILL and reap so the test fails loudly instead of
/// hanging until GitHub's job-level cap. Returns `Some(status)` on a
/// clean exit, `None` if the child had to be killed.
fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(Duration::from_millis(200));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn locate_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Some(entry.path());
        }
    }
    None
}
