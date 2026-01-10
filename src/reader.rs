// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::error::BoxedError;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use async_trait::async_trait;
use core::error::Error;
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use {
    std::fs::File,
    std::io::{Seek, SeekFrom},
    std::sync::Mutex,
};

/// Interface used by [`Ext4`] to read the filesystem data from a storage
/// file or device.
///
/// [`Ext4`]: crate::Ext4
#[async_trait]
pub trait Ext4Read: Send + Sync {
    /// Read bytes into `dst`, starting at `start_byte`.
    ///
    /// Exactly `dst.len()` bytes will be read; an error will be
    /// returned if there is not enough data to fill `dst`, or if the
    /// data cannot be read for any reason.
    async fn read(
        &self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), BoxedError>;
}

// TODO: Move this someplace else
/// Interface used by [`Ext4`] to write the filesystem data to a storage
/// file or device.
///
/// [`Ext4`]: crate::Ext4
#[async_trait]
pub trait Ext4Write: Send + Sync {
    /// Write bytes from `src`, starting at `start_byte`.
    async fn write(
        &self,
        start_byte: u64,
        src: &[u8],
    ) -> Result<(), BoxedError>;
}

#[async_trait]
impl<T> Ext4Read for Arc<T>
where
    T: Ext4Read,
{
    async fn read(
        &self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), BoxedError> {
        (**self).read(start_byte, dst).await
    }
}

#[async_trait]
impl<T> Ext4Write for Arc<T>
where
    T: Ext4Write,
{
    async fn write(
        &self,
        start_byte: u64,
        src: &[u8],
    ) -> Result<(), BoxedError> {
        (**self).write(start_byte, src).await
    }
}

/// Error type used by the [`Vec<u8>`] impls of [`Ext4Read`] and [`Ext4Write`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemIoError {
    start: u64,
    read_len: usize,
    src_len: usize,
}

impl Display for MemIoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to read {} bytes at offset {} from a slice of length {}",
            self.read_len, self.start, self.src_len
        )
    }
}

impl Error for MemIoError {}

#[async_trait]
impl Ext4Read for Vec<u8> {
    async fn read(
        &self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), BoxedError> {
        read_from_bytes(self, start_byte, dst).ok_or_else(|| {
            Box::new(MemIoError {
                start: start_byte,
                read_len: dst.len(),
                src_len: self.len(),
            })
            .into()
        })
    }
}

#[cfg(feature = "std")]
#[async_trait]
impl Ext4Write for Mutex<Vec<u8>> {
    async fn write(
        &self,
        start_byte: u64,
        src: &[u8],
    ) -> Result<(), BoxedError> {
        let mut guard = self.lock().unwrap();
        write_to_bytes(guard.as_mut(), start_byte, src).ok_or_else(|| {
            Box::new(MemIoError {
                start: start_byte,
                read_len: src.len(),
                src_len: guard.len(),
            })
            .into()
        })
    }
}

fn read_from_bytes(src: &[u8], start_byte: u64, dst: &mut [u8]) -> Option<()> {
    let start = usize::try_from(start_byte).ok()?;
    let end = start.checked_add(dst.len())?;
    let src = src.get(start..end)?;
    dst.copy_from_slice(src);

    Some(())
}

fn write_to_bytes(dst: &mut [u8], start_byte: u64, src: &[u8]) -> Option<()> {
    let start = usize::try_from(start_byte).ok()?;
    let end = start.checked_add(src.len())?;
    let dst = dst.get_mut(start..end)?;
    dst.copy_from_slice(src);

    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vec_read() {
        let src = vec![1, 2, 3];

        let mut dst = [0; 3];
        src.read(0, &mut dst).await.unwrap();
        assert_eq!(dst, [1, 2, 3]);

        let mut dst = [0; 2];
        src.read(1, &mut dst).await.unwrap();
        assert_eq!(dst, [2, 3]);

        let err = src.read(4, &mut dst).await.unwrap_err();
        assert_eq!(
            format!("{err}"),
            format!(
                "failed to read 2 bytes at offset 4 from a slice of length 3"
            )
        );
    }
}
