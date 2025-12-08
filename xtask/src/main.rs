// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

extern crate alloc;

mod bench;
mod big_fs;
mod dmsetup;
mod losetup;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use dmsetup::{DmDevice, DmFlakey};
use losetup::LoopDevice;
use nix::fcntl::{self, FallocateFlags};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, str};
use tempfile::TempDir;
use xtask::{Mount, ReadOnly, capture_cmd, diff_walk, run_cmd, sudo};

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
///
/// This function is duplicated in `/tests/integration/ext2.rs`.
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
    Ext3,
    Ext4,
}

enum HashAlg {
    Tea,
}

struct DiskParams {
    path: PathBuf,
    size_in_kilobytes: u32,
    fs_type: FsType,
    block_size: u32,
    // Directory block hash algorithm. If `None`, the `mkfs` default is used.
    hash_alg: Option<HashAlg>,
    // Inode size in bytes. If `None`, the `mkfs` default is used.
    inode_size: Option<u32>,
}

impl DiskParams {
    fn create(&self) -> Result<()> {
        // Delete the file if it already exists.
        let _ = fs::remove_file(&self.path);

        let uid = nix::unistd::getuid();
        let gid = nix::unistd::getgid();

        let mkfs = match self.fs_type {
            FsType::Ext2 => "mkfs.ext2",
            FsType::Ext3 => "mkfs.ext3",
            FsType::Ext4 => "mkfs.ext4",
        };

        let mut cmd = Command::new(mkfs);
        cmd
            // Set the ownership of the root directory in the filesystem
            // to the current uid/gid instead of root. That allows the
            // mounted filesystem to be edited without root permissions,
            // although the mount operation itself still requires root.
            .args(["-E", &format!("root_owner={uid}:{gid}")])
            // Set the volume label. This string is 16 bytes, which is
            // the maximum length.
            .args(["-L", "ext4-view testfs"])
            .arg(&self.path)
            .arg(format!("{}k", self.size_in_kilobytes));

        if self.fs_type == FsType::Ext4 {
            // Enable directory encryption.
            cmd.args(["-O", "encrypt"]);
        }

        // Set block size.
        cmd.arg("-b");
        cmd.arg(self.block_size.to_string());

        // Set inode size.
        if let Some(inode_size) = self.inode_size {
            cmd.arg("-I");
            cmd.arg(inode_size.to_string());
        }

        // Set the hash algorithm. This seems to require a config file,
        // couldn't find a way to do it through mke2fs arguments.
        if matches!(self.hash_alg, Some(HashAlg::Tea)) {
            cmd.env("MKE2FS_CONFIG", "xtask/src/tea.mke2fs.conf");
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

            run_cmd(sudo().args(["chown", "123:456"]).arg(path))?;
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

        create_file_with_holes(&root.join("holes"))?;

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

        mount.unmount()?;

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

        create_file_with_holes(&root.join("holes"))?;

        mount.unmount()?;

        Ok(())
    }

    fn fill_ext3(&self) -> Result<()> {
        let mount = Mount::new(&self.path, ReadOnly(false))?;
        let root = mount.path();

        // Create a directory with 1000 files. This is sized to
        // create an htree with depth 0.
        let medium_dir = root.join("medium_dir");
        fs::create_dir(&medium_dir)?;
        for i in 0..1_000 {
            let i = i.to_string();
            fs::write(medium_dir.join(&i), i)?;
        }
        Ok(())
    }

    /// Create a filesystem that was not unmounted cleanly. The root
    /// directory contains a number of subdirectories that are only in
    /// the journal.
    fn create_with_journal(&self) -> Result<()> {
        // Multiple attempts may be needed to get a filesystem with the
        // desired journal state.
        for i in 1..=10 {
            println!("creating filesystem with journal, attempt {i}");

            self.create()?;
            self.make_filesystem_need_recovery()?;

            // Verify that the journal contains at least one descriptor
            // block one revocation block, and one commit block. The
            // full output should look something like this:
            //
            // ```
            // Journal starts at block 1, transaction 2
            // Found expected sequence 2, type 1 (descriptor block) at block 1
            // Found expected sequence 2, type 5 (revoke table) at block 577
            // [...]
            // Found expected sequence 2, type 1 (descriptor block) at block 1303
            // Found expected sequence 2, type 2 (commit block) at block 1311
            // ```
            let logdump = self.run_debugfs("logdump")?;
            let logdump = str::from_utf8(&logdump)?;
            if logdump.contains("(descriptor block)")
                && logdump.contains("(revoke table)")
                && logdump.contains("(commit block)")
            {
                return Ok(());
            }
        }

        bail!("failed to create filesystem");
    }

    /// Modify the filesystem so that some data is written to the
    /// journal, but not yet flushed to the main filesystem.
    ///
    /// This uses losetup and dmsetup to simulate a power failure after
    /// data is written to the journal.
    fn make_filesystem_need_recovery(&self) -> Result<()> {
        // Get the number of sectors in the filesystem.
        let num_sectors = {
            let sector_size = 512;
            u64::from(self.size_in_kilobytes) * 1024 / sector_size
        };

        // Use losetup to create a block device from the file containing
        // the filesystem.
        let loop_dev = LoopDevice::new(&self.path)?;

        // Create a device-mapper device using the dm-flakey
        // target. This target allows us to cut off writes at a certain
        // point, simulating a power failure. In its initial state
        // however, this acts as a simple pass-through device.
        let table = DmFlakey {
            start_sector: 0,
            num_sectors,
            block_dev: loop_dev.path().to_owned(),
            offset: 0,
            up_interval: 100,
            down_interval: 0,
            features: Vec::new(),
        };
        let dm_device = DmDevice::create("flakey-dev", &table.as_string())?;

        // Mount the filesystem from the flakey device, and create a
        // bunch of directories.
        let mount = Mount::new(&dm_device.path(), ReadOnly(false))?;
        for i in 0..1000 {
            let p = mount.path().join(format!("dir{i}"));
            fs::create_dir(&p)?;

            // Immediately remove the directory and recreate it. This
            // causes revocation blocks to be created in the journal.
            fs::remove_dir(&p)?;
            fs::create_dir(&p)?;
        }

        // At this point, the directory blocks have likely been written
        // to the journal, but not yet written to their final locations
        // on disk. (This is somewhat timing dependant however, so this
        // whole function is called in a loop until the desired
        // conditions are met.)

        // Change the device configuration so that all writes are
        // dropped. When the filesystem is unmounted below, any data not
        // already written will be lost.
        dm_device.suspend()?;
        let drop_writes_table = DmFlakey {
            up_interval: 0,
            down_interval: 100,
            features: vec!["drop_writes"],
            ..table
        };
        dm_device.load_table(&drop_writes_table.as_string())?;
        dm_device.resume()?;

        // Clean up.
        mount.unmount()?;
        dm_device.remove()?;
        loop_dev.detach()?;

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

/// Create a file with holes at `path`.
///
/// This assumes a block size of 1024.
///
/// File format per block:
///  0,1: hole
///  2,3: data
///  4,5: hole
///  6,7: data
///  8,9: hole
///
/// Should match `expected_holes_data` in the ext4-view tests.
fn create_file_with_holes(path: &Path) -> Result<()> {
    let block_size = 1024;
    let mut data = Vec::new();
    data.extend(vec![0; block_size * 2]);
    data.extend(vec![0xa1; block_size]);
    data.extend(vec![0xa2; block_size]);
    data.extend(vec![0; block_size * 2]);
    data.extend(vec![0xa3; block_size]);
    data.extend(vec![0xa4; block_size]);
    data.extend(vec![0; block_size * 2]);
    fs::write(path, data)?;
    let f = OpenOptions::new().write(true).open(path)?;

    for block in [0, 4, 8] {
        let offset = block_size * block;
        let len = block_size * 2;
        fcntl::fallocate(
            &f,
            FallocateFlags::FALLOC_FL_PUNCH_HOLE
                | FallocateFlags::FALLOC_FL_KEEP_SIZE,
            offset as i64,
            len as i64,
        )?;
    }

    Ok(())
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
            block_size: 1024,
            hash_alg: None,
            inode_size: None,
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
        block_size: 1024,
        hash_alg: None,
        inode_size: None,
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
        block_size: 1024,
        hash_alg: None,
        inode_size: None,
    };
    disk.create()?;
    disk.fill_ext2()?;
    zstd_compress(&disk.path)?;

    let path = dir.join("test_disk_4k_block_journal.bin");
    let disk = DiskParams {
        path: path.to_owned(),
        size_in_kilobytes: 1024 * 64,
        fs_type: FsType::Ext4,
        block_size: 4096,
        hash_alg: None,
        inode_size: None,
    };
    disk.create_with_journal()?;
    zstd_compress(&disk.path)?;

    // Ext3 filesystem with the smallest-possible inode size (128
    // bytes), and using TEA instead of half-MD4 for directory entry
    // hashes.
    let path = dir.join("test_disk_ext3.bin");
    let disk = DiskParams {
        path: path.to_owned(),
        size_in_kilobytes: 1024 * 96,
        fs_type: FsType::Ext3,
        block_size: 1024,
        hash_alg: Some(HashAlg::Tea),
        inode_size: Some(128),
    };
    disk.create()?;
    disk.fill_ext3()?;
    disk.check_dir_htree_depth("/medium_dir", 0)?;
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

    /// Download a ChromiumOS image and extract its root & stateful partitions.
    ///
    /// Each can be used with the `diff-walk` action to verify that the
    /// library can read the whole filesystem correctly.
    DownloadBigFilesystems,

    /// Benchmark the library.
    Bench {
        /// Path of a file containing an ext4 filesystem.
        path: PathBuf,

        /// Number of iterations to run.
        #[arg(short, long, default_value_t = 5)]
        iterations: u32,
    },
}

fn main() -> Result<()> {
    let opt = Opt::parse();

    match &opt.action {
        Action::CreateTestData => create_test_data(),
        Action::DiffWalk { path } => diff_walk::diff_walk(path),
        Action::DownloadBigFilesystems => big_fs::download_big_filesystems(),
        Action::Bench { path, iterations } => {
            bench::run_bench(path, *iterations)
        }
    }
}
