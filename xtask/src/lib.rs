// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod mount;

use anyhow::Result;
use sha2::Digest;
use sha2::Sha256;
use std::fs::File;
use std::io;
use std::path::Path;

pub use mount::{Mount, ReadOnly};

/// Calculate the SHA256 hash of the file at `path`.
///
/// This calculates the hash incrementally, so large files are not
/// loaded into memory all at once.
pub fn calc_file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(format!("{hash:x}"))
}
