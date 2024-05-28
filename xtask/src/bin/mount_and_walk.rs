// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! This executable is intended to be run via the `cargo xtask
//! diff-walk` action, but it can also be run directly:
//!
//!     cargo build --release -p xtask --bin mount_and_walk
//!     sudo target/release/mount_and_walk test_data/test_disk1.bin
//!
//! Expects one argument, the path of a file containing an ext4
//! filesystem.
//!
//! Outputs one line for each file in the filesystem (including
//! directories and symlinks). Each line contains the file's path, mode,
//! and a summary of the file's contents (e.g. symlink target or a
//! SHA-256 hash of a regular file's contents). Example:
//!
//! ```
//! /big_dir 755 dir
//! /big_dir/0 644 file sha256=5feceb66ffc86f38d952786c6d696c79c2dbc239dd4e91b46729d73a27fb57e9
//! ```

use anyhow::{Context, Result};
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::{env, fs};
use xtask::diff_walk::{FileContent, WalkDirEntry};
use xtask::{calc_file_sha256, Mount, ReadOnly};

fn new_dir_entry(dir_entry: fs::DirEntry) -> Result<WalkDirEntry> {
    let metadata = dir_entry.metadata()?;
    let path = dir_entry.path();

    // Test for symlink first, because `is_dir` follows symlinks.
    let content = if metadata.is_symlink() {
        let target = fs::read_link(&path)?;
        FileContent::Symlink(target)
    } else if metadata.is_dir() {
        FileContent::Dir
    } else {
        FileContent::Regular(calc_file_sha256(&path)?)
    };
    Ok(WalkDirEntry {
        path,
        content,
        mode: mode_from_metadata(metadata),
    })
}

fn mode_from_metadata(metadata: fs::Metadata) -> u16 {
    // fs::Metadata::mode() returns the full st_mode field which
    // combines file type and permissions. Mask and truncate to just the
    // mode bits.
    let mode = metadata.mode() & 0o7777;
    u16::try_from(mode).unwrap()
}

fn walk_mounted(path: &Path) -> Result<Vec<WalkDirEntry>> {
    assert!(path.is_dir());

    let mut output = Vec::new();

    output.push(WalkDirEntry {
        path: path.to_path_buf(),
        content: FileContent::Dir,
        mode: mode_from_metadata(path.symlink_metadata()?),
    });

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        if entry.file_type()?.is_dir() {
            output.extend(walk_mounted(&path)?);
        } else {
            output.push(new_dir_entry(entry)?);
        }
    }

    Ok(output)
}

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .context("missing required path argument")?;

    let mount = Mount::new(Path::new(&path), ReadOnly(true))?;
    let mut paths = walk_mounted(mount.path())?;
    paths.sort_unstable();

    for mut dir_entry in paths {
        // Remove the mount point from the beginning of the path. Append
        // that to `/` to make the path absolute again. This makes the
        // output convenient to compare against the library, which
        // produces absolute paths when iterating over directories.
        let path = dir_entry.path.strip_prefix(mount.path())?;
        dir_entry.path = Path::new("/").join(path);

        io::stdout().write_all(&dir_entry.format())?;
        println!();
    }
    Ok(())
}
