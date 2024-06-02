// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

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
}

fn create_test_data() -> Result<()> {
    let dir = test_data_dir()?;
    if !dir.exists() {
        fs::create_dir(&dir)?;
    }

    let path = dir.join("test_disk1.bin");
    if !path.exists() {
        let disk = DiskParams {
            path: path.to_owned(),
            size_in_kilobytes: 1024 * 64,
        };
        disk.create()?;
        // TODO(nicholasbishop): mount the filesystem and fill it with
        // test data.
    }

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
