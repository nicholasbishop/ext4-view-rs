// Copyright 2026 Léo Coletta
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use super::block_header::{JournalBlockHeader, JournalBlockType};
use crate::Ext4;
use crate::checksum::Checksum;
use crate::error::CorruptKind;
use crate::inode::Inode;
use crate::iters::file_blocks::FileBlocks;
use crate::test_util::load_compressed_data;
use crate::uuid::Uuid;

const BLOCK_SIZE: usize = 4096;

/// Reuse the existing filesystem's journal inode, replacing its contents
/// with small, checksummed transactions. No mounts or new tools are needed.
struct JournalImage {
    bytes: Vec<u8>,
    blocks: Vec<usize>,
    uuid: Uuid,
    target: u64,
    position: usize,
    sequence: u32,
}

fn put_u32(block: &mut [u8], offset: usize, value: u32) {
    block[offset..]
        .get_mut(..4)
        .unwrap()
        .copy_from_slice(&value.to_be_bytes());
}

impl JournalImage {
    fn new() -> Self {
        let mut bytes =
            load_compressed_data("test_disk_4k_block_journal.bin.zst");
        let fs = Ext4::load(Box::new(bytes.clone())).unwrap();
        let inode =
            Inode::read(&fs, fs.0.superblock.journal_inode.unwrap()).unwrap();
        let blocks: Vec<_> = FileBlocks::new(fs.clone(), &inode)
            .unwrap()
            .map(|block| usize::try_from(block.unwrap()).unwrap())
            .collect();
        assert_eq!(fs.0.superblock.block_size.to_usize(), BLOCK_SIZE);
        let target = fs.0.superblock.blocks_count.checked_sub(1).unwrap();
        assert!(!blocks.contains(&usize::try_from(target).unwrap()));
        let uuid = {
            let block = bytes.chunks_exact(BLOCK_SIZE).nth(blocks[0]).unwrap();
            Uuid(block[48..64].try_into().unwrap())
        };
        for block in blocks.iter().skip(1) {
            bytes
                .chunks_exact_mut(BLOCK_SIZE)
                .nth(*block)
                .unwrap()
                .fill(0);
        }
        // The filesystem location holds the current data; an older
        // transaction below will temporarily override it with 'a'.
        bytes
            .chunks_exact_mut(BLOCK_SIZE)
            .nth(usize::try_from(target).unwrap())
            .unwrap()
            .fill(b'c');

        let mut image = Self {
            bytes,
            blocks,
            uuid,
            target,
            position: 1,
            sequence: 1,
        };
        image.set_superblock(24, 1);
        image.set_superblock(28, 1);
        image
    }

    fn journal_block(&mut self, index: usize) -> &mut [u8] {
        self.bytes
            .chunks_exact_mut(BLOCK_SIZE)
            .nth(self.blocks[index])
            .unwrap()
    }

    fn set_superblock(&mut self, offset: usize, value: u32) {
        let block = self.journal_block(0);
        put_u32(block, offset, value);
        put_u32(block, 252, 0);
        let mut checksum = Checksum::new();
        checksum.update(&block[..1024]);
        put_u32(block, 252, checksum.finalize());
    }

    fn header(&self, block_type: JournalBlockType) -> Vec<u8> {
        let mut block = vec![0; BLOCK_SIZE];
        put_u32(&mut block, 0, JournalBlockHeader::MAGIC);
        put_u32(&mut block, 4, block_type.0);
        put_u32(&mut block, 8, self.sequence);
        block
    }

    fn push(&mut self, block: &[u8]) {
        self.journal_block(self.position).copy_from_slice(block);
        self.position = self.position.checked_add(1).unwrap();
    }

    fn checksum(&self, block: &[u8]) -> u32 {
        let mut checksum = Checksum::new();
        checksum.update(self.uuid.as_bytes());
        checksum.update(block);
        checksum.finalize()
    }

    fn write(&mut self, value: u8) {
        let data = vec![value; BLOCK_SIZE];
        let mut checksum = Checksum::new();
        checksum.update(self.uuid.as_bytes());
        checksum.update_u32_be(self.sequence);
        checksum.update(&data);

        let mut descriptor = self.header(JournalBlockType::DESCRIPTOR);
        put_u32(&mut descriptor, 12, u32::try_from(self.target).unwrap());
        // Omit the UUID in the tag and mark it as the last tag.
        put_u32(&mut descriptor, 16, 2 | 8);
        put_u32(&mut descriptor, 24, checksum.finalize());
        let checksum = self.checksum(&descriptor);
        put_u32(&mut descriptor, 4092, checksum);
        self.push(&descriptor);
        self.push(&data);
    }

    fn revoke(&mut self) {
        let mut block = self.header(JournalBlockType::REVOCATION);
        // r_count includes the 16-byte header and one 64-bit entry.
        put_u32(&mut block, 12, 24);
        block[16..24].copy_from_slice(&self.target.to_be_bytes());
        let checksum = self.checksum(&block);
        put_u32(&mut block, 4092, checksum);
        self.push(&block);
    }

    fn commit(&mut self) {
        let mut block = self.header(JournalBlockType::COMMIT);
        let checksum = self.checksum(&block);
        put_u32(&mut block, 16, checksum);
        self.push(&block);
        self.sequence = self.sequence.checked_add(1).unwrap();
    }

    fn assert_read(self, value: u8) {
        let fs = Ext4::load(Box::new(self.bytes)).unwrap();
        let mut data = vec![0; BLOCK_SIZE];
        fs.read_from_block(self.target, 0, &mut data).unwrap();
        assert_eq!(
            data.iter().position(|byte| *byte != value),
            None,
            "expected the entire recovered block to contain {value}",
        );
    }

    /// Relocate the transaction across the circular journal's end.
    fn wrap_transaction(&mut self, start: usize, first: usize, end: usize) {
        let transaction: Vec<_> = (1..self.position)
            .map(|index| self.journal_block(index).to_vec())
            .collect();
        for index in 1..self.blocks.len() {
            self.journal_block(index).fill(0);
        }
        let positions = (start..end).chain(first..start);
        for (index, block) in positions.zip(transaction) {
            self.journal_block(index).copy_from_slice(&block);
        }
        self.set_superblock(16, u32::try_from(end).unwrap());
        self.set_superblock(20, u32::try_from(first).unwrap());
        self.set_superblock(28, u32::try_from(start).unwrap());
    }
}

#[test]
fn test_committed_revoke_removes_earlier_mapping() {
    let mut image = JournalImage::new();
    image.write(b'a');
    image.commit();
    image.revoke();
    image.commit();
    image.assert_read(b'c');
}

#[test]
fn test_uncommitted_revoke_preserves_earlier_mapping() {
    let mut image = JournalImage::new();
    image.write(b'a');
    image.commit();
    image.revoke();
    image.assert_read(b'a');
}

#[test]
fn test_later_write_replaces_revoked_mapping() {
    let mut image = JournalImage::new();
    image.write(b'a');
    image.commit();
    image.revoke();
    image.commit();
    image.write(b'b');
    image.commit();
    image.assert_read(b'b');
}

#[test]
fn test_invalid_commit_does_not_apply_revoke() {
    let mut image = JournalImage::new();
    image.write(b'a');
    image.commit();
    image.revoke();
    image.commit();
    let commit = image.position.checked_sub(1).unwrap();
    image.journal_block(commit)[16] ^= 1;
    image.assert_read(b'a');
}

#[test]
fn test_transaction_wraps_at_journal_end() {
    // Wrap either between descriptor/data or between data/commit.
    for tail_blocks in [1, 2] {
        let mut image = JournalImage::new();
        image.write(b'a');
        image.commit();
        let end = image.blocks.len();
        let start = end.checked_sub(tail_blocks).unwrap();
        image.wrap_transaction(start, 1, end);
        image.assert_read(b'a');
    }
}

#[test]
fn test_wrap_respects_first_and_max_len() {
    let mut image = JournalImage::new();
    image.write(b'a');
    image.commit();
    let end = image.blocks.len().checked_sub(1).unwrap();
    image.wrap_transaction(end.checked_sub(1).unwrap(), 2, end);
    image.assert_read(b'a');
}

#[test]
fn test_zero_start_is_empty() {
    let mut image = JournalImage::new();
    image.write(b'a');
    image.commit();
    image.set_superblock(28, 0);
    image.assert_read(b'c');
}

#[test]
fn test_invalid_journal_bounds() {
    // first == 0, first >= max_len, start outside [first, max_len),
    // and max_len extending beyond the journal inode.
    for (first, start, end) in [
        (0, 1, 1024),
        (1024, 1, 1024),
        (2, 1, 1024),
        (1, 1024, 1024),
        (1, 1, u32::MAX),
    ] {
        let mut image = JournalImage::new();
        image.set_superblock(16, end);
        image.set_superblock(20, first);
        image.set_superblock(28, start);
        assert_eq!(
            Ext4::load(Box::new(image.bytes)).unwrap_err(),
            CorruptKind::JournalSize,
            "first={first}, start={start}, max_len={end}",
        );
    }
}
