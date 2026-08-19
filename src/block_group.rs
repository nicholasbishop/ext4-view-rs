// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::Ext4Read;
use crate::block_index::FsBlockIndex;
use crate::checksum::Checksum;
use crate::error::{CorruptKind, Ext4Error};
use crate::features::{IncompatibleFeatures, ReadOnlyCompatibleFeatures};
use crate::superblock::Superblock;
use crate::util::{read_u16le, read_u32le, u64_from_hilo, usize_from_u32};
use alloc::vec;
use alloc::vec::Vec;

pub(crate) type BlockGroupIndex = u32;

#[derive(Debug)]
pub(crate) struct BlockGroupDescriptor {
    pub(crate) inode_table_first_block: FsBlockIndex,
    checksum: u16,
}

impl BlockGroupDescriptor {
    const BG_CHECKSUM_OFFSET: usize = 0x1e;

    fn from_bytes(superblock: &Superblock, bytes: &[u8]) -> Self {
        const BG_INODE_TABLE_HI_OFFSET: usize = 0x28;

        let bg_inode_table_lo = read_u32le(bytes, 0x8);
        let bg_checksum = read_u16le(bytes, Self::BG_CHECKSUM_OFFSET);

        // Get the high bits of the inode table block.
        let bg_inode_table_hi = if superblock
            .incompatible_features
            .contains(IncompatibleFeatures::IS_64BIT)
        {
            read_u32le(bytes, BG_INODE_TABLE_HI_OFFSET)
        } else {
            0
        };

        let inode_table_first_block =
            u64_from_hilo(bg_inode_table_hi, bg_inode_table_lo);

        Self {
            inode_table_first_block,
            checksum: bg_checksum,
        }
    }

    /// Whether a block group holds a copy of the superblock, under `SPARSE_SUPER`.
    ///
    /// Groups 0 and 1, and every power of 3, 5 and 7. `META_BLOCK_GROUPS` does not change this:
    /// that feature moves the *descriptor* blocks and leaves the superblock backups where
    /// `SPARSE_SUPER` puts them.
    fn group_has_superblock(group: u32) -> bool {
        if group <= 1 {
            return true;
        }
        [3u32, 5, 7].iter().any(|base| {
            let mut power = *base;
            loop {
                if power == group {
                    return true;
                }
                match power.checked_mul(*base) {
                    Some(next) if next <= group => power = next,
                    _ => return false,
                }
            }
        })
    }

    /// Map from a block group descriptor index to the absolute byte
    /// within the file where the descriptor starts.
    ///
    /// Two layouts. Without `META_BLOCK_GROUPS` the descriptor table is one run beginning in the
    /// block after the superblock. With it, the table is cut into one block per meta block group —
    /// `block_size / descriptor_size` groups' worth — and each such block is stored inside the
    /// groups it describes rather than at the start of the filesystem. The kernel's documentation
    /// states the placement: "a single block group descriptor block is placed at the beginning of
    /// the first, second, and last block groups in a meta-block group". This returns the first of
    /// those, which is the copy e2fsprogs treats as primary.
    ///
    /// The feature exists because the contiguous table has to fit in group 0, which stops being
    /// possible above about 1TB with a 1KiB block size — `mke2fs` switches to this layout there.
    fn get_start_byte(
        sb: &Superblock,
        bgd_index: BlockGroupIndex,
    ) -> Option<u64> {
        let bgd_per_block = sb
            .block_size
            .to_u32()
            .checked_div(u32::from(sb.block_group_descriptor_size))?;
        let table_block = bgd_index.checked_div(bgd_per_block)?;
        let offset_within_block = (bgd_index.checked_rem(bgd_per_block)?)
            .checked_mul(u32::from(sb.block_group_descriptor_size))?;

        let block_index: u64 = match sb.first_meta_block_group {
            Some(first) if table_block >= first => {
                // The meta block group's own block, at the start of the first group it describes,
                // after that group's superblock backup where it has one.
                let first_group = table_block.checked_mul(bgd_per_block)?;
                let group_start = u64::from(sb.first_data_block).checked_add(
                    u64::from(first_group)
                        .checked_mul(u64::from(sb.blocks_per_group))?,
                )?;
                group_start.checked_add(u64::from(
                    Self::group_has_superblock(first_group),
                ))?
            }
            _ => {
                let bgd_start_block: u32 =
                    if sb.block_size == 1024 { 2 } else { 1 };
                u64::from(bgd_start_block.checked_add(table_block)?)
            }
        };

        block_index
            .checked_mul(sb.block_size.to_u64())?
            .checked_add(u64::from(offset_within_block))
    }

    /// Read a block group descriptor.
    fn read(
        sb: &Superblock,
        reader: &mut dyn Ext4Read,
        bgd_index: BlockGroupIndex,
    ) -> Result<Self, Ext4Error> {
        // Allocate a byte vec to read the raw data into.
        let block_group_descriptor_size =
            usize::from(sb.block_group_descriptor_size);
        let mut data = vec![0; block_group_descriptor_size];

        let start = Self::get_start_byte(sb, bgd_index)
            .ok_or(CorruptKind::BlockGroupDescriptor(bgd_index))?;
        reader.read(start, &mut data).map_err(Ext4Error::Io)?;

        let block_group_descriptor = Self::from_bytes(sb, &data);

        let has_metadata_checksums = sb
            .read_only_compatible_features
            .contains(ReadOnlyCompatibleFeatures::METADATA_CHECKSUMS);

        // Verify the descriptor checksum.
        if has_metadata_checksums {
            let mut checksum = Checksum::with_seed(sb.checksum_seed);
            checksum.update_u32_le(bgd_index);
            // Up to the checksum field.
            checksum.update(&data[..Self::BG_CHECKSUM_OFFSET]);
            // Zero'd checksum field.
            checksum.update_u16_le(0);
            // Rest of the block group descriptor.
            checksum.update(&data[Self::BG_CHECKSUM_OFFSET + 2..]);
            // Truncate to the lower 16 bits.
            let checksum = u16::try_from(checksum.finalize() & 0xffff).unwrap();

            if checksum != block_group_descriptor.checksum {
                return Err(CorruptKind::BlockGroupDescriptorChecksum(
                    bgd_index,
                )
                .into());
            }
        } else if sb
            .read_only_compatible_features
            .contains(ReadOnlyCompatibleFeatures::GROUP_DESCRIPTOR_CHECKSUMS)
        {
            // TODO: prior to general checksum metadata being added,
            // there was a separate feature just for block group
            // descriptors. Add support for that here.
        }

        Ok(block_group_descriptor)
    }

    /// Read all block group descriptors.
    pub(crate) fn read_all(
        sb: &Superblock,
        reader: &mut dyn Ext4Read,
    ) -> Result<Vec<Self>, Ext4Error> {
        let mut block_group_descriptors =
            Vec::with_capacity(usize_from_u32(sb.num_block_groups));

        for bgd_index in 0..sb.num_block_groups {
            let bgd = Self::read(sb, reader, bgd_index)?;
            block_group_descriptors.push(bgd);
        }

        Ok(block_group_descriptors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_size::BlockSize;
    use crate::label::Label;
    use crate::uuid::Uuid;
    use core::num::NonZero;

    /// A 1KiB-block, 64-bit filesystem of 128 block groups: eight meta block groups of sixteen.
    fn meta_bg_superblock(
        blocks_count: u64,
        first_meta_block_group: Option<u32>,
    ) -> Superblock {
        Superblock {
            block_size: BlockSize::from_superblock_value(0).unwrap(),
            blocks_count,
            inode_size: 256,
            inodes_per_block_group: NonZero::new(2048).unwrap(),
            block_group_descriptor_size: 64,
            first_data_block: 1,
            blocks_per_group: 8192,
            first_meta_block_group,
            num_block_groups: u32::try_from(
                blocks_count.saturating_sub(1).div_ceil(8192),
            )
            .unwrap(),
            incompatible_features: IncompatibleFeatures::FILE_TYPE_IN_DIR_ENTRY
                | IncompatibleFeatures::EXTENTS
                | IncompatibleFeatures::IS_64BIT,
            read_only_compatible_features:
                ReadOnlyCompatibleFeatures::SPARSE_SUPERBLOCKS,
            checksum_seed: 0,
            htree_hash_seed: [0; 4],
            journal_inode: None,
            label: Label::new([0; 16]),
            uuid: Uuid::new([0; 16]),
        }
    }

    /// `META_BLOCK_GROUPS` stores each descriptor block inside the groups it describes.
    ///
    /// The numbers come from `dumpe2fs` on a filesystem built with
    /// `mke2fs -t ext4 -O meta_bg,^resize_inode -b 1024 -F img 1048576`, which reports the
    /// descriptor blocks at 2, 8194, 122881, 131073, 139265, 253953, 262145, 270337, …
    #[test]
    fn meta_block_group_descriptor_locations() {
        let sb = meta_bg_superblock(1_048_576, Some(0));
        assert_eq!(sb.num_block_groups, 128);

        // Meta block group 0 covers groups 0..15, and its block is in group 0 after the primary
        // superblock: block 2.
        assert_eq!(
            BlockGroupDescriptor::get_start_byte(&sb, 0),
            Some(2 * 1024)
        );
        assert_eq!(
            BlockGroupDescriptor::get_start_byte(&sb, 15),
            Some(2 * 1024 + 15 * 64)
        );

        // Meta block group 1 covers 16..31, and group 16 carries no superblock backup, so its
        // block is that group's first: 131073.
        assert_eq!(
            BlockGroupDescriptor::get_start_byte(&sb, 16),
            Some(131_073 * 1024)
        );
        assert_eq!(
            BlockGroupDescriptor::get_start_byte(&sb, 31),
            Some(131_073 * 1024 + 15 * 64)
        );
        // And meta block group 2.
        assert_eq!(
            BlockGroupDescriptor::get_start_byte(&sb, 32),
            Some(262_145 * 1024)
        );

        // Without the feature the table is one run beginning after the superblock.
        let old = meta_bg_superblock(1_048_576, None);
        assert_eq!(
            BlockGroupDescriptor::get_start_byte(&old, 0),
            Some(2 * 1024)
        );
        assert_eq!(
            BlockGroupDescriptor::get_start_byte(&old, 16),
            Some(2 * 1024 + 16 * 64)
        );

        // `s_first_meta_bg` counts descriptor blocks, so blocks below it keep the old layout.
        let mixed = meta_bg_superblock(1_048_576, Some(2));
        assert_eq!(
            BlockGroupDescriptor::get_start_byte(&mixed, 31),
            Some(2 * 1024 + 31 * 64)
        );
        assert_eq!(
            BlockGroupDescriptor::get_start_byte(&mixed, 32),
            Some(262_145 * 1024)
        );
    }

    /// The groups carrying a superblock backup are `SPARSE_SUPER`'s, which `META_BLOCK_GROUPS`
    /// does not change — measured on the same filesystem, whose backups are in groups 0, 1, 3, 5,
    /// 7, 9, 25, 27, 49, 81 and 125.
    #[test]
    fn superblock_backups_follow_sparse_super() {
        let sparse: Vec<u32> = (0..128)
            .filter(|g| BlockGroupDescriptor::group_has_superblock(*g))
            .collect();
        assert_eq!(sparse, [0, 1, 3, 5, 7, 9, 25, 27, 49, 81, 125]);
    }
}
