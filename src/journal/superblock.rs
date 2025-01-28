// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::checksum::Checksum;
use crate::inode::Inode;
use crate::iters::file_blocks::FileBlocks;
use crate::journal::block_header::{JournalBlockHeader, JournalBlockType};
use crate::util::read_u32be;
use crate::uuid::Uuid;
use crate::{CorruptKind, Ext4, Ext4Error, Incompatible};
use alloc::vec;
use bitflags::bitflags;

const JOURNAL_SUPERBLOCK_SIZE: usize = 1024;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct JournalSuperblock {
    /// Size in bytes of journal blocks. This must be the same block
    /// size as the main filesystem.
    pub(super) block_size: u32,

    /// Sequence number of the first journal commit to apply.
    pub(super) sequence: u32,

    /// Index of the journal block from which to start reading
    /// data. This index is relative to the journal superblock.
    pub(super) start_block: u32,

    /// Journal UUID. Used for checksums.
    pub(super) uuid: Uuid,
}

impl JournalSuperblock {
    /// Load the journal superblock from the filesystem.
    ///
    /// An error is returned if:
    /// * The superblock cannot be read from the filesystem.
    /// * `JournalSuperblock::read_bytes` fails.
    pub(super) fn load(
        fs: &Ext4,
        journal_inode: &Inode,
    ) -> Result<Self, Ext4Error> {
        // Get an iterator over the journal's block indices.
        let mut journal_block_iter =
            FileBlocks::new(fs.clone(), journal_inode)?;

        // Read the first 1024 bytes of the first block. This is the
        // journal's superblock.
        let block_index = journal_block_iter
            .next()
            .ok_or(CorruptKind::JournalSize)??;
        let mut block = vec![0; JOURNAL_SUPERBLOCK_SIZE];
        fs.read_from_block(block_index, 0, &mut block)?;

        let superblock = Self::read_bytes(&block)?;

        Ok(superblock)
    }

    /// Read superblock data from `bytes`.
    ///
    /// An error is returned if:
    /// * The superblock magic number is not present.
    /// * The superblock type is unsupported.
    /// * The checksum type is unsupported.
    /// * The superblock's checksum is incorrect.
    fn read_bytes(bytes: &[u8]) -> Result<Self, Ext4Error> {
        assert_eq!(bytes.len(), JOURNAL_SUPERBLOCK_SIZE);

        const CHECKSUM_TYPE_CRC32C: u8 = 4;
        const CHECKSUM_OFFSET: usize = 0xfc;

        let header = JournalBlockHeader::read_bytes(bytes)?
            .ok_or(CorruptKind::JournalMagic)?;

        // For now only superblock v2 is supported.
        if header.block_type != JournalBlockType::SUPERBLOCK_V2 {
            return Err(Incompatible::JournalSuperblockType(
                header.block_type.0,
            )
            .into());
        }

        let s_blocksize = read_u32be(bytes, 0xc);
        let s_sequence = read_u32be(bytes, 0x18);
        let s_start = read_u32be(bytes, 0x1c);
        let s_feature_incompat = read_u32be(bytes, 0x28);
        let s_uuid = &bytes[0x30..0x30 + 16];
        let s_checksum_type = bytes[0x50];
        let s_checksum = read_u32be(bytes, CHECKSUM_OFFSET);

        // Check that features required by this library are present, and
        // that no unsupported features are present.
        let incompat_features =
            JournalIncompatibleFeatures::from_bits_retain(s_feature_incompat);
        let required_incompat_features = JournalIncompatibleFeatures::IS_64BIT
            | JournalIncompatibleFeatures::CHECKSUM_V3;
        if incompat_features != required_incompat_features {
            return Err(Incompatible::JournalIncompatibleFeatures(
                s_feature_incompat,
            )
            .into());
        }

        // For now only one checksum type is supported.
        if s_checksum_type != CHECKSUM_TYPE_CRC32C {
            return Err(
                Incompatible::JournalChecksumType(s_checksum_type).into()
            );
        }

        // Validate the superblock checksum.
        let mut checksum = Checksum::new();
        checksum.update(&bytes[..CHECKSUM_OFFSET]);
        checksum.update_u32_le(0);
        checksum.update(&bytes[CHECKSUM_OFFSET + 4..JOURNAL_SUPERBLOCK_SIZE]);
        if checksum.finalize() != s_checksum {
            return Err(CorruptKind::JournalSuperblockChecksum.into());
        }

        // OK to unwrap: `s_uuid` is always 16 bytes.
        let uuid = Uuid(s_uuid.try_into().unwrap());

        Ok(Self {
            block_size: s_blocksize,
            sequence: s_sequence,
            start_block: s_start,
            uuid,
        })
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct JournalIncompatibleFeatures: u32 {
        const BLOCK_REVOCATIONS = 0x1;
        const IS_64BIT = 0x2;
        const ASYNC_COMMITS = 0x4;
        const CHECKSUM_V2 = 0x8;
        const CHECKSUM_V3 = 0x10;
        const FAST_COMMITS = 0x20;
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::test_util::load_compressed_filesystem;

    #[test]
    fn test_load_journal_superblock() {
        let fs =
            load_compressed_filesystem("test_disk_4k_block_journal.bin.zst");
        let journal_inode =
            Inode::read(&fs, fs.0.superblock.journal_inode.unwrap()).unwrap();
        let superblock = JournalSuperblock::load(&fs, &journal_inode).unwrap();
        assert_eq!(
            superblock,
            JournalSuperblock {
                block_size: 4096,
                sequence: 3,
                start_block: 289,
                uuid: Uuid([
                    0x6c, 0x48, 0x4f, 0x1b, 0x7f, 0x71, 0x47, 0x4c, 0xa1, 0xf9,
                    0x3b, 0x50, 0x0c, 0xc1, 0xe2, 0x74
                ]),
            }
        );
    }

    fn create_test_superblock() -> Vec<u8> {
        let mut block = vec![0; 1024];
        // Set magic.
        block[..4].copy_from_slice(&JournalBlockHeader::MAGIC.to_be_bytes());
        // Set superblock type.
        block[4..8].copy_from_slice(&4u32.to_be_bytes());
        // Set block size.
        block[0xc..0xc + 4].copy_from_slice(&4096u32.to_be_bytes());
        // Set sequence.
        block[0x18..0x18 + 4].copy_from_slice(&123u32.to_be_bytes());
        // Set start block.
        block[0x1c..0x1c + 4].copy_from_slice(&456u32.to_be_bytes());
        // Set features.
        block[0x28..0x28 + 4].copy_from_slice(&0x12u32.to_be_bytes());
        // Set UUID.
        block[0x30..0x30 + 16].copy_from_slice(&[0xab; 16]);
        // Set checksum type.
        block[0x50] = 4;
        // Set checksum.
        block[0xfc..0xfc + 4].copy_from_slice(&0x78a2_c32bu32.to_be_bytes());
        block
    }

    #[test]
    fn test_journal_superblock_read_success() {
        let block = create_test_superblock();
        assert_eq!(
            JournalSuperblock::read_bytes(&block).unwrap(),
            JournalSuperblock {
                block_size: 4096,
                sequence: 123,
                start_block: 456,
                uuid: Uuid([0xab; 16]),
            }
        );
    }

    #[test]
    fn test_journal_superblock_invalid_magic() {
        let mut block = create_test_superblock();
        // Set magic.
        block[..4].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(
            JournalSuperblock::read_bytes(&block).unwrap_err(),
            CorruptKind::JournalMagic
        );
    }

    #[test]
    fn test_journal_superblock_unsupported_type() {
        let mut block = create_test_superblock();
        // Set superblock type.
        block[4..8].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(
            JournalSuperblock::read_bytes(&block).unwrap_err(),
            Incompatible::JournalSuperblockType(0)
        );
    }

    #[test]
    fn test_journal_superblock_missing_required_features() {
        let mut block = create_test_superblock();
        // Set features.
        block[0x28..0x28 + 4].copy_from_slice(&0x10u32.to_be_bytes());
        assert_eq!(
            JournalSuperblock::read_bytes(&block).unwrap_err(),
            Incompatible::JournalIncompatibleFeatures(0x10),
        );
    }

    #[test]
    fn test_journal_superblock_unsupported_checksum_type() {
        let mut block = create_test_superblock();
        // Set checksum type.
        block[0x50] = 0;
        assert_eq!(
            JournalSuperblock::read_bytes(&block).unwrap_err(),
            Incompatible::JournalChecksumType(0)
        );
    }

    #[test]
    fn test_journal_superblock_incorrect_checksum() {
        let mut block = create_test_superblock();
        // Set checksum.
        block[0xfc..0xfc + 4].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(
            JournalSuperblock::read_bytes(&block).unwrap_err(),
            CorruptKind::JournalSuperblockChecksum,
        );
    }
}
