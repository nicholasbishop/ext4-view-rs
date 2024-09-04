// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod big_fs;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, str};
use tempfile::TempDir;
use xtask::{capture_cmd, run_cmd};
use xtask::{diff_walk, Mount, ReadOnly};

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

/// Generate data for a big file.
///
/// The data will be `num_blocks` in size, with a block size of
/// 1024. Each block will be all zeroes, except for the first and last
/// four bytes, which will contain the block index encoded as a
/// little-endian `u32`.
fn gen_big_file(num_blocks: u32) -> Vec<u8> {
    let mut file = Vec::new();
    let block_size = 1024;
    for i in 0..num_blocks {
        let mut block = vec![0; block_size];
        let i_le = i.to_le_bytes();
        block[..4].copy_from_slice(&i_le);
        block[block_size - 4..].copy_from_slice(&i_le);
        file.extend(block);
    }
    file
}

#[derive(PartialEq)]
enum FsType {
    Ext2,
    Ext4,
}

struct DiskParams {
    path: PathBuf,
    size_in_kilobytes: u32,
    fs_type: FsType,
}

impl DiskParams {
    fn create(&self) -> Result<()> {
        let uid = nix::unistd::getuid();
        let gid = nix::unistd::getgid();

        let mkfs = match self.fs_type {
            FsType::Ext2 => "mkfs.ext2",
            FsType::Ext4 => "mkfs.ext4",
        };

        let mut cmd = Command::new(mkfs);
        cmd
            // Set the ownership of the root directory in the filesystem
            // to the current uid/gid instead of root. That allows the
            // mounted filesystem to be edited without root permissions,
            // although the mount operation itself still requires root.
            .args(["-E", &format!("root_owner={uid}:{gid}")])
            .arg(&self.path)
            .arg(format!("{}k", self.size_in_kilobytes));

        if self.fs_type == FsType::Ext4 {
            // Enable directory encryption.
            cmd.args(["-O", "encrypt"]);
        }

        run_cmd(&mut cmd)
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

        // Create an empty file with a specific uid/gid.
        {
            let path = root.join("owner_file");
            fs::write(&path, [])?;

            let status = Command::new("sudo")
                .args(["chown", "123:456"])
                .arg(path)
                .status()?;
            if !status.success() {
                bail!("chmod failed");
            }
        }

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

        run_cmd(
            Command::new("fscrypt")
                .args(["setup", "--all-users"])
                .arg(root),
        )?;

        // Create an empty directory to encrypt.
        let encrypted_dir = root.join("encrypted_dir");
        fs::create_dir(&encrypted_dir)?;

        // Create a temporary 32-byte file containing a raw key. This
        // key is just used for test data, it is intentionally not a
        // good key.
        let tmp_dir = TempDir::new()?;
        let raw_key_path = tmp_dir.path().join("raw_key");
        fs::write(&raw_key_path, [0xab; 32])?;

        // Set up encryption for the directory. This leaves the
        // directory unlocked.
        run_cmd(
            Command::new("fscrypt")
                .arg("encrypt")
                // Set up the protector for this directory. The protector
                // will be a raw key (32 bytes of data) named "protector1".
                .args(["--name", "protector1"])
                .args(["--source", "raw_key"])
                .arg("--key")
                .arg(raw_key_path)
                .arg(&encrypted_dir),
        )?;

        // Create a file in the encrypted directory.
        fs::write(encrypted_dir.join("file"), "encrypted!")?;

        // Lock the directory.
        run_cmd(Command::new("fscrypt").arg("lock").arg(encrypted_dir))?;

        Ok(())
    }

    /// Write some data to an ext2 test filesystem.
    ///
    /// An ext2 filesystem is in many ways not that different from
    /// ext4. The main thing we want to test is files stored with a
    /// block map instead of extents.
    fn fill_ext2(&self) -> Result<()> {
        let mount = Mount::new(&self.path, ReadOnly(false))?;
        let root = mount.path();

        fs::write(root.join("small_file"), "hello, world!")?;

        // Create a big file to exercise the BlockMap iterator. Note
        // that the calculations below assume a 1K block size, so
        // indirect blocks can store 256 (1024÷4) block indices.
        let big_file_size_in_blocks =
            // Direct blocks.
            12 +
            // Indirect blocks.
            256 +
            // Double indirect blocks.
            (256 * 256) +
            // Triple indirect blocks. This is the highest level of
            // block maps. Ideally this size would be 256³, but that
            // would require a huge filesystem.
            (256 * 16);
        fs::write(
            root.join("big_file"),
            gen_big_file(big_file_size_in_blocks),
        )?;

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
        let output = capture_cmd(
            Command::new("debugfs")
                .args(["-R", request])
                .arg(&self.path),
        )?;
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

/// Use `zstd` to compress the file at `path`. A new file will be
/// created with the same path but with ".zst" appended.
///
/// The original file will be deleted.
fn zstd_compress(path: &Path) -> Result<()> {
    run_cmd(
        Command::new("zstd")
            .args([
                // Delete the input file.
                "--rm",
                // If the output already exists, overwrite it without asking.
                "--force",
            ])
            .arg(path),
    )
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
            fs_type: FsType::Ext4,
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
        fs_type: FsType::Ext4,
    };
    disk.create()?;
    disk.fill()?;
    disk.check()?;
    zstd_compress(&disk.path)?;

    let path = dir.join("test_disk_ext2.bin");
    let disk = DiskParams {
        path: path.to_owned(),
        // A 64MiB disk isn't quite big enough for a three-level block
        // map, so jump up to 64+32.
        size_in_kilobytes: 1024 * 96,
        fs_type: FsType::Ext2,
    };
    disk.create()?;
    disk.fill_ext2()?;
    zstd_compress(&disk.path)?;

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

    /// Test that all files/directories in a filesystem are read correctly.
    ///
    /// This mounts a filesystem and walks the mount point, then
    /// compares the result with walking the filesystem via the
    /// `ext4-view` crate.
    ///
    /// Note that mounting a filesystem normally requires elevated
    /// permissions, so this command runs some code with `sudo`.
    DiffWalk {
        /// Path of a file containing an ext4 filesystem.
        path: PathBuf,
    },

    /// Download a ChromiumOS image and extract its stateful partition.
    ///
    /// This can be used with the `diff-walk` action to verify that the
    /// library can read the whole filesystem correctly.
    DownloadBigFilesystem,
}

fn main() -> Result<()> {
    let opt = Opt::parse();

    match &opt.action {
        Action::CreateTestData => create_test_data(),
        Action::DiffWalk { path } => diff_walk::diff_walk(path),
        Action::DownloadBigFilesystem => big_fs::download_big_filesystem(),
    }
}
