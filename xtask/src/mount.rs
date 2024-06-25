// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Whether to mount read-only or read-write.
pub struct ReadOnly(pub bool);

/// Mounted filesystem.
///
/// The filesystem will be unmounted on drop.
pub struct Mount {
    mount_point: TempDir,
}

impl Mount {
    /// Mount a file containing a filesystem to a temporary directory.
    ///
    /// Mounting is a privileged operation, so this runs `sudo mount`.
    pub fn new(fs_bin: &Path, read_only: ReadOnly) -> Result<Self> {
        let mount_point = TempDir::new()?;
        let status = Command::new("sudo")
            .args(["mount", "-o", if read_only.0 { "ro" } else { "rw" }])
            .args([fs_bin, mount_point.path()])
            .status()?;
        if !status.success() {
            bail!("mount failed");
        }
        Ok(Self { mount_point })
    }

    /// Get the mount point.
    pub fn path(&self) -> &Path {
        self.mount_point.path()
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        // Ignore errors in drop.
        let _ = Command::new("sudo")
            .arg("umount")
            .arg(self.mount_point.path())
            .status();
    }
}
