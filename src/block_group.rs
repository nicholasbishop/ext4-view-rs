// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::checksum::Checksum;
use crate::error::{Corrupt, Ext4Error};
use crate::features::ReadOnlyCompatibleFeatures;
use crate::superblock::Superblock;
use crate::util::{read_u16le, read_u32le, u64_from_hilo, usize_from_u32};
use crate::Ext4Read;
use alloc::vec;
use alloc::vec::Vec;

pub(crate) type BlockGroupIndex = u32;

#[derive(Debug)]
pub(crate) struct BlockGroupDescriptor {
    pub(crate) inode_table_first_block: u64,
    checksum: u16,
}

impl BlockGroupDescriptor {
    const BG_CHECKSUM_OFFSET: usize = 0x1e;

    fn from_bytes(
        bgd_index: BlockGroupIndex,
        bytes: &[u8],
    ) -> Result<Self, Ext4Error> {
        const BG_INODE_TABLE_HI_OFFSET: usize = 0x28;

        if bytes.len() < (BG_INODE_TABLE_HI_OFFSET + 4) {
            return Err(Ext4Error::Corrupt(Corrupt::BlockGroupDescriptor(
                bgd_index,
            )));
        }

        let bg_inode_table_lo = read_u32le(bytes, 0x8);
        let bg_checksum = read_u16le(bytes, Self::BG_CHECKSUM_OFFSET);
        let bg_inode_table_hi = read_u32le(bytes, BG_INODE_TABLE_HI_OFFSET);

        let inode_table_first_block =
            u64_from_hilo(bg_inode_table_hi, bg_inode_table_lo);

        Ok(Self {
            inode_table_first_block,
            checksum: bg_checksum,
        })
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

        let bgd_start_block = if sb.block_size == 1024 { 2 } else { 1 };
        let bgd_per_block =
            sb.block_size / u32::from(sb.block_group_descriptor_size);
        let block_index = bgd_start_block + (bgd_index / bgd_per_block);
        let offset_within_block = (bgd_index % bgd_per_block)
            * u32::from(sb.block_group_descriptor_size);

        let start = u64::from(block_index) * u64::from(sb.block_size)
            + u64::from(offset_within_block);
        reader.read(start, &mut data).map_err(Ext4Error::Io)?;

        let block_group_descriptor = Self::from_bytes(bgd_index, &data)?;

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
                return Err(Ext4Error::Corrupt(
                    Corrupt::BlockGroupDescriptorChecksum(bgd_index),
                ));
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
