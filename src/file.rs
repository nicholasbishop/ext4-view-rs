// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::error::Ext4Error;
use crate::inode::Inode;
use crate::iters::file_blocks::FileBlocks;
use crate::metadata::Metadata;
use crate::path::Path;
use crate::resolve::FollowSymlinks;
use crate::util::u64_from_usize;
use crate::Ext4;
use alloc::vec;
use alloc::vec::Vec;

/// An open file within an [`Ext4`] filesystem.
pub struct File {
    fs: Ext4,
    inode: Inode,
    position: u64,
    block: Vec<u8>,
    file_blocks: FileBlocks,
    read_next_block: bool,
    offset_within_block: usize,
}

impl File {
    /// Open the file at `path`.
    pub(crate) fn open(fs: &Ext4, path: Path<'_>) -> Result<Self, Ext4Error> {
        let inode = fs.path_to_inode(path, FollowSymlinks::All)?;

        if inode.metadata.is_dir() {
            return Err(Ext4Error::IsADirectory);
        }
        if !inode.metadata.file_type.is_regular_file() {
            return Err(Ext4Error::IsASpecialFile);
        }

        Ok(Self {
            fs: fs.clone(),
            position: 0,
            // TODO: lazy init?
            block: vec![0; fs.0.superblock.block_size.to_usize()],
            file_blocks: FileBlocks::new(fs.clone(), &inode)?,
            inode,
            read_next_block: true,
            offset_within_block: 0,
        })
    }

    /// Get the file metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.inode.metadata
    }

    // TODO: will this conflict with Read trait annoyingly?
    // TODO: maybeunit?

    /// Read bytes from the file into `buf`, returning how many bytes
    /// were read.
    ///
    /// This advanced the position of the file by the number of bytes
    /// read, so calling `read_bytes` repeatedly can be used to read the
    /// entire file.
    ///
    /// Returns `Ok(0)` if the end of the file has been reached.
    pub fn read_bytes(&mut self, buf: &mut [u8]) -> Result<usize, Ext4Error> {
        // Nothing to do if output buffer is empty.
        if buf.is_empty() {
            return Ok(0);
        }

        // Nothing to do if already at the end of the file.
        if self.position >= self.inode.metadata.size_in_bytes {
            return Ok(0);
        }

        // let mut block;
        let block_size = self.fs.0.superblock.block_size;
        // if buf.len() < block_size {
        //     block = vec![0; block_size];
        // }

        // TODO: avoid always copying from internal buffer

        if self.read_next_block {
            if let Some(block_index) = self.file_blocks.next() {
                // TODO: impl ez conv from Ext4Error to io::Error
                let block_index = block_index?;
                self.read_next_block = false;
                if block_index == 0 {
                    self.block.fill(0);
                } else {
                    self.fs.read_bytes(
                        block_index
                            .checked_mul(block_size.to_u64())
                            .ok_or(Ext4Error::FileTooLarge)?,
                        &mut self.block,
                    )?;
                }
            } else {
                // End of file reached.
                return Ok(0);
            }
        }

        // let Some(offset_within_block) = self.offset_within_block else {
        //     //self.fs.read_bytes(
        //     0
        // };

        let offset_within_block = self.offset_within_block;

        // if this is the last block in the file, cap
        // TODO: move to `open` once we figure out calculation...
        let unused_bytes_in_last_block =
            u64::from(self.inode.file_size_in_blocks()) * block_size.to_u64()
                - self.inode.metadata.size_in_bytes;
        let max_read_in_last_block =
            block_size.to_u64() - unused_bytes_in_last_block;
        let mut max_read_in_block = self.block.len();

        if (self.position + block_size.to_u64())
            > self.inode.metadata.size_in_bytes
        {
            // TODO
            max_read_in_block = max_read_in_last_block as usize;
        }

        // OK to unwrap: `offset_within_block` is always less than or
        // equal to the block length.
        // TODO: not good unwrap comment anymore
        let bytes_remaining_in_block =
            max_read_in_block.checked_sub(offset_within_block).unwrap();

        let bytes_to_copy = buf.len().min(bytes_remaining_in_block);

        // OK to unwrap: this sum is at most the block size.
        let end = offset_within_block.checked_add(bytes_to_copy).unwrap();

        buf[..bytes_to_copy]
            .copy_from_slice(&self.block[offset_within_block..end]);

        // Advance offset
        self.offset_within_block = end;

        if self.offset_within_block >= self.block.len() {
            self.read_next_block = true;
            self.offset_within_block = 0;
        }

        // TODO
        self.position = self
            .position
            .checked_add(u64_from_usize(bytes_to_copy))
            .unwrap();

        Ok(bytes_to_copy)
    }

    /// Current position within the file.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Seek to `position` within the file.
    pub fn seek(&mut self, position: u64) -> Result<(), Ext4Error> {
        self.position = position;

        // Reset iteration.
        self.file_blocks = FileBlocks::new(self.fs.clone(), &self.inode)?;
        self.read_next_block = true;

        let block_size = self.fs.0.superblock.block_size.to_nz_u64();
        let num_blocks = position / block_size;
        for _ in 0..num_blocks {
            self.file_blocks.next().unwrap()?;
        }

        // OK to unwrap: the offset is less than the block size. The
        // block size always fits in a `u32`, and we assume `usize` is
        // at least as big as a `u32`.
        self.offset_within_block =
            usize::try_from(position % block_size).unwrap();

        Ok(())
    }
}

// TODO: impl Read/Seek with std feature
