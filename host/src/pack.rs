use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::archive::{
    build_aplib_entry, build_stored_entry, flags, Algorithm, Archive, TargetTier, APLIB_CHUNK_INPUT,
};
use crate::fat_time::unix_to_fat;
use crate::name83::{dedupe, mangle};
use crate::stubs::stub_for;

pub struct PackOptions {
    pub output: PathBuf,
    pub inputs: Vec<PathBuf>,
    pub algorithm: Algorithm,
    pub target: TargetTier,
    pub preserve_timestamps: bool,
    /// Max bytes per uncompressed chunk. The per-algorithm ceilings
    /// are: aplib ≤ `APLIB_CHUNK_INPUT` (16 KiB, the stub's BSS scratch
    /// size); stored ≤ `u16::MAX` (the per-chunk size field). Today
    /// pack reads each input file fully into memory before encoding,
    /// so `chunk_size` does NOT bound peak host RAM during pack — it
    /// controls archive layout (how many chunks per file, per-chunk
    /// framing overhead) and the transient encode buffer. The default
    /// matches the stub's BSS budget; smaller values exist mostly for
    /// testing chunked decode paths.
    pub chunk_size: usize,
}

pub fn pack(opts: PackOptions) -> Result<()> {
    match opts.algorithm {
        Algorithm::Stored | Algorithm::Aplib => {}
        Algorithm::Lzsa2 => bail!("algorithm 'lzsa2' lands in phase 6"),
        Algorithm::Lzma => {
            bail!("algorithm 'lzma' lands in phase 5 (and will require --target 386+ when enabled)")
        }
    }

    // The CLI layer enforces the same ceiling; assert here so library
    // callers (and any future test that calls pack() directly) catch
    // bad values before they reach the chunk encoder.
    let chunk_max = match opts.algorithm {
        Algorithm::Aplib => APLIB_CHUNK_INPUT,
        // Stored chunks are bounded by the per-chunk u16 size field.
        Algorithm::Stored => u16::MAX as usize,
        _ => unreachable!(),
    };
    if !(1..=chunk_max).contains(&opts.chunk_size) {
        bail!(
            "chunk_size {} is outside the valid range 1..={} for algorithm '{}'",
            opts.chunk_size,
            chunk_max,
            opts.algorithm.name()
        );
    }

    let stub = stub_for(opts.algorithm, opts.target).map_err(|e| anyhow::anyhow!(e))?;
    if stub.len() < 2 || &stub[..2] != b"MZ" {
        bail!(
            "embedded stub blob for ({}, {}) is not a DOS .EXE (missing MZ magic) — \
             this build was made before the Watcom stub was produced; \
             build it via `docker run --rm -v \"$PWD/stubs:/work\" -w /work doskrunch-watcom make all` \
             and commit the result to `stubs/blobs/`.",
            opts.algorithm.name(),
            opts.target.name()
        );
    }
    // Detect the committed placeholder (256 bytes; MZ header + zero
    // padding) so users don't ship a non-runnable .EXE without
    // noticing. A real Watcom-built stub is several KB and has
    // executable bytes beyond the MZ header. `.get` keeps this
    // defensive against truncated blobs that still start with `MZ`.
    if stub.len() <= 512
        && stub
            .get(28..)
            .map(|tail| tail.iter().all(|&b| b == 0))
            .unwrap_or(true)
    {
        eprintln!(
            "warning: stub blob for ({}, {}) looks like the placeholder \
             (MZ header + zero padding). The resulting .EXE will not run \
             on DOS until the Watcom-built blob replaces stubs/blobs/.",
            opts.algorithm.name(),
            opts.target.name()
        );
    }

    let mut archive = Archive::new(opts.algorithm, opts.target);
    if !opts.preserve_timestamps {
        archive.flags |= flags::REPRODUCIBLE;
    }

    // Expand any directory inputs into the regular files they contain.
    // Symlinks are not followed (avoids cycles and surprise inclusion of
    // files outside the named tree). Hidden / dotfile names are kept —
    // DOS has no leading-dot convention, so a `.gitignore` named on
    // purpose is intended. The output path is excluded so re-running
    // `doskrunch pack dir/OUT.EXE dir/` doesn't pack the previous SFX
    // into the new SFX (which would also break rerun determinism).
    let exclude = canonicalize_for_exclude(&opts.output);
    let expanded = expand_inputs(&opts.inputs, exclude.as_deref())?;
    if expanded.len() > u16::MAX as usize {
        bail!(
            "too many input files ({}); archive header file_count is u16",
            expanded.len()
        );
    }

    // Pass 1: stat + mangle. Collect (mangled, source path, metadata) before
    // any dedupe so we can reorder deterministically.
    let mut prelim: Vec<(String, PathBuf, fs::Metadata)> = Vec::with_capacity(expanded.len());
    for src in &expanded {
        // `symlink_metadata` (not `fs::metadata`) so a TOCTOU race where
        // something replaces a walked file with a symlink between
        // expand_inputs and this stat is caught here — we documented
        // "symlinks not followed" as a hard boundary in the CLI help, so
        // honour that even under concurrent modification of the input
        // tree. A real attacker still has a tiny window before
        // `fs::read` below, but using symlink_metadata closes the
        // larger one.
        let meta = fs::symlink_metadata(src).with_context(|| format!("stat {}", src.display()))?;
        if meta.file_type().is_symlink() {
            bail!(
                "{}: replaced by a symlink between walk and stat (TOCTOU); refusing to follow",
                src.display()
            );
        }
        if !meta.is_file() {
            bail!("{}: no longer a regular file", src.display());
        }
        let src_name = src
            .file_name()
            .and_then(|n| n.to_str())
            .with_context(|| format!("non-utf8 filename: {}", src.display()))?;
        let (mangled, _was_mangled) = mangle(src_name);
        if is_dos_reserved(&mangled) {
            bail!(
                "{}: mangles to '{}', a reserved DOS device name; \
                 rename the source file (e.g. add a prefix) and retry.",
                src.display(),
                mangled
            );
        }
        prelim.push((mangled, src.clone(), meta));
    }

    // Reproducible default: sort by mangled name BEFORE dedupe so two
    // invocations with the same set of inputs (any argv order) resolve
    // collisions identically. Tie-break on the source path so distinct
    // sources that mangle to the same name still order deterministically.
    if !opts.preserve_timestamps {
        prelim.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    }

    let mut used: HashSet<String> = HashSet::new();
    let mut entries: Vec<(String, PathBuf, fs::Metadata)> = Vec::with_capacity(prelim.len());
    for (mangled, src, meta) in prelim {
        let src_name = src.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let final_name = dedupe(&mangled, &used).with_context(|| {
            format!(
                "{}: exhausted the ~1..~9999 suffix space for stem '{}'",
                src.display(),
                mangled
            )
        })?;
        if final_name != src_name.to_ascii_uppercase() {
            eprintln!(
                "warning: '{}' stored as '{}' (8.3 mangling)",
                src_name, final_name
            );
        }
        used.insert(final_name.clone());
        entries.push((final_name, src, meta));
    }

    for (stored_name, src, meta) in entries {
        let data = read_no_follow(&src).with_context(|| format!("read {}", src.display()))?;
        let timestamp = if opts.preserve_timestamps {
            let mtime_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            unix_to_fat(mtime_secs)
        } else {
            0
        };
        if data.len() > u32::MAX as usize {
            bail!("{}: file exceeds 4 GiB", src.display());
        }
        let entry = match opts.algorithm {
            Algorithm::Stored => {
                build_stored_entry(&stored_name, 0x20, timestamp, &data, opts.chunk_size)
            }
            Algorithm::Aplib => {
                build_aplib_entry(&stored_name, 0x20, timestamp, &data, opts.chunk_size)
            }
            // Lzsa2/Lzma rejected earlier; unreachable here.
            other => bail!("internal: unexpected algorithm {}", other.name()),
        }
        .map_err(|e| anyhow::anyhow!("{}: {}", src.display(), e))?;
        archive.files.push(entry);
    }

    // Cumulative size must fit in the u32 header fields. DOS lseek is
    // signed 32-bit, so the whole .EXE must also fit in 2 GiB (i32::MAX).
    let mut total_u: u64 = 0;
    let mut total_c: u64 = 0;
    for f in &archive.files {
        total_u += f.uncompressed_size() as u64;
        total_c += f.compressed_size() as u64;
    }
    if total_u > u32::MAX as u64 || total_c > u32::MAX as u64 {
        bail!(
            "archive payload exceeds 4 GiB (uncompressed {} / compressed {})",
            total_u,
            total_c
        );
    }
    // i32::MAX accounts for the stub + archive + trailer; checked again
    // in write_sfx once we know the exact archive byte size.
    write_sfx(&opts.output, stub, &archive)?;
    Ok(())
}

/// Expand `inputs` into a flat list of regular files. Directory inputs
/// are walked recursively; symlinks (whether passed as top-level
/// inputs or encountered during the walk) are skipped to avoid cycles
/// and to keep the included set within the named tree. `exclude`, if
/// provided, is a canonical path that should be omitted from the
/// walk — used to skip the pack's own output file when it sits inside
/// a walked directory.
///
/// Walk order: each `read_dir` result is sorted by path before recursion
/// so the final list is identical across hosts (HFS+/ext4/NTFS all
/// return directory entries in arbitrary order). The pack pipeline then
/// re-sorts by mangled 8.3 name before dedupe, which is what guarantees
/// reproducible output bytes — but pre-sorting here keeps the walk
/// itself deterministic, which matters for any caller that wants to
/// reason about pack ordering without relying on that downstream sort.
fn expand_inputs(inputs: &[PathBuf], exclude: Option<&Path>) -> Result<Vec<PathBuf>> {
    let mut out = Vec::with_capacity(inputs.len());
    for top in inputs {
        // `symlink_metadata` so a symlinked top-level input is skipped
        // rather than silently dereferenced. Matches walk-time symlink
        // handling; documented behaviour in pack's CLI help.
        let meta = fs::symlink_metadata(top).with_context(|| format!("stat {}", top.display()))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        // Top-level input matching the output path is a clear user
        // error (asking us to read what we're about to overwrite).
        // Bail loudly rather than silently dropping it — the silent
        // exclusion is reserved for files discovered during a
        // directory walk where the user reasonably expected the
        // walker to skip the output file.
        if matches_exclude(top, exclude) {
            bail!(
                "{}: input path is the same file as the output (--output); refusing to pack the output into itself",
                top.display()
            );
        }
        if meta.is_file() {
            out.push(top.clone());
        } else if meta.is_dir() {
            walk_dir(top, &mut out, exclude)?;
        } else {
            bail!(
                "{}: not a regular file or directory (block dev / fifo / socket?)",
                top.display()
            );
        }
    }
    Ok(out)
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>, exclude: Option<&Path>) -> Result<()> {
    // Propagate read_dir iteration errors instead of swallowing them —
    // a transient EACCES / EIO halfway through a directory should fail
    // the pack rather than silently producing an incomplete SFX.
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("iter {}", dir.display()))?;
    entries.sort();
    for path in entries {
        // Symlink check FIRST. `matches_exclude` calls `fs::canonicalize`,
        // which follows symlinks — checking it before the symlink test
        // would dereference any symlinked entry under the walked tree
        // (the canonicalize might also error harmlessly on a dangling
        // symlink, but we'd still have lost the no-follow contract).
        let meta =
            fs::symlink_metadata(&path).with_context(|| format!("stat {}", path.display()))?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            // Skip silently — most workflows have stray symlinks and
            // failing the whole pack on them is noisier than dropping.
            continue;
        }
        if matches_exclude(&path, exclude) {
            continue;
        }
        if ft.is_file() {
            out.push(path);
        } else if ft.is_dir() {
            walk_dir(&path, out, exclude)?;
        }
        // Other file types (block dev, char dev, FIFO, socket) are
        // silently skipped; they don't belong in a DOS SFX.
    }
    Ok(())
}

/// Canonicalize `path` for use as the walk-exclusion key. Returns
/// `None` if the path doesn't yet exist on disk (e.g. the first pack
/// run before the output file is created) — there's nothing to exclude
/// in that case.
fn canonicalize_for_exclude(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

fn matches_exclude(candidate: &Path, exclude: Option<&Path>) -> bool {
    let Some(target) = exclude else {
        return false;
    };
    fs::canonicalize(candidate)
        .map(|c| c == target)
        .unwrap_or(false)
}

/// Read `path`'s entire contents, refusing to follow symlinks even if
/// the path was swapped between the earlier `symlink_metadata` check
/// and this read (a TOCTOU window the bare `fs::read` would lose to).
/// On unix we open with `O_NOFOLLOW` so the open itself fails with
/// ELOOP if the path is now a symlink. On non-unix targets (Windows)
/// we fall through to the plain read — Windows' symlink semantics are
/// different, and the host pack pipeline doesn't have meaningful
/// real-world unix-symlink exposure on those targets.
fn read_no_follow(path: &Path) -> std::io::Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::io::Read as _;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        Ok(data)
    }
    #[cfg(not(unix))]
    {
        fs::read(path)
    }
}

/// Return true if a mangled 8.3 name corresponds to a DOS/Windows
/// reserved device (CON/PRN/AUX/NUL/COM1..9/LPT1..9). Compared on the
/// upper-ASCII stem (everything before the first dot).
fn is_dos_reserved(mangled: &str) -> bool {
    const DOS_RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = mangled.split('.').next().unwrap_or(mangled);
    DOS_RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r))
}

fn write_sfx(out: &Path, stub: &[u8], archive: &Archive) -> Result<()> {
    let mut f = fs::File::create(out).with_context(|| format!("create {}", out.display()))?;
    f.write_all(stub)?;
    let archive_offset: u32 = stub.len().try_into().context("stub larger than 4 GiB")?;
    archive.write(&mut f)?;
    crate::archive::write_trailer(&mut f, archive_offset)?;
    let total = f.metadata()?.len();
    // DOS lseek is signed 32-bit, so the stub can't reach past 2 GiB
    // even though the archive header's u32 fields could express more.
    if total > i32::MAX as u64 {
        bail!("output .EXE is {} bytes; DOS lseek caps at 2 GiB", total);
    }
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_input(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let p = dir.join(name);
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(body).unwrap();
        p
    }

    fn default_opts(td: &Path, inputs: Vec<PathBuf>, algorithm: Algorithm) -> PackOptions {
        PackOptions {
            output: td.join("o.exe"),
            inputs,
            algorithm,
            target: TargetTier::I8086,
            preserve_timestamps: false,
            chunk_size: APLIB_CHUNK_INPUT,
        }
    }

    #[test]
    fn rejects_lzma_on_8086() {
        let td = tempfile::tempdir().unwrap();
        let input = make_input(td.path(), "a.txt", b"x");
        let opts = default_opts(td.path(), vec![input], Algorithm::Lzma);
        let err = pack(opts).unwrap_err();
        assert!(err.to_string().contains("lzma"));
        assert!(err.to_string().contains("phase 5"));
    }

    #[test]
    fn rejects_reserved_device_input_name() {
        let td = tempfile::tempdir().unwrap();
        let input = make_input(td.path(), "con.txt", b"x");
        let opts = default_opts(td.path(), vec![input], Algorithm::Stored);
        let err = pack(opts).unwrap_err();
        assert!(
            err.to_string().contains("reserved DOS device name"),
            "got: {err}"
        );
    }

    #[test]
    fn rejects_zero_chunk_size() {
        let td = tempfile::tempdir().unwrap();
        let input = make_input(td.path(), "a.txt", b"x");
        let mut opts = default_opts(td.path(), vec![input], Algorithm::Aplib);
        opts.chunk_size = 0;
        let err = pack(opts).unwrap_err();
        assert!(err.to_string().contains("chunk_size"));
    }

    #[test]
    fn rejects_chunk_size_above_stub_budget_for_aplib() {
        let td = tempfile::tempdir().unwrap();
        let input = make_input(td.path(), "a.txt", b"x");
        let mut opts = default_opts(td.path(), vec![input], Algorithm::Aplib);
        opts.chunk_size = APLIB_CHUNK_INPUT + 1;
        let err = pack(opts).unwrap_err();
        assert!(err.to_string().contains("chunk_size"));
    }

    #[test]
    fn directory_input_is_walked_recursively() {
        let td = tempfile::tempdir().unwrap();
        // Build:
        //   src/a.txt
        //   src/sub/b.txt
        //   src/sub/c.txt
        let src = td.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        let a = make_input(&src, "a.txt", b"alpha");
        let b = make_input(&src.join("sub"), "b.txt", b"bravo");
        let c = make_input(&src.join("sub"), "c.txt", b"charlie");

        let expanded = expand_inputs(std::slice::from_ref(&src), None).unwrap();
        // expand_inputs preserves walk order (sorted per directory).
        assert_eq!(expanded.len(), 3);
        assert!(expanded.contains(&a));
        assert!(expanded.contains(&b));
        assert!(expanded.contains(&c));
    }

    #[cfg(unix)]
    #[test]
    fn walk_skips_symlinks() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("src");
        fs::create_dir(&src).unwrap();
        let _real = make_input(&src, "real.txt", b"keep");
        let target = src.join("real.txt");
        let link = src.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let expanded = expand_inputs(std::slice::from_ref(&src), None).unwrap();
        assert_eq!(expanded.len(), 1, "symlink should be skipped");
    }

    #[cfg(unix)]
    #[test]
    fn top_level_symlink_is_skipped() {
        // Matches CLI help: top-level symlink inputs are skipped, not
        // an error — same behaviour as symlinks encountered during a
        // directory walk.
        let td = tempfile::tempdir().unwrap();
        let real = make_input(td.path(), "real.txt", b"data");
        let link = td.path().join("link.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let expanded = expand_inputs(&[link.clone(), real.clone()], None).unwrap();
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0], real);
    }

    #[test]
    fn pack_excludes_its_own_output_inside_walked_directory() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("src");
        fs::create_dir_all(&src).unwrap();
        let real = make_input(&src, "real.txt", b"keep");
        // Pretend an earlier pack already produced OUT.EXE inside src/.
        let out = src.join("OUT.EXE");
        fs::write(&out, b"\x4d\x5afakeexe").unwrap();
        let out_canonical = fs::canonicalize(&out).unwrap();
        let expanded = expand_inputs(std::slice::from_ref(&src), Some(&out_canonical)).unwrap();
        assert_eq!(expanded.len(), 1, "previous OUT.EXE must be skipped");
        assert!(expanded.contains(&real));
    }

    #[cfg(unix)]
    #[test]
    fn read_no_follow_refuses_symlink() {
        // Defends the no-follow boundary against a TOCTOU swap: the
        // input list passed expand_inputs as a regular file, then by
        // the time pack reaches read_no_follow it's a symlink. The
        // read must fail rather than silently follow. open(2) with
        // O_NOFOLLOW returns ELOOP (40 on Linux, 62 on macOS); the
        // exact errno doesn't matter for this gate, only that the
        // open errored instead of returning the symlink target's
        // bytes.
        let td = tempfile::tempdir().unwrap();
        let real = make_input(td.path(), "real.txt", b"keep");
        let link = td.path().join("link.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = read_no_follow(&link).unwrap_err();
        // raw_os_error() is Some for any errno from open(2); a None
        // here would mean we never reached open which is itself a
        // failure of the no-follow boundary.
        assert!(
            err.raw_os_error().is_some(),
            "read_no_follow on a symlink should fail with a syscall-level errno; got: {err}"
        );
    }

    #[test]
    fn pack_bails_when_top_level_input_is_the_output() {
        // Top-level input identical to the output path is a clear
        // user error (asking us to read what we're about to
        // overwrite). Bail loudly — silent drop is for files
        // discovered during a walk, not for files the user named.
        let td = tempfile::tempdir().unwrap();
        let out = td.path().join("OUT.EXE");
        fs::write(&out, b"\x4d\x5afakeexe").unwrap();
        let out_canonical = fs::canonicalize(&out).unwrap();
        let err = expand_inputs(std::slice::from_ref(&out), Some(&out_canonical)).unwrap_err();
        assert!(err.to_string().contains("output into itself"), "got: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn walk_propagates_read_dir_iteration_errors() {
        // We can't easily fault-inject mid-iteration, but we can prove
        // the unreadable-directory case bails instead of returning an
        // empty list. Skip on root because root can read chmod 000
        // dirs and the assertion would spuriously fail; CI containers
        // often run as root.
        use std::os::unix::fs::PermissionsExt;
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("noaccess");
        fs::create_dir(&src).unwrap();
        make_input(&src, "real.txt", b"keep");
        let mut perms = fs::metadata(&src).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&src, perms).unwrap();

        // Probe: if read_dir still succeeds (root, or filesystem
        // ignores chmod), this test can't validate the error-bubbling
        // path on this platform. Restore perms and skip.
        let probe = fs::read_dir(&src).map(|_| ()).err();
        if probe.is_none() {
            let mut perms = fs::metadata(&src).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&src, perms).unwrap();
            eprintln!(
                "walk_propagates_read_dir_iteration_errors: \
                 read_dir on chmod-000 dir still succeeded (likely root); \
                 skipping the assertion"
            );
            return;
        }

        let err = expand_inputs(std::slice::from_ref(&src), None).err();
        // Restore so the tempdir cleanup works regardless of the result.
        let mut perms = fs::metadata(&src).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&src, perms).unwrap();
        assert!(err.is_some(), "EACCES on read_dir must bail the pack");
    }
}
