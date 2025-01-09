// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// All TODO, just an experiment

use crate::block_size::BlockSize;
use crate::error::{Corrupt, Ext4Error};
use crate::util::usize_from_u32;
use crate::Ext4Read;

pub(crate) struct BlockRead<'a> {
    pub(crate) reader: &'a mut dyn Ext4Read,
    pub(crate) num_blocks: u64,
    pub(crate) block_size: BlockSize,
    pub(crate) block_index: u64,
    pub(crate) offset_within_block: u32,
    pub(crate) dst: &'a mut [u8],
}

impl BlockRead<'_> {
    fn err(&self) -> Ext4Error {
        Ext4Error::Corrupt(Corrupt::BlockRead {
            block_index: self.block_index,
            offset_within_block: self.offset_within_block,
            read_len: self.dst.len(),
        })
    }

    pub(crate) fn read(&mut self) -> Result<(), Ext4Error> {
        // The first 1024 bytes are reserved for non-filesystem
        // data. This conveniently allows for something like a null
        // pointer check.
        if self.block_index == 0 && self.offset_within_block < 1024 {
            return Err(self.err());
        }

        // Check the block index.
        if self.block_index >= self.num_blocks {
            return Err(self.err());
        }

        // The start of the read must be within the block.
        if self.offset_within_block >= self.block_size {
            return Err(self.err());
        }

        // Get end bound of the read. This must be at the end of the block,
        // or earlier.
        let read_end = usize_from_u32(self.offset_within_block)
            .checked_add(self.dst.len())
            .ok_or_else(|| self.err())?;
        if read_end > self.block_size {
            return Err(self.err());
        }

        // Get the absolute byte to start reading from.
        let start_byte = self
            .block_index
            .checked_mul(self.block_size.to_u64())
            .and_then(|v| v.checked_add(u64::from(self.offset_within_block)))
            .ok_or_else(|| self.err())?;

        self.reader
            .read(start_byte, self.dst)
            .map_err(Ext4Error::Io)
    }
}

#[cfg(feature = "std")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::MockExt4Read;
    use std::io::{self, ErrorKind};

    fn bs1024() -> BlockSize {
        BlockSize::from_superblock_value(0).unwrap()
    }

    fn block_read_error(
        block_index: u64,
        offset_within_block: u32,
        read_len: usize,
    ) -> Corrupt {
        Corrupt::BlockRead {
            block_index,
            offset_within_block,
            read_len,
        }
    }

    /// Test that reading from the first 1024 bytes of the file fails.
    #[test]
    fn test_read_from_block_first_1024() {
        let mut reader = MockExt4Read::new();
        let mut dst = vec![0; 1024];
        assert_eq!(
            BlockRead {
                reader: &mut reader,
                num_blocks: 10,
                block_size: bs1024(),
                block_index: 0,
                offset_within_block: 1023,
                dst: &mut dst
            }
            .read()
            .unwrap_err()
            .as_corrupt()
            .unwrap(),
            &block_read_error(0, 1023, 1024),
        );
    }

    /// Test that reading past the last block of the file fails.
    #[test]
    fn test_read_from_block_past_file_end() {
        let mut reader = MockExt4Read::new();
        let mut dst = vec![0; 1024];
        assert_eq!(
            BlockRead {
                reader: &mut reader,
                num_blocks: 10,
                block_size: bs1024(),
                block_index: 10,
                offset_within_block: 0,
                dst: &mut dst
            }
            .read()
            .unwrap_err()
            .as_corrupt()
            .unwrap(),
            &block_read_error(10, 0, 1024),
        );
    }

    /// Test that reading at an offset >= the block size fails.
    #[test]
    fn test_read_from_block_invalid_offset() {
        let mut reader = MockExt4Read::new();
        let mut dst = vec![0; 1024];
        assert_eq!(
            BlockRead {
                reader: &mut reader,
                num_blocks: 10,
                block_size: bs1024(),
                block_index: 1,
                offset_within_block: 1024,
                dst: &mut dst
            }
            .read()
            .unwrap_err()
            .as_corrupt()
            .unwrap(),
            &block_read_error(1, 1024, 1024),
        );
    }

    /// Test that reading past the end of the block fails.
    #[test]
    fn test_read_from_block_past_block_end() {
        let mut reader = MockExt4Read::new();
        let mut dst = vec![0; 25];
        assert_eq!(
            BlockRead {
                reader: &mut reader,
                num_blocks: 10,
                block_size: bs1024(),
                block_index: 1,
                offset_within_block: 1000,
                dst: &mut dst
            }
            .read()
            .unwrap_err()
            .as_corrupt()
            .unwrap(),
            &block_read_error(1, 1000, 25),
        );
    }

    /// Test that IO errors are propagated.
    #[test]
    fn test_read_from_block_io_err() {
        let mut reader = MockExt4Read::new();
        reader
            .expect_read()
            .times(1)
            .withf(|start_byte, dst| {
                *start_byte == 2 * 1024 + 100 && dst.len() == 25
            })
            .return_once(|_, _| {
                Err(Box::new(io::Error::from(ErrorKind::NotFound)))
            });
        let mut dst = vec![0; 25];
        BlockRead {
            reader: &mut reader,
            num_blocks: 10,
            block_size: bs1024(),
            block_index: 2,
            offset_within_block: 100,
            dst: &mut dst,
        }
        .read()
        .unwrap_err()
        .as_io()
        .unwrap();
    }

    /// Test a successful read.
    #[test]
    fn test_read_from_block_success() {
        let mut reader = MockExt4Read::new();
        reader
            .expect_read()
            .times(1)
            .withf(|start_byte, dst| {
                *start_byte == 2 * 1024 + 100 && dst.len() == 25
            })
            .return_once(|_, _| Ok(()));
        let mut dst = vec![0; 25];
        BlockRead {
            reader: &mut reader,
            num_blocks: 10,
            block_size: bs1024(),
            block_index: 2,
            offset_within_block: 100,
            dst: &mut dst,
        }
        .read()
        .unwrap();
    }
}
