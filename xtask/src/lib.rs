// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod mount;

use anyhow::{Context, Result, bail};
use sha2::Digest;
use sha2::Sha256;
use std::fs::File;
use std::io;
use std::path::Path;
use std::process::{Command, Output};

pub mod diff_walk;
pub use mount::{Mount, ReadOnly};

/// Calculate the SHA256 hash of the file at `path`.
///
/// This calculates the hash incrementally, so large files are not
/// loaded into memory all at once.
pub fn calc_file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(format!("{hash:x}"))
}

fn cmd_to_string(cmd: &Command) -> String {
    format!("{cmd:?}").replace('"', "")
}

/// Run a command.
///
/// Return an error if the command fails to launch or if it exits
/// non-zero.
pub fn run_cmd(cmd: &mut Command) -> Result<()> {
    eprintln!("run: {}", cmd_to_string(cmd));
    let program = cmd.get_program().to_string_lossy().into_owned();
    let status = cmd
        .status()
        .context(format!("failed to launch {program}"))?;
    if !status.success() {
        bail!("command {program} failed: {status:?}");
    }
    Ok(())
}

/// Run a command and capture its output.
///
/// Return an error if the command fails to launch or if it exits
/// non-zero.
pub fn capture_cmd(cmd: &mut Command) -> Result<Output> {
    eprintln!("capture: {}", cmd_to_string(cmd));
    let program = cmd.get_program().to_string_lossy().into_owned();
    let output = cmd
        .output()
        .context(format!("failed to launch {program}"))?;
    if !output.status.success() {
        bail!(
            "command {program} failed: {status:?}",
            status = output.status
        );
    }
    Ok(output)
}

/// Create a `Command` to run sudo.
pub fn sudo() -> Command {
    Command::new("sudo")
}
