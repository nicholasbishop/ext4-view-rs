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

pub(crate) fn read_from_block(
    reader: &mut dyn Ext4Read,
    block_size: BlockSize,
    block_index: u64,
    offset_within_block: u32,
    dst: &mut [u8],
) -> Result<(), Ext4Error> {
    let block_read_err = || {
        Ext4Error::Corrupt(Corrupt::BlockRead {
            block_index,
            offset_within_block,
            read_len: dst.len(),
        })
    };

    // The first 1024 bytes are reserved for non-filesystem
    // data. This conveniently allows for something like a null
    // pointer check.
    if block_index == 0 && offset_within_block < 1024 {
        return Err(block_read_err());
    }

    // The start of the read must be within the block.
    if offset_within_block >= block_size {
        return Err(block_read_err());
    }

    // Get end bound of the read. This must be at the end of the block,
    // or earlier.
    let read_end = usize_from_u32(offset_within_block)
        .checked_add(dst.len())
        .ok_or_else(block_read_err)?;
    if read_end > block_size {
        return Err(block_read_err());
    }

    // Get the absolute byte to start reading from.
    let start_byte = block_index
        .checked_mul(block_size.to_u64())
        .and_then(|v| v.checked_add(u64::from(offset_within_block)))
        .ok_or_else(block_read_err)?;

    reader.read(start_byte, dst).map_err(Ext4Error::Io)
}

#[cfg(feature = "std")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BoxedError;

    struct MockReader(Vec<(u64, usize)>);

    impl Ext4Read for MockReader {
        fn read(
            &mut self,
            start_byte: u64,
            dst: &mut [u8],
        ) -> Result<(), BoxedError> {
            self.0.push((start_byte, dst.len()));
            Ok(())
        }
    }

    /// Test that reading from the first 1024 bytes of the file fails.
    #[test]
    fn test_read_from_block_first_1024() {
        let mut reader = MockReader(Vec::new());
        let block_size = BlockSize::from_superblock_value(0).unwrap();
        let mut dst = vec![0; 1024];
        let block_index = 0;
        let offset_within_block = 1023;
        assert_eq!(
            read_from_block(
                &mut reader,
                block_size,
                block_index,
                offset_within_block,
                &mut dst
            )
            .unwrap_err()
            .as_corrupt()
            .unwrap(),
            &Corrupt::BlockRead {
                block_index,
                offset_within_block,
                read_len: 1024,
            }
        );
    }

    /// Test that reading at an offset >= the block size fails.
    #[test]
    fn test_read_from_block_invalid_offset() {
        let mut reader = MockReader(Vec::new());
        let block_size = BlockSize::from_superblock_value(0).unwrap();
        let mut dst = vec![0; 1024];
        let block_index = 1;
        let offset_within_block = 1024;
        assert_eq!(
            read_from_block(
                &mut reader,
                block_size,
                block_index,
                offset_within_block,
                &mut dst
            )
            .unwrap_err()
            .as_corrupt()
            .unwrap(),
            &Corrupt::BlockRead {
                block_index,
                offset_within_block,
                read_len: 1024,
            }
        );
    }

    /// Test that reading past the end of the block fails.
    #[test]
    fn test_read_from_block_past_the_end() {
        let mut reader = MockReader(Vec::new());
        let block_size = BlockSize::from_superblock_value(0).unwrap();
        let mut dst = vec![0; 25];
        let block_index = 1;
        let offset_within_block = 1000;
        assert_eq!(
            read_from_block(
                &mut reader,
                block_size,
                block_index,
                offset_within_block,
                &mut dst
            )
            .unwrap_err()
            .as_corrupt()
            .unwrap(),
            &Corrupt::BlockRead {
                block_index,
                offset_within_block,
                read_len: 25,
            }
        );
    }
}
