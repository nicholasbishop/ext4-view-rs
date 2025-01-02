// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use anyhow::Result;
use std::path::{Path, PathBuf};
use xtask::{run_cmd, sudo};

const DMSETUP: &str = "dmsetup";

/// Device-mapper device.
///
/// The device will be removed on drop.
pub struct DmDevice {
    /// Name of the DM device, as passed into `create`. This is normally
    /// always `Some`, the `Option` is only needed so that `drop`
    /// doesn't try to remove the mapping after `remove` is called.
    name: Option<String>,
}

impl DmDevice {
    pub fn create(name: &str, table: &str) -> Result<Self> {
        run_cmd(sudo().args([DMSETUP, "create", name, "--table", table]))?;
        Ok(Self {
            name: Some(name.to_owned()),
        })
    }

    fn name(&self) -> &str {
        // OK to unwrap: `name` is always `Some` while the object is live.
        self.name.as_ref().unwrap()
    }

    pub fn path(&self) -> PathBuf {
        Path::new("/dev/mapper").join(self.name())
    }

    pub fn suspend(&self) -> Result<()> {
        run_cmd(sudo().args([DMSETUP, "suspend", "--nolockfs", self.name()]))
    }

    pub fn resume(&self) -> Result<()> {
        run_cmd(sudo().args([DMSETUP, "resume", self.name()]))
    }

    pub fn load_table(&self, table: &str) -> Result<()> {
        run_cmd(sudo().args([DMSETUP, "load", self.name(), "--table", table]))
    }

    pub fn remove(mut self) -> Result<()> {
        self.remove_impl()
    }

    fn remove_impl(&mut self) -> Result<()> {
        if self.name.is_some() {
            run_cmd(sudo().args([DMSETUP, "remove", self.name()]))?;
            self.name = None;
        }
        Ok(())
    }
}

impl Drop for DmDevice {
    fn drop(&mut self) {
        // Ignore errors in drop.
        if let Err(err) = self.remove_impl() {
            eprintln!("{err:?}");
        }
    }
}

pub struct DmFlakey {
    pub start_sector: u64,
    pub num_sectors: u64,
    pub block_dev: PathBuf,
    pub offset: u64,
    pub up_interval: u64,
    pub down_interval: u64,
    pub features: Vec<&'static str>,
}

impl DmFlakey {
    pub fn as_string(&self) -> String {
        let mut t = vec![
            self.start_sector.to_string(),
            self.num_sectors.to_string(),
            "flakey".to_owned(),
            self.block_dev.to_str().unwrap().to_owned(),
            self.offset.to_string(),
            self.up_interval.to_string(),
            self.down_interval.to_string(),
            self.features.len().to_string(),
        ];
        t.extend(self.features.iter().map(|s| s.to_string()));
        t.join(" ")
    }
}
