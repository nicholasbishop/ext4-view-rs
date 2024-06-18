// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use anyhow::{bail, Result};
use ext4_view::Ext4;
use sha2::{Digest, Sha256};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::{self, Command};

/// Summary of a file's contents.
///
/// For regular files this is the SHA256 hash of the file's
/// contents. For symlinks, the target path. For directories no
/// additional data is stored.
#[derive(Eq, PartialEq, Ord, PartialOrd)]
pub enum FileContent {
    Dir,
    /// Symlink target.
    Symlink(PathBuf),
    /// SHA256 hash of a regular file's contents.
    Regular(String),
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
pub struct WalkDirEntry {
    pub path: PathBuf,
    pub content: FileContent,
    pub mode: u16,
}

impl WalkDirEntry {
    pub fn format(&self) -> Vec<u8> {
        let mut output = self.path.as_os_str().as_bytes().to_vec();

        output.push(b' ');
        output.extend(format!("{:o}", self.mode).as_bytes());
        output.push(b' ');

        match &self.content {
            FileContent::Dir => output.extend(b"dir"),
            FileContent::Symlink(target) => {
                output.extend(b"symlink=");
                output.extend(target.as_os_str().as_bytes());
            }
            FileContent::Regular(hash) => {
                output.extend(b"file sha256=");
                output.extend(hash.as_bytes());
            }
        }
        output
    }
}

fn new_dir_entry(
    fs: &Ext4,
    dir_entry: ext4_view::DirEntry,
) -> Result<WalkDirEntry> {
    let metadata = fs.metadata(&dir_entry)?;

    let content = if metadata.is_symlink() {
        let target = fs.read_link(&dir_entry)?;
        FileContent::Symlink(target.into())
    } else if metadata.is_dir() {
        FileContent::Dir
    } else {
        let data = fs.read(&dir_entry)?;
        let hash = format!("{:x}", Sha256::digest(data));
        FileContent::Regular(hash)
    };
    Ok(WalkDirEntry {
        path: dir_entry.path().into(),
        content,
        mode: metadata.mode(),
    })
}

fn walk_with_lib(
    fs: &Ext4,
    path: ext4_view::Path<'_>,
) -> Result<Vec<WalkDirEntry>> {
    let mut output = Vec::new();

    output.push(WalkDirEntry {
        path: ext4_view::PathBuf::from(path).into(),
        content: FileContent::Dir,
        mode: fs.metadata("/")?.mode(),
    });

    for entry in fs.read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }

        // TODO: use DirEntry::file_type once that exists.
        if fs.metadata(&entry)?.is_dir() {
            output.extend(walk_with_lib(fs, path.as_path())?);
        } else {
            output.push(new_dir_entry(fs, entry)?);
        }
    }

    Ok(output)
}

pub fn diff_walk(path: &Path) -> Result<()> {
    // Build `mount_and_walk` in release mode.
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--package",
            "xtask",
            "--bin",
            "mount_and_walk",
        ])
        .status()?;
    if !status.success() {
        bail!("failed to build mount_and_walk");
    }

    let actual = {
        let ext4 = Ext4::load_from_path(path)?;
        let mut paths = walk_with_lib(&ext4, ext4_view::Path::ROOT)?;
        paths.sort_unstable();
        paths
            .iter()
            .map(|dir_entry| dir_entry.format())
            .collect::<Vec<_>>()
    };
    let expected = {
        let output = Command::new("sudo")
            .arg("target/release/mount_and_walk")
            .arg(path)
            .output()?;
        assert!(output.status.success());
        let mut lines = output
            .stdout
            .split(|c| *c == b'\n')
            .map(|l| l.to_vec())
            .collect::<Vec<_>>();

        // Remove the empty line at the end of the file.
        let last = lines.pop().unwrap();
        assert!(last.is_empty());

        lines
    };

    for (a, b) in actual.iter().zip(expected.iter()) {
        if a != b {
            println!(
                "{} != {}",
                String::from_utf8_lossy(a),
                String::from_utf8_lossy(b)
            );
            process::exit(1);
        }
    }

    assert_eq!(actual.len(), expected.len());

    println!("success, no differences detected");
    Ok(())
}
