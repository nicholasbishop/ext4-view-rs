// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::error::IoError;
use alloc::boxed::Box;

/// Interface used by `Ext4` to read the filesystem data from a storage
/// file or device.
pub trait Ext4Read {
    /// Read bytes into `dst`, starting at `start_byte`.
    ///
    /// Exactly `dst.len()` bytes will be read; an error will be
    /// returned if there is not enough data to fill `dst`, or if the
    /// data cannot be read for any reason.
    fn read(
        &mut self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn IoError>>;
}
