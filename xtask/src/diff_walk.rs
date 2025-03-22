// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::{capture_cmd, run_cmd, sudo};
use anyhow::{Result, bail};
use ext4_view::{Ext4, Ext4Error};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::File;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::time::SystemTime;

/// Summary of a file's contents.
///
/// For regular files this is the SHA256 hash of the file's
/// contents. For symlinks, the target path. For directories no
/// additional data is stored.
#[derive(Eq, PartialEq, Ord, PartialOrd)]
pub enum FileContent {
    /// Regular directory.
    Dir,
    /// Encrypted directory.
    EncryptedDir,
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
    pub uid: u32,
    pub gid: u32,
}

impl WalkDirEntry {
    pub fn format(&self) -> Vec<u8> {
        let mut output = self.path.as_os_str().as_bytes().to_vec();

        output.push(b' ');
        output.extend(format!("{:o}", self.mode).as_bytes());
        output.push(b' ');
        output.extend(format!("{}:{}", self.uid, self.gid).as_bytes());
        output.push(b' ');

        match &self.content {
            FileContent::Dir => output.extend(b"dir"),
            FileContent::EncryptedDir => output.extend(b"dir encrypted"),
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
    let path = dir_entry.path();
    let metadata = dir_entry.metadata()?;

    let content = if metadata.is_symlink() {
        let target = fs.read_link(&path)?;
        FileContent::Symlink(target.into())
    } else if metadata.is_dir() {
        FileContent::Dir
    } else {
        let data = fs.read(&path)?;
        let hash = format!("{:x}", Sha256::digest(data));
        FileContent::Regular(hash)
    };
    Ok(WalkDirEntry {
        path: dir_entry.path().into(),
        content,
        mode: metadata.mode(),
        uid: metadata.uid(),
        gid: metadata.gid(),
    })
}

fn walk_with_lib(
    fs: &Ext4,
    path: ext4_view::Path<'_>,
) -> Result<Vec<WalkDirEntry>> {
    let mut output = Vec::new();

    let metadata = fs.symlink_metadata(path)?;
    output.push(WalkDirEntry {
        path: ext4_view::PathBuf::from(path).into(),
        content: FileContent::Dir,
        mode: metadata.mode(),
        uid: metadata.uid(),
        gid: metadata.gid(),
    });

    let entry_iter = match fs.read_dir(path) {
        Ok(entry_iter) => entry_iter,
        Err(Ext4Error::Encrypted) => {
            output[0].content = FileContent::EncryptedDir;
            return Ok(output);
        }
        Err(err) => return Err(err.into()),
    };

    for entry in entry_iter {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }

        if entry.file_type()?.is_dir() {
            output.extend(walk_with_lib(fs, path.as_path())?);
        } else {
            output.push(new_dir_entry(fs, entry)?);
        }
    }

    Ok(output)
}

/// Check if the file at `path` starts with the magic bytes indicating
/// its a compressed test file.
fn is_compressed(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut buf = [0; 4];
    file.read_exact(&mut buf)?;
    Ok(buf == [0x28, 0xb5, 0x2f, 0xfd])
}

/// Check that walking the filesystem with the `ext4-view` crate gives
/// the same results as mounting the filesystem and walking it with
/// [`std::fs`].
///
/// See `./bin/mount_and_walk.rs` for details of mounting and walking
/// the filesystem. That program is run under `sudo` since `mount`
/// requires elevated permissions.
pub fn diff_walk(orig_path: &Path) -> Result<()> {
    // Build `mount_and_walk` in release mode.
    let path = env::var("PATH")?;
    run_cmd(
        Command::new("cargo")
            .args([
                "build",
                "--release",
                "--package",
                "xtask",
                "--bin",
                "mount_and_walk",
            ])
            // Clear the env except for the PATH var. This avoids
            // unwanted rebuilds. The details are complicated, but
            // basically rustup/cargo are not designed to allow nested
            // invocations. Clearing env vars tricks rustup/cargo into
            // behaving correctly.
            //
            // Some past issues that may be relevant (but probably don't
            // tell the whole story):
            // * https://github.com/rust-lang/rustup/issues/3036
            // * https://github.com/rust-lang/cargo/issues/15099
            .env_clear()
            .env("PATH", path),
    )?;

    // If the input file is compressed, decompress it and write to a
    // temporary file.
    let tempdir;
    let path = if is_compressed(orig_path)? {
        tempdir = tempfile::tempdir()?;
        let path = tempdir.path().join("fs.bin");

        run_cmd(
            Command::new("zstd")
                .args([
                    "--decompress",
                    // Don't delete the input file.
                    "--keep",
                    // Set output path.
                    "-o",
                ])
                .args([&path, orig_path]),
        )?;
        path
    } else {
        orig_path.to_path_buf()
    };

    let actual = {
        let ext4 = Ext4::load_from_path(&path)?;
        let before_walk = SystemTime::now();
        let mut paths = walk_with_lib(&ext4, ext4_view::Path::ROOT)?;
        println!(
            "walk_with_lib took {:?}",
            SystemTime::now().duration_since(before_walk).unwrap()
        );
        paths.sort_unstable();
        paths
            .iter()
            .map(|dir_entry| dir_entry.format())
            .collect::<Vec<_>>()
    };
    let expected = {
        let before_cmd = SystemTime::now();
        let output =
            capture_cmd(sudo().arg("target/release/mount_and_walk").arg(path))?;
        println!(
            "mount_and_walk took {:?}",
            SystemTime::now().duration_since(before_cmd).unwrap()
        );
        let mut lines = output
            .stdout
            .split(|c| *c == b'\n')
            .map(|l| l.to_vec())
            .collect::<Vec<_>>();

        // Remove the empty line at the end of the file.
        let last = lines.pop().unwrap();
        if !last.is_empty() {
            bail!("unexpected output from mount_and_walk: last line not empty");
        }

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

    if actual.len() != expected.len() {
        println!(
            "got {} lines, expected {} lines",
            actual.len(),
            expected.len()
        );
        process::exit(1);
    }

    println!("success, no differences detected");
    Ok(())
}
