// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    let dir = Path::new("test_data");
    if !dir.exists() {
        fs::create_dir(dir)?;
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
