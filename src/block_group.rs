// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::error::{Corrupt, Ext4Error};
use crate::util::{read_u16le, read_u32le, u64_from_hilo};

pub(crate) type BlockGroupIndex = u32;

#[derive(Debug)]
pub(crate) struct BlockGroupDescriptor {
    pub(crate) inode_table_first_block: u64,
    checksum: u16,
}

impl BlockGroupDescriptor {
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
        let bg_checksum = read_u16le(bytes, 0x1e);
        let bg_inode_table_hi = read_u32le(bytes, BG_INODE_TABLE_HI_OFFSET);

        let inode_table_first_block =
            u64_from_hilo(bg_inode_table_hi, bg_inode_table_lo);

        Ok(Self {
            inode_table_first_block,
            checksum: bg_checksum,
        })
    }
}
