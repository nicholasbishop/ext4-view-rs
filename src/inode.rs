// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::Ext4;
use crate::block_index::FsBlockIndex;
use crate::checksum::Checksum;
use crate::error::{CorruptKind, Ext4Error};
use crate::file_type::FileType;
use crate::metadata::Metadata;
use crate::path::PathBuf;
use crate::util::{
    read_u16le, read_u32le, u32_from_hilo, u64_from_hilo, usize_from_u32,
};
use alloc::vec;
use alloc::vec::Vec;
use bitflags::bitflags;
use core::num::NonZeroU32;
use core::time::Duration;

/// Inode index.
///
/// This is always nonzero.
pub(crate) type InodeIndex = NonZeroU32;

bitflags! {
    /// Inode flags.
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct InodeFlags: u32 {
        /// File is immutable.
        const IMMUTABLE = 0x10;

        /// Directory is encrypted.
        const DIRECTORY_ENCRYPTED = 0x800;

        /// Directory has hashed indexes.
        const DIRECTORY_HTREE = 0x1000;

        /// File is huge.
        const HUGE_FILE = 0x4_0000;

        /// Inode uses extents.
        const EXTENTS = 0x8_0000;

        /// Verity protected data.
        const VERITY = 0x10_0000;

        /// Inode stores a large extended attribute value in its data blocks.
        const EXTENDED_ATTRIBUTES = 0x20_0000;

        /// Inode has inline data.
        const INLINE_DATA = 0x1000_0000;

        // TODO: other flags
    }
}

bitflags! {
    /// Inode mode.
    ///
    /// The mode bitfield stores file permissions in the lower bits and
    /// file type in the upper bits.
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct InodeMode: u16 {
        const S_IXOTH = 0x0001;
        const S_IWOTH = 0x0002;
        const S_IROTH = 0x0004;

        const S_IXGRP = 0x0008;
        const S_IWGRP = 0x0010;
        const S_IRGRP = 0x0020;

        const S_IXUSR = 0x0040;
        const S_IWUSR = 0x0080;
        const S_IRUSR = 0x0100;

        const S_ISVTX = 0x0200;

        const S_ISGID = 0x0400;
        const S_ISUID = 0x0800;

        // Mutually-exclusive file types:
        const S_IFIFO = 0x1000;
        const S_IFCHR = 0x2000;
        const S_IFDIR = 0x4000;
        const S_IFBLK = 0x6000;
        const S_IFREG = 0x8000;
        const S_IFLNK = 0xA000;
        const S_IFSOCK = 0xC000;
    }
}

fn timestamp_to_duration(timestamp: u32, _high: Option<u32>) -> Duration {
    if timestamp == u32::MAX {
        panic!("timestamp overflow");
    }
    // TODO: nanosecond precision
    Duration::from_secs(u64::from(timestamp))
}

/// An inode within an Ext4 filesystem.
#[derive(Clone, Debug)]
pub struct Inode {
    /// This inode's index.
    pub index: InodeIndex,

    /// Various kinds of file data can be stored within the inode, including:
    /// * The root node of the extent tree.
    /// * Target path for symlinks.
    pub inline_data: [u8; Self::INLINE_DATA_LEN],

    /// Metadata about the file.
    pub metadata: Metadata,

    /// Full inode data as read from disk.
    pub inode_data: Vec<u8>,

    /// Internal inode flags.
    pub flags: InodeFlags,

    /// Checksum seed used in various places.
    pub checksum_base: Checksum,

    /// Number of blocks in the file (including holes).
    file_size_in_blocks: u32,
}

impl Inode {
    const INLINE_DATA_LEN: usize = 60;
    const L_I_CHECKSUM_LO_OFFSET: usize = 0x74 + 0x8;
    const I_CHECKSUM_HI_OFFSET: usize = 0x82;

    /// Load an inode from `bytes`.
    ///
    /// If successful, returns a tuple containing the inode and its
    /// checksum field.
    fn from_bytes(
        ext4: &Ext4,
        index: InodeIndex,
        data: &[u8],
    ) -> Result<(Self, u32), Ext4Error> {
        // Inodes must be at least 128 bytes.
        if data.len() < 128 {
            return Err(CorruptKind::InodeTruncated {
                inode: index,
                size: data.len(),
            }
            .into());
        }

        // If metadata checksums are enabled, the inode must be big
        // enough to include the checksum fields.
        if ext4.has_metadata_checksums()
            && data.len() < (Self::I_CHECKSUM_HI_OFFSET + 2)
        {
            return Err(CorruptKind::InodeTruncated {
                inode: index,
                size: data.len(),
            }
            .into());
        }

        let i_mode = read_u16le(data, 0x0);
        let i_uid = read_u16le(data, 0x2);
        let i_size_lo = read_u32le(data, 0x4);
        let i_atime = read_u32le(data, 0x8);
        let i_ctime = read_u32le(data, 0xc);
        let i_mtime = read_u32le(data, 0x10);
        let i_dtime = read_u32le(data, 0x14);
        let i_gid = read_u16le(data, 0x18);
        let i_flags = read_u32le(data, 0x20);
        // OK to unwrap: already checked the length.
        let i_block = data.get(0x28..0x28 + Self::INLINE_DATA_LEN).unwrap();
        let i_generation = read_u32le(data, 0x64);
        let i_size_high = read_u32le(data, 0x6c);
        let l_i_uid_high = read_u16le(data, 0x74 + 0x4);
        let l_i_gid_high = read_u16le(data, 0x74 + 0x6);
        let (l_i_checksum_lo, i_checksum_hi) = if ext4.has_metadata_checksums()
        {
            (
                read_u16le(data, Self::L_I_CHECKSUM_LO_OFFSET),
                read_u16le(data, Self::I_CHECKSUM_HI_OFFSET),
            )
        } else {
            // If metadata checksums aren't enabled then these values
            // aren't used; arbitrarily set to zero.
            (0, 0)
        };

        let size_in_bytes = u64_from_hilo(i_size_high, i_size_lo);
        let uid = u32_from_hilo(l_i_uid_high, i_uid);
        let gid = u32_from_hilo(l_i_gid_high, i_gid);
        let checksum = u32_from_hilo(i_checksum_hi, l_i_checksum_lo);
        let mode = InodeMode::from_bits_retain(i_mode);

        let mut checksum_base =
            Checksum::with_seed(ext4.0.superblock.checksum_seed);
        checksum_base.update_u32_le(index.get());
        checksum_base.update_u32_le(i_generation);

        let file_size_in_blocks: u32 = size_in_bytes
            // Round up.
            .div_ceil(ext4.0.superblock.block_size.to_u64())
            // Ext4 allows at most `2^32` blocks in a file.
            .try_into()
            .map_err(|_| CorruptKind::TooManyBlocksInFile)?;

        Ok((
            Self {
                index,
                // OK to unwap, we know `i_block` is 60 bytes.
                inline_data: i_block.try_into().unwrap(),
                metadata: Metadata {
                    size_in_bytes,
                    mode,
                    uid,
                    gid,
                    atime: timestamp_to_duration(i_atime, None),
                    ctime: timestamp_to_duration(i_ctime, None),
                    dtime: timestamp_to_duration(i_dtime, None),
                    file_type: FileType::try_from(mode).map_err(|_| {
                        CorruptKind::InodeFileType { inode: index, mode }
                    })?,
                    mtime: timestamp_to_duration(i_mtime, None),
                },
                inode_data: data.to_vec(),
                flags: InodeFlags::from_bits_retain(i_flags),
                checksum_base,
                file_size_in_blocks,
            },
            checksum,
        ))
    }

    /// Read an inode.
    pub async fn read(
        ext4: &Ext4,
        inode: InodeIndex,
    ) -> Result<Self, Ext4Error> {
        let (block_index, offset_within_block) =
            get_inode_location(ext4, inode)?;

        let mut data = vec![0; usize::from(ext4.0.superblock.inode_size)];
        ext4.read_from_block(block_index, offset_within_block, &mut data)
            .await?;

        let (inode, expected_checksum) = Self::from_bytes(ext4, inode, &data)?;

        // Verify the inode checksum.
        if ext4.has_metadata_checksums() {
            let mut checksum = inode.checksum_base.clone();

            // Hash all the inode data, but treat the two checksum
            // fields as zeroes.

            // Up to the l_i_checksum_lo field.
            checksum.update(&data[..Self::L_I_CHECKSUM_LO_OFFSET]);

            // Zero'd field.
            checksum.update_u16_le(0);

            // Up to the i_checksum_hi field.
            checksum.update(
                &data[Self::L_I_CHECKSUM_LO_OFFSET + 2
                    ..Self::I_CHECKSUM_HI_OFFSET],
            );

            // Zero'd field.
            checksum.update_u16_le(0);

            // Rest of the inode.
            checksum.update(&data[Self::I_CHECKSUM_HI_OFFSET + 2..]);

            let actual_checksum = checksum.finalize();
            if actual_checksum != expected_checksum {
                return Err(CorruptKind::InodeChecksum(inode.index).into());
            }
        }

        Ok(inode)
    }

    pub(crate) fn update_inode_data(&mut self, ext4: &Ext4) {
        // i_mode
        self.inode_data[0x0..0x2]
            .copy_from_slice(&self.metadata.mode.bits().to_le_bytes());
        // i_uid
        self.inode_data[0x2..0x4]
            .copy_from_slice(&(self.metadata.uid as u16).to_le_bytes());
        // i_size_lo
        self.inode_data[0x4..0x8].copy_from_slice(
            &u32::try_from(self.metadata.size_in_bytes)
                .unwrap_or(0)
                .to_le_bytes(),
        );
        // i_atime
        if self.metadata.atime.as_secs() > u32::MAX as u64 {
            panic!("atime overflow");
        }
        self.inode_data[0x8..0xc].copy_from_slice(
            &(self.metadata.atime.as_secs() as u32).to_le_bytes(),
        );
        // i_ctime
        if self.metadata.ctime.as_secs() > u32::MAX as u64 {
            panic!("ctime overflow");
        }
        self.inode_data[0xc..0x10].copy_from_slice(
            &(self.metadata.ctime.as_secs() as u32).to_le_bytes(),
        );
        // i_mtime
        if self.metadata.mtime.as_secs() > u32::MAX as u64 {
            panic!("mtime overflow");
        }
        self.inode_data[0x10..0x14].copy_from_slice(
            &(self.metadata.mtime.as_secs() as u32).to_le_bytes(),
        );
        // i_dtime
        if self.metadata.dtime.as_secs() > u32::MAX as u64 {
            panic!("dtime overflow");
        }
        self.inode_data[0x14..0x18].copy_from_slice(
            &(self.metadata.dtime.as_secs() as u32).to_le_bytes(),
        );
        // i_gid
        self.inode_data[0x18..0x1a]
            .copy_from_slice(&(self.metadata.gid as u16).to_le_bytes());
        // i_flags
        self.inode_data[0x20..0x24]
            .copy_from_slice(&self.flags.bits().to_le_bytes());
        // i_size_hi
        self.inode_data[0x6c..0x70].copy_from_slice(
            &((self.metadata.size_in_bytes >> 32) as u32).to_le_bytes(),
        );
        // TODO: update other fields as need
        if ext4.has_metadata_checksums() {
            let mut checksum = self.checksum_base.clone();
            // Up to the l_i_checksum_lo field.
            checksum.update(&self.inode_data[..Self::L_I_CHECKSUM_LO_OFFSET]);
            // Zero'd field.
            checksum.update_u16_le(0);
            // Up to the i_checksum_hi field.
            checksum.update(
                &self.inode_data[Self::L_I_CHECKSUM_LO_OFFSET + 2
                    ..Self::I_CHECKSUM_HI_OFFSET],
            );
            // Zero'd field.
            checksum.update_u16_le(0);
            // Rest of the inode.
            checksum.update(&self.inode_data[Self::I_CHECKSUM_HI_OFFSET + 2..]);
            let final_checksum = checksum.finalize();
            self.inode_data[Self::L_I_CHECKSUM_LO_OFFSET
                ..Self::L_I_CHECKSUM_LO_OFFSET + 2]
                .copy_from_slice(&(final_checksum as u16).to_le_bytes());
            self.inode_data
                [Self::I_CHECKSUM_HI_OFFSET..Self::I_CHECKSUM_HI_OFFSET + 2]
                .copy_from_slice(
                    &((final_checksum >> 16) as u16).to_le_bytes(),
                );
        }
    }

    /// Write the inode back to disk.
    pub async fn write(&mut self, ext4: &Ext4) -> Result<(), Ext4Error> {
        let (block_index, offset_within_block) =
            get_inode_location(ext4, self.index)?;
        let block_size = ext4.0.superblock.block_size.to_u64();
        let pos = block_index * block_size + u64::from(offset_within_block);
        self.update_inode_data(ext4);
        // Write only the data we've saved to avoid overwriting any unread info
        let writer = ext4.0.writer.as_ref().ok_or(Ext4Error::Readonly)?;
        writer
            .write(pos, &self.inode_data)
            .await
            .map_err(Ext4Error::Io)?;
        Ok(())
    }

    /// Get the target path of a symlink inode.
    pub async fn symlink_target(
        &self,
        ext4: &Ext4,
    ) -> Result<PathBuf, Ext4Error> {
        if !self.metadata.is_symlink() {
            return Err(Ext4Error::NotASymlink);
        }

        // An empty symlink target is not allowed.
        if self.metadata.size_in_bytes == 0 {
            return Err(CorruptKind::SymlinkTarget(self.index).into());
        }

        // Symlink targets of up to 59 bytes are stored inline. Longer
        // targets are stored as regular file data.
        const MAX_INLINE_SYMLINK_LEN: u64 = 59;

        if self.metadata.size_in_bytes <= MAX_INLINE_SYMLINK_LEN {
            // OK to unwrap since we checked the size above.
            let len = usize::try_from(self.metadata.size_in_bytes).unwrap();
            let target = &self.inline_data[..len];

            PathBuf::try_from(target)
                .map_err(|_| CorruptKind::SymlinkTarget(self.index).into())
        } else {
            let data = ext4.read_inode_file(self).await?;
            PathBuf::try_from(data)
                .map_err(|_| CorruptKind::SymlinkTarget(self.index).into())
        }
    }

    /// Get the number of blocks in the file.
    ///
    /// If the file size is not an even multiple of the block size,
    /// round up.
    ///
    /// # Errors
    ///
    /// Ext4 allows at most `2^32` blocks in a file. Returns
    /// `CorruptKind::TooManyBlocksInFile` if that limit is exceeded.
    pub fn file_size_in_blocks(&self) -> u32 {
        self.file_size_in_blocks
    }
}

/// Get an inode's location: block index and offset within that block.
/// Note that this is the location of the inode itself, not the file
/// data associated with the inode.
fn get_inode_location(
    ext4: &Ext4,
    inode: InodeIndex,
) -> Result<(FsBlockIndex, u32), Ext4Error> {
    let sb = &ext4.0.superblock;

    // OK to unwrap: `inode` is nonzero.
    let inode_minus_1 = inode.get().checked_sub(1).unwrap();

    let block_group_index = inode_minus_1 / sb.inodes_per_block_group;

    let group = ext4
        .0
        .block_group_descriptors
        .get(usize_from_u32(block_group_index))
        .ok_or(CorruptKind::InodeBlockGroup {
            inode,
            block_group: block_group_index,
            num_block_groups: ext4.0.block_group_descriptors.len(),
        })?;

    let index_within_group = inode_minus_1 % sb.inodes_per_block_group;

    let err = || CorruptKind::InodeLocation {
        inode,
        block_group: block_group_index,
        inodes_per_block_group: sb.inodes_per_block_group,
        inode_size: sb.inode_size,
        block_size: sb.block_size,
        inode_table_first_block: group.inode_table_first_block,
    };

    let byte_offset_within_group = u64::from(index_within_group)
        .checked_mul(u64::from(sb.inode_size))
        .ok_or_else(err)?;

    let byte_offset_of_group = sb
        .block_size
        .to_u64()
        .checked_mul(group.inode_table_first_block)
        .ok_or_else(err)?;

    // Absolute byte index of the inode.
    let start_byte = byte_offset_of_group
        .checked_add(byte_offset_within_group)
        .ok_or_else(err)?;

    let block_index = start_byte / sb.block_size.to_nz_u64();
    let offset_within_block =
        u32::try_from(start_byte % sb.block_size.to_nz_u64())
            .map_err(|_| err())?;

    Ok((block_index, offset_within_block))
}
