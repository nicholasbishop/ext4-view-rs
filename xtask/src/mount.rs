// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::{run_cmd, sudo};
use anyhow::Result;
use std::path::Path;
use tempfile::TempDir;

/// Whether to mount read-only or read-write.
pub struct ReadOnly(pub bool);

/// Mounted filesystem.
///
/// The filesystem will be unmounted on drop.
pub struct Mount {
    /// Temporary directory used as the mount point. This is normally
    /// always `Some`, the `Option` is only needed so that `drop`
    /// doesn't try to unmount after `unmount` is called.
    mount_point: Option<TempDir>,
}

impl Mount {
    /// Mount a file containing a filesystem to a temporary directory.
    ///
    /// Mounting is a privileged operation, so this runs `sudo mount`.
    pub fn new(fs_bin: &Path, read_only: ReadOnly) -> Result<Self> {
        let mount_point = TempDir::new()?;
        run_cmd(
            sudo()
                .args(["mount", "-o", if read_only.0 { "ro" } else { "rw" }])
                .args([fs_bin, mount_point.path()]),
        )?;
        Ok(Self {
            mount_point: Some(mount_point),
        })
    }

    /// Get the mount point.
    pub fn path(&self) -> &Path {
        // OK to unwrap: `mount_point` is always `Some` while the object
        // is live.
        self.mount_point.as_ref().unwrap().path()
    }

    /// Unmount the filesystem.
    pub fn unmount(mut self) -> Result<()> {
        self.unmount_impl()
    }

    fn unmount_impl(&mut self) -> Result<()> {
        if self.mount_point.is_some() {
            run_cmd(sudo().arg("umount").arg(self.path()))?;
            self.mount_point = None;
        }
        Ok(())
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        // Ignore errors in drop.
        if let Err(err) = self.unmount_impl() {
            eprintln!("{err:?}");
        }
    }
}
