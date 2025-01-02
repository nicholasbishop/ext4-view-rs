// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::{capture_cmd, run_cmd, sudo};
use anyhow::Result;
use std::path::{Path, PathBuf};

const LOSETUP: &str = "losetup";

/// Loop device.
///
/// The loop device will be detached on drop.
pub struct LoopDevice {
    /// Path of the loop device, e.g. "/dev/loop0". This is normally
    /// always `Some`, the `Option` is only needed so that `drop`
    /// doesn't try to detach after `detach` is called.
    path: Option<PathBuf>,
}

impl LoopDevice {
    /// Create a loop device using `file` as the backing storage.
    pub fn new(file: &Path) -> Result<Self> {
        let output = capture_cmd(
            sudo()
                .arg(LOSETUP)
                // Automatically find the first unused loop device.
                .arg("--find")
                // Print the device path to stdout.
                .arg("--show")
                .arg(file),
        )?;
        let path = String::from_utf8(output.stdout)?;
        Ok(Self {
            path: Some(PathBuf::from(path.trim())),
        })
    }

    /// Get the path of the loop device, e.g. "/dev/loop0".
    pub fn path(&self) -> &Path {
        // OK to unwrap: `path` is always `Some` while the object is live.
        self.path.as_ref().unwrap()
    }

    /// Detach the loop device.
    pub fn detach(mut self) -> Result<()> {
        self.detach_impl()
    }

    fn detach_impl(&mut self) -> Result<()> {
        if self.path.is_some() {
            run_cmd(sudo().args([LOSETUP, "--detach"]).arg(self.path()))?;
            self.path = None;
        }
        Ok(())
    }
}

impl Drop for LoopDevice {
    fn drop(&mut self) {
        // Ignore errors in drop.
        if let Err(err) = self.detach_impl() {
            eprintln!("{err:?}");
        }
    }
}
