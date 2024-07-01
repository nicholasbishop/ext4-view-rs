// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;
use std::{env, str};
use xtask::{Mount, ReadOnly};

/// Get the path of the root directory of the repo.
///
/// This assumes the currently-running executable is `<repo>/target/release/xtask`.
fn repo_root() -> Result<PathBuf> {
    let current_exe = env::current_exe()?;
    Ok(current_exe
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .context("xtask is not in expected location")?
        .to_owned())
}

/// Get the path of the `test_data` directory.
fn test_data_dir() -> Result<PathBuf> {
    Ok(repo_root()?.join("test_data"))
}

struct DiskParams {
    path: PathBuf,
    size_in_kilobytes: u32,
}

impl DiskParams {
    fn create(&self) -> Result<()> {
        let uid = nix::unistd::getuid();
        let gid = nix::unistd::getgid();

        let status = Command::new("mkfs.ext4")
            // Set the ownership of the root directory in the filesystem
            // to the current uid/gid instead of root. That allows the
            // mounted filesystem to be edited without root permissions,
            // although the mount operation itself still requires root.
            .args(["-E", &format!("root_owner={uid}:{gid}")])
            .arg(&self.path)
            .arg(format!("{}k", self.size_in_kilobytes))
            .status()?;
        if !status.success() {
            bail!("mkfs.ext4 failed");
        }
        Ok(())
    }

    /// Put some data on the disk.
    fn fill(&self) -> Result<()> {
        let mount = Mount::new(&self.path, ReadOnly(false))?;
        let root = mount.path();

        // Create an empty file.
        fs::write(root.join("empty_file"), [])?;
        // Create an empty dir.
        fs::create_dir(root.join("empty_dir"))?;

        // Create a small text file.
        fs::write(root.join("small_file"), "hello, world!")?;

        // Create some nested directories.
        let dir1 = root.join("dir1");
        let dir2 = dir1.join("dir2");
        fs::create_dir(dir1).unwrap();
        fs::create_dir(&dir2).unwrap();

        // Create some symlinks.
        symlink("small_file", root.join("sym_simple"))?;
        // Symlink targets up to 59 characters are stored inline, so
        // create a symlink just under the limit and just over the
        // limit.
        symlink("a".repeat(59), root.join("sym_59"))?;
        symlink("a".repeat(60), root.join("sym_60"))?;
        // Target is an absolute file path.
        symlink("/small_file", dir2.join("sym_abs")).unwrap();
        // Target is an absolute directory path.
        symlink("/dir1", dir2.join("sym_abs_dir")).unwrap();
        // Target is a relative file path.
        symlink("../../small_file", dir2.join("sym_rel")).unwrap();
        // Target is a relative directory path.
        symlink("../../dir1", dir2.join("sym_rel_dir")).unwrap();
        // Target is maximum length (341*3 = 1023).
        symlink("/..".repeat(341), root.join("sym_long")).unwrap();
        // Create a symlink loop.
        symlink("sym_loop_b", root.join("sym_loop_a")).unwrap();
        symlink("sym_loop_a", root.join("sym_loop_b")).unwrap();

        // Create a directory with 1000 files. This is sized to
        // create an htree with depth 0.
        let medium_dir = root.join("medium_dir");
        fs::create_dir(&medium_dir)?;
        for i in 0..1_000 {
            let i = i.to_string();
            fs::write(medium_dir.join(&i), i)?;
        }

        // Create a directory with 10_000 files. This is sized to
        // create an htree with depth 1.
        let big_dir = root.join("big_dir");
        fs::create_dir(&big_dir)?;
        for i in 0..10_000 {
            let i = i.to_string();
            fs::write(big_dir.join(&i), i)?;
        }

        // Create a file with holes. By having five blocks, with holes
        // between them, the file will require at least five extents. This
        // will ensure the extent tree does not fit entirely within the
        // inode, allowing testing of internal nodes.
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("holes"))?;
        let block = vec![0xa5; 4096];
        for _ in 0..5 {
            // Write a 4K block.
            f.write_all(&block)?;
            // Leave an 8K hole.
            f.seek(SeekFrom::Current(8192))?;
        }
        Ok(())
    }

    /// Check some properties of the filesystem.
    fn check(&self) -> Result<()> {
        self.check_dir_htree_depth("/medium_dir", 0)?;
        self.check_dir_htree_depth("/big_dir", 1)?;

        Ok(())
    }

    /// Run the [debugfs] tool on the disk with the given `request` and
    /// return the raw stdout.
    ///
    /// [debugfs]: https://www.man7.org/linux/man-pages/man8/debugfs.8.html
    fn run_debugfs(&self, request: &str) -> Result<Vec<u8>> {
        let output = Command::new("debugfs")
            .args(["-R", request])
            .arg(&self.path)
            .output()?;
        if !output.status.success() {
            bail!("debugfs failed");
        }
        Ok(output.stdout)
    }

    /// Use debugfs to check that a directory has the expected htree depth.
    ///
    /// The depth is the number of levels containing internal nodes, not
    /// counting the root. So, a depth of zero means the htree's root
    /// node points directly to leaf nodes. A depth of one means the
    /// htree's root node points to internal nodes, and those nodes
    /// point to leaf nodes.
    fn check_dir_htree_depth(
        &self,
        dir_path: &str,
        expected_depth: u8,
    ) -> Result<()> {
        let stdout = self.run_debugfs(&format!("htree_dump {dir_path}"))?;
        let stdout = str::from_utf8(&stdout)?;
        let depth = stdout
            .lines()
            .filter_map(|line| line.trim().strip_prefix("Indirect levels:"))
            .next()
            .context("htree levels not found")?;
        let depth: u8 = depth.trim().parse()?;
        if depth != expected_depth {
            bail!(
                "{dir_path}: htree depth is {depth}, expected {expected_depth}"
            );
        }
        Ok(())
    }
}

fn create_test_data() -> Result<()> {
    let dir = test_data_dir()?;
    if !dir.exists() {
        fs::create_dir(&dir)?;
    }

    // Create a 1KiB file containing just the superblock data. Used for
    // unit testing in the superblock module.
    let path = dir.join("raw_superblock.bin");
    if !path.exists() {
        let disk = DiskParams {
            path: path.to_owned(),
            size_in_kilobytes: 128,
        };
        disk.create()?;
        let data = fs::read(&path)?;
        let superblock = &data[1024..2048];
        fs::write(path, superblock)?;
    }

    let path = dir.join("test_disk1.bin");
    let disk = DiskParams {
        path: path.to_owned(),
        size_in_kilobytes: 1024 * 64,
    };
    if !path.exists() {
        disk.create()?;
        disk.fill()?;
    }
    disk.check()?;

    Ok(())
}

#[derive(Parser)]
struct Opt {
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Create files for tests.
    ///
    /// The test files will be committed via git-lfs, so developers
    /// working on the repo do not typically need to run this command.
    CreateTestData,
}

fn main() -> Result<()> {
    let opt = Opt::parse();

    match &opt.action {
        Action::CreateTestData => create_test_data(),
    }
}
