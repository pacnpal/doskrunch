#![no_main]
//! Fuzz the DKCH archive parser. Success = no panic, no unbounded
//! allocation, no UB. Every parse error is acceptable; that's the
//! parser's job.

use std::io::Cursor;

use doskrunch::archive::Archive;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut cur = Cursor::new(data);
    let _ = Archive::read(&mut cur);
});
