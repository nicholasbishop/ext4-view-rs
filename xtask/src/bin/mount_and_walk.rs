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

use anyhow::{Context, Result, bail};
use std::ffi::CString;
use std::io::{self, Write};
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::{env, fs};
use xtask::diff_walk::{FileContent, WalkDirEntry};
use xtask::{Mount, ReadOnly, calc_file_sha256};

/// Check if a directory is encrypted or not.
fn is_encrypted_dir(path: &Path) -> Result<bool> {
    // Output buffer. The `statx` struct can't be directly constructed,
    // so create an uninitialized buffer that will be filled in by
    // calling `statx`.
    let mut statx_buf: MaybeUninit<libc::statx> = MaybeUninit::uninit();

    // The `inode` input is unused when `path` is absolute.
    assert!(path.is_absolute());
    let inode = -1;

    // Convert the path to a null-terminated C string.
    let path = CString::new(path.as_os_str().as_bytes())?;

    // Don't follow symlinks.
    let flags = libc::AT_SYMLINK_NOFOLLOW;

    // Request basic stats.
    let mask = libc::STATX_BASIC_STATS;

    // Call `statx` and return an error if it fails.
    let rc = unsafe {
        libc::statx(inode, path.as_ptr(), flags, mask, statx_buf.as_mut_ptr())
    };
    if rc != 0 {
        bail!("statx failed: {}", io::Error::last_os_error());
    }

    // The call to `statx` succeeded, so the buffer should be valid now.
    let statx = unsafe { statx_buf.assume_init() };

    // Check the attributes to see if the directory is encrypted.
    let is_encrypted = (statx.stx_attributes
        & u64::try_from(libc::STATX_ATTR_ENCRYPTED).unwrap())
        != 0;
    Ok(is_encrypted)
}

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
        mode: mode_from_metadata(&metadata),
        uid: metadata.uid(),
        gid: metadata.gid(),
    })
}

fn mode_from_metadata(metadata: &fs::Metadata) -> u16 {
    // fs::Metadata::mode() returns the full st_mode field which
    // combines file type and permissions. Mask and truncate to just the
    // mode bits.
    let mode = metadata.mode() & 0o7777;
    u16::try_from(mode).unwrap()
}

fn walk_mounted(path: &Path) -> Result<Vec<WalkDirEntry>> {
    assert!(path.is_dir());

    let mut output = Vec::new();

    let metadata = path.symlink_metadata()?;
    output.push(WalkDirEntry {
        path: path.to_path_buf(),
        content: FileContent::Dir,
        mode: mode_from_metadata(&metadata),
        uid: metadata.uid(),
        gid: metadata.gid(),
    });

    if is_encrypted_dir(path)? {
        output[0].content = FileContent::EncryptedDir;
        return Ok(output);
    }

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

    mount.unmount()?;

    Ok(())
}
