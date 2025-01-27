// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::features::IncompatibleFeatures;
use alloc::boxed::Box;
use core::error::Error;
use core::fmt::{self, Debug, Display, Formatter};

/// Boxed error, used for IO errors. This is similar in spirit to
/// `anyhow::Error`, although a much simpler implementation.
pub(crate) type BoxedError = Box<dyn Error + Send + Sync + 'static>;

/// Common error type for all [`Ext4`] operations.
///
/// [`Ext4`]: crate::Ext4
#[derive(Debug)]
#[non_exhaustive]
pub enum Ext4Error {
    /// An operation that requires an absolute path was attempted on a
    /// relative path.
    NotAbsolute,

    /// An operation that requires a symlink was attempted on a
    /// non-symlink file.
    NotASymlink,

    /// A path points to a non-existent file.
    NotFound,

    /// An operation that requires a non-directory path was attempted on
    /// a directory path.
    IsADirectory,

    /// An operation that requires a directory path was attempted on a
    /// non-directory path.
    NotADirectory,

    /// An operation that requires a regular file (or a symlink to a
    /// regular file) was attempted on a special file (fifo, character
    /// device, block device, or socket).
    IsASpecialFile,

    /// The file cannot be read into memory because it is too large.
    FileTooLarge,

    /// Data is not valid UTF-8.
    NotUtf8,

    /// Data cannot be converted into a valid path.
    MalformedPath,

    /// Path is too long.
    ///
    /// Maximum path length is not strictly enforced by this library for
    /// all paths, but during path resolution the length may not exceed
    /// 4096 bytes.
    PathTooLong,

    /// Path could not be resolved because it contains too many levels
    /// of symbolic links.
    TooManySymlinks,

    /// An IO operation failed. This error comes from the [`Ext4Read`]
    /// passed to [`Ext4::load`].
    ///
    /// [`Ext4::load`]: crate::Ext4::load
    /// [`Ext4Read`]: crate::Ext4Read
    Io(
        /// Underlying error.
        BoxedError,
    ),

    /// The filesystem is not supported by this library. This does not
    /// indicate a problem with the filesystem, or with the calling
    /// code. Please file a feature request and include the incompatible
    /// features.
    Incompatible(Incompatible),

    /// The filesystem is corrupt in some way.
    Corrupt(Corrupt),
}

impl Ext4Error {
    /// If the error type is [`Ext4Error::Corrupt`], get the underlying error.
    #[must_use]
    pub fn as_corrupt(&self) -> Option<&Corrupt> {
        if let Self::Corrupt(err) = self {
            Some(err)
        } else {
            None
        }
    }

    /// If the error type is [`Ext4Error::Incompatible`], get the underlying error.
    #[must_use]
    pub fn as_incompatible(&self) -> Option<&Incompatible> {
        if let Self::Incompatible(err) = self {
            Some(err)
        } else {
            None
        }
    }

    /// If the error type is [`Ext4Error::Io`], get the underlying error.
    #[must_use]
    pub fn as_io(&self) -> Option<&(dyn Error + Send + Sync + 'static)> {
        if let Self::Io(err) = self {
            Some(&**err)
        } else {
            None
        }
    }
}

impl Display for Ext4Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAbsolute => write!(f, "path is not absolute"),
            Self::NotASymlink => write!(f, "path is not a symlink"),
            Self::NotFound => write!(f, "file not found"),
            Self::IsADirectory => write!(f, "path is a directory"),
            Self::NotADirectory => write!(f, "path is not a directory"),
            Self::IsASpecialFile => write!(f, "path is a special file"),
            Self::FileTooLarge => {
                write!(f, "file is too large to store in memory")
            }
            Self::NotUtf8 => write!(f, "data is not utf-8"),
            Self::MalformedPath => write!(f, "data is not a valid path"),
            Self::PathTooLong => write!(f, "path is too long"),
            Self::TooManySymlinks => {
                write!(f, "too many levels of symbolic links")
            }
            // TODO: if the `Error` trait ever makes it into core, stop
            // printing `err` here and return it via `Error::source` instead.
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Incompatible(i) => write!(f, "incompatible: {i}"),
            Self::Corrupt(c) => write!(f, "corrupt: {c}"),
        }
    }
}

impl Error for Ext4Error {}

#[cfg(feature = "std")]
impl From<Ext4Error> for std::io::Error {
    fn from(e: Ext4Error) -> Self {
        use std::io::ErrorKind::*;

        // TODO: Rust 1.83 adds NotADirectory, IsADirectory, and
        // FileTooLarge to std::io::Error; use those after bumping the
        // MSRV.
        match e {
            Ext4Error::IsADirectory
            | Ext4Error::IsASpecialFile
            | Ext4Error::MalformedPath
            | Ext4Error::NotADirectory
            | Ext4Error::NotASymlink
            | Ext4Error::NotAbsolute => InvalidInput.into(),
            Ext4Error::Corrupt(_)
            | Ext4Error::FileTooLarge
            | Ext4Error::Incompatible(_)
            | Ext4Error::PathTooLong
            | Ext4Error::TooManySymlinks => Self::other(e),
            Ext4Error::Io(inner) => Self::other(inner),
            Ext4Error::NotFound => NotFound.into(),
            Ext4Error::NotUtf8 => InvalidData.into(),
        }
    }
}

/// Error type used in [`Ext4Error::Corrupt`] when the filesystem is
/// corrupt in some way.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Corrupt(CorruptKind);

impl Display for Corrupt {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        <CorruptKind as Display>::fmt(&self.0, f)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub(crate) enum CorruptKind {
    /// Superblock magic is invalid.
    SuperblockMagic,

    /// Superblock checksum is invalid.
    SuperblockChecksum,

    /// The block size in the superblock is invalid.
    InvalidBlockSize,

    /// The number of block groups does not fit in a [`u32`].
    TooManyBlockGroups,

    /// The number of inodes per block group is zero.
    InodesPerBlockGroup,

    /// The inode size exceeds the block size.
    InodeSize,

    /// The journal inode in the superblock is invalid.
    JournalInode,

    /// Invalid first data block.
    FirstDataBlock(
        /// First data block.
        u32,
    ),

    /// Invalid block group descriptor.
    BlockGroupDescriptor(
        /// Block group number.
        u32,
    ),

    /// Block group descriptor checksum is invalid.
    BlockGroupDescriptorChecksum(
        /// Block group number.
        u32,
    ),

    /// An inode's checksum is invalid.
    InodeChecksum(
        /// Inode number.
        u32,
    ),

    /// An inode is invalid.
    Inode(
        /// Inode number.
        u32,
    ),

    /// The target of a symlink is not a valid path.
    SymlinkTarget(
        /// Inode number.
        u32,
    ),

    /// The number of blocks in a file exceeds 2^32.
    TooManyBlocksInFile,

    /// An extent's magic is invalid.
    ExtentMagic(
        /// Inode number.
        u32,
    ),

    /// An extent's checksum is invalid.
    ExtentChecksum(
        /// Inode number.
        u32,
    ),

    /// An extent's depth is greater than five.
    ExtentDepth(
        /// Inode number.
        u32,
    ),

    /// Not enough data is present to read an extent node.
    ExtentNotEnoughData(
        /// Inode number.
        u32,
    ),

    /// An extent points to an invalid block.
    ExtentBlock(
        /// Inode number.
        u32,
    ),

    /// An extent node's size exceeds the block size.
    ExtentNodeSize(
        /// Inode number.
        u32,
    ),

    /// A directory block's checksum is invalid.
    DirBlockChecksum(
        /// Inode number.
        u32,
    ),

    // TODO: consider breaking this down into more specific problems.
    /// A directory entry is invalid.
    DirEntry(
        /// Inode number.
        u32,
    ),

    /// Invalid read of a block.
    BlockRead {
        /// Absolute block index.
        block_index: u64,

        /// Offset in bytes within the block.
        offset_within_block: u32,

        /// Length in bytes of the read.
        read_len: usize,
    },
}

impl Display for CorruptKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::SuperblockMagic => write!(f, "invalid superblock magic"),
            Self::SuperblockChecksum => {
                write!(f, "invalid superblock checksum")
            }
            Self::InvalidBlockSize => write!(f, "invalid block size"),
            Self::TooManyBlockGroups => write!(f, "too many block groups"),
            Self::InodesPerBlockGroup => {
                write!(f, "inodes per block group is zero")
            }
            Self::InodeSize => write!(f, "inode size is invalid"),
            Self::JournalInode => write!(f, "invalid journal inode"),
            Self::FirstDataBlock(block) => {
                write!(f, "invalid first data block: {block}")
            }
            Self::BlockGroupDescriptor(block_group_num) => {
                write!(f, "block group descriptor {block_group_num} is invalid")
            }
            Self::BlockGroupDescriptorChecksum(block_group_num) => write!(
                f,
                "invalid checksum for block group descriptor {block_group_num}"
            ),
            Self::InodeChecksum(inode) => {
                write!(f, "invalid checksum for inode {inode}")
            }
            Self::Inode(inode) => write!(f, "inode {inode} is invalid"),
            Self::SymlinkTarget(inode) => {
                write!(f, "inode {inode} has an invalid symlink path")
            }
            Self::TooManyBlocksInFile => write!(f, "too many blocks in file"),
            Self::ExtentMagic(inode) => {
                write!(f, "extent in inode {inode} has invalid magic")
            }
            Self::ExtentChecksum(inode) => {
                write!(f, "extent in inode {inode} has an invalid checksum")
            }
            Self::ExtentDepth(inode) => {
                write!(f, "extent in inode {inode} has an invalid depth")
            }
            Self::ExtentNotEnoughData(inode) => {
                write!(f, "extent data in inode {inode} is invalid")
            }
            Self::ExtentBlock(inode) => {
                write!(f, "extent in inode {inode} points to an invalid block")
            }
            Self::ExtentNodeSize(inode) => {
                write!(
                    f,
                    "extent in inode {inode} has a node with an invalid size"
                )
            }
            Self::DirBlockChecksum(inode) => write!(
                f,
                "directory block in inode {inode} has an invalid checksum"
            ),
            Self::DirEntry(inode) => {
                write!(f, "invalid directory entry in inode {inode}")
            }
            Self::BlockRead {
                block_index,
                offset_within_block,
                read_len,
            } => {
                write!(f, "invalid read of length {read_len} from block {block_index} at offset {offset_within_block}")
            }
        }
    }
}

impl PartialEq<CorruptKind> for Ext4Error {
    fn eq(&self, ck: &CorruptKind) -> bool {
        if let Self::Corrupt(c) = self {
            c.0 == *ck
        } else {
            false
        }
    }
}

impl From<Corrupt> for Ext4Error {
    fn from(c: Corrupt) -> Self {
        Self::Corrupt(c)
    }
}

impl From<CorruptKind> for Corrupt {
    fn from(c: CorruptKind) -> Self {
        Self(c)
    }
}

impl From<CorruptKind> for Ext4Error {
    fn from(c: CorruptKind) -> Self {
        Self::Corrupt(Corrupt(c))
    }
}

impl From<Incompatible> for Ext4Error {
    fn from(i: Incompatible) -> Self {
        Self::Incompatible(i)
    }
}

/// Error type used in [`Ext4Error::Incompatible`] when the filesystem
/// cannot be read due to incomplete support in this library.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Incompatible {
    /// One or more unknown bits are set in the incompatible feature flags.
    Unknown(
        /// The unknown features.
        IncompatibleFeatures,
    ),

    /// One or more required incompatible features are missing.
    Missing(
        /// The missing features.
        IncompatibleFeatures,
    ),

    /// One or more disallowed incompatible features are present.
    #[allow(clippy::enum_variant_names)]
    Incompatible(
        /// The incompatible features.
        IncompatibleFeatures,
    ),

    /// The directory hash algorithm is not supported.
    DirectoryHash(
        /// The algorithm identifier.
        u8,
    ),

    /// Attempted to read an encrypted directory. Only unencrypted
    /// directories are currently supported.
    DirectoryEncrypted(
        /// Inode number.
        u32,
    ),
}

impl Display for Incompatible {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(feat) => {
                write!(f, "unknown features: {feat:?}")
            }
            Self::Missing(feat) => {
                write!(f, "missing required features: {feat:?}")
            }
            Self::Incompatible(feat) => {
                write!(f, "incompatible features: {feat:?}")
            }
            Self::DirectoryHash(algorithm) => {
                write!(f, "unsupported directory hash algorithm: {algorithm}")
            }
            Self::DirectoryEncrypted(inode) => {
                write!(f, "directory in inode {inode} is encrypted")
            }
        }
    }
}
