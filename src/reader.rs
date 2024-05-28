// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::error::IoError;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "std")]
use {
    std::fs::File,
    std::io::{Seek, SeekFrom},
};

// TODO
#[derive(Debug)]
pub struct SliceError;
impl IoError for SliceError {}

impl Display for SliceError {
    fn fmt(&self, _f: &mut Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

#[cfg(feature = "std")]
impl IoError for std::io::Error {}

fn box_err<E: IoError>(err: E) -> Box<dyn IoError> {
    Box::new(err)
}

/// Interface used by [`Ext4`] to read the filesystem data from a storage
/// file or device.
///
/// [`Ext4`]: crate::Ext4
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

impl Ext4Read for &[u8] {
    fn read(
        &mut self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn IoError>> {
        let start =
            usize::try_from(start_byte).map_err(|_| box_err(SliceError))?;

        let end = start + dst.len();
        let src = self.get(start..end).ok_or(box_err(SliceError))?;
        dst.copy_from_slice(src);
        Ok(())
    }
}

// TODO, want this for all &[u8]
impl Ext4Read for Vec<u8> {
    fn read(
        &mut self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn IoError>> {
        self.as_slice().read(start_byte, dst)
    }
}

#[cfg(feature = "std")]
impl Ext4Read for File {
    fn read(
        &mut self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn IoError>> {
        use std::io::Read;

        self.seek(SeekFrom::Start(start_byte)).map_err(box_err)?;
        self.read_exact(dst).map_err(box_err)?;
        Ok(())
    }
}
