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

/// Error type used by the [`Vec<u8>`] impl of [`Ext4Read`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemIoError {
    start: u64,
    read_len: usize,
    src_len: usize,
}

impl IoError for MemIoError {}

impl Display for MemIoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to read {} bytes at offset {} from a slice of length {}",
            self.read_len, self.start, self.src_len
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for MemIoError {}

impl Ext4Read for Vec<u8> {
    fn read(
        &mut self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn IoError>> {
        let start = usize::try_from(start_byte).map_err(|_| {
            box_err(MemIoError {
                start: start_byte,
                read_len: dst.len(),
                src_len: self.len(),
            })
        })?;

        let end = start + dst.len();
        let src = self.get(start..end).ok_or(box_err(MemIoError {
            start: start_byte,
            read_len: dst.len(),
            src_len: self.len(),
        }))?;
        dst.copy_from_slice(src);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec_read() {
        let mut src = vec![1, 2, 3];

        let mut dst = [0; 3];
        src.read(0, &mut dst).unwrap();
        assert_eq!(dst, [1, 2, 3]);

        let mut dst = [0; 2];
        src.read(1, &mut dst).unwrap();
        assert_eq!(dst, [2, 3]);

        let err = src.read(4, &mut dst).unwrap_err();
        assert_eq!(
            format!("{err}"),
            format!(
                "failed to read 2 bytes at offset 4 from a slice of length 3"
            )
        );
    }
}
