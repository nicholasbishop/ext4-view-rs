// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// Note: this file is used as a module in `bench.rs`, but is also used
// via an `include!` in `xtask/uefibench`.

use alloc::string::String;
use alloc::{format, vec};
use ext4_view::{Ext4, Ext4Error, File, Path};
use sha2::{Digest, Sha256};

/// Walk the filesystem and create a SHA256 hash of the paths and file
/// contents.
///
/// Returning a hash ensures that none of the reads can be optimized out.
pub fn walk(fs: &Ext4) -> Result<String, Ext4Error> {
    let mut hash = Sha256::new();
    walk_impl(fs, Path::ROOT, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn walk_impl(
    fs: &Ext4,
    path: Path<'_>,
    hash: &mut Sha256,
) -> Result<(), Ext4Error> {
    let entry_iter = match fs.read_dir(path) {
        Ok(entry_iter) => entry_iter,
        Err(Ext4Error::Encrypted) => {
            eprintln!("ignoring encrypted dir");
            return Ok(());
        }
        Err(err) => return Err(err),
    };

    for entry in entry_iter {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }

        // Hash the path.
        hash.update(&path);

        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            // Hash the symlink target.
            let target = fs.read_link(&path)?;
            hash.update(target);
        } else if file_type.is_dir() {
            // Recurse.
            walk_impl(fs, path.as_path(), hash)?;
        } else {
            // Hash the file contents.
            let file = fs.open(path.as_path())?;
            hash_file(file, hash)?;
        };
    }

    Ok(())
}

/// Read a file in chunks and hash it.
fn hash_file(mut file: File, hash: &mut Sha256) -> Result<(), Ext4Error> {
    let mut chunk = vec![0; 4096];

    loop {
        let bytes_read = file.read_bytes(&mut chunk)?;
        if bytes_read == 0 {
            // End of file reached.
            return Ok(());
        }

        hash.update(&chunk[..bytes_read]);
    }
}
