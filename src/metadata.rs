// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::file_type::FileType;
use crate::inode::InodeMode;

/// A point in time recorded in an inode: seconds since the Unix epoch
/// (negative for times before it) and nanoseconds within that second.
///
/// Inodes of 128 bytes store only whole seconds in a 32-bit field; larger
/// inodes may carry two more bits of seconds (extending the range to the
/// year 2446) and the nanoseconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Timestamp {
    /// Whole seconds since 1970-01-01T00:00:00Z.
    pub seconds: i64,
    /// Nanoseconds within the second, less than one billion.
    pub nanoseconds: u32,
}

impl Timestamp {
    /// Decode a timestamp from the base 32-bit seconds field and the
    /// optional extra field (the low two bits extend the seconds, the
    /// rest are nanoseconds), as Linux does.
    pub(crate) fn from_raw(seconds: u32, extra: Option<u32>) -> Self {
        // The base field is signed: it covers 1901 to 2038 on its own.
        let mut seconds = i64::from(i32::from_le_bytes(seconds.to_le_bytes()));
        let mut nanoseconds = 0;
        if let Some(extra) = extra {
            seconds = seconds.saturating_add(i64::from(extra & 0b11) << 32);
            nanoseconds = (extra >> 2).min(999_999_999);
        }
        Self {
            seconds,
            nanoseconds,
        }
    }
}

/// Metadata information about a file.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Metadata {
    /// Size in bytes of the file data.
    pub(crate) size_in_bytes: u64,

    /// Raw permissions and file type.
    pub(crate) mode: InodeMode,

    /// File type parsed from the `mode` bitfield.
    pub(crate) file_type: FileType,

    /// Owner user ID.
    pub(crate) uid: u32,

    /// Owner group ID.
    pub(crate) gid: u32,

    /// Last access time.
    pub(crate) atime: Timestamp,

    /// Last inode change time.
    pub(crate) ctime: Timestamp,

    /// Last data modification time.
    pub(crate) mtime: Timestamp,

    /// Creation time, if the inode is large enough to record it.
    pub(crate) crtime: Option<Timestamp>,
}

impl Metadata {
    /// Get the last access time.
    #[must_use]
    pub fn accessed(&self) -> Timestamp {
        self.atime
    }

    /// Get the last inode change time (the `ctime`: when the metadata or
    /// the data last changed).
    #[must_use]
    pub fn changed(&self) -> Timestamp {
        self.ctime
    }

    /// Get the last data modification time.
    #[must_use]
    pub fn modified(&self) -> Timestamp {
        self.mtime
    }

    /// Get the creation time, if the inode records one (inodes of 128
    /// bytes, as `mke2fs -I 128` or old filesystems have them, do not).
    #[must_use]
    pub fn created(&self) -> Option<Timestamp> {
        self.crtime
    }

    /// Get the file type.
    #[must_use]
    pub fn file_type(&self) -> FileType {
        self.file_type
    }

    /// Return true if this metadata is for a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.file_type.is_dir()
    }

    /// Return true if this metadata is for a symlink.
    #[must_use]
    pub fn is_symlink(&self) -> bool {
        self.file_type.is_symlink()
    }

    /// Get the size in bytes of the file.
    #[allow(clippy::len_without_is_empty)]
    #[must_use]
    pub fn len(&self) -> u64 {
        self.size_in_bytes
    }

    /// Get the file's UNIX permission bits.
    ///
    /// Diagram of the returned value's bits:
    ///
    /// ```text
    ///       top four bits are always zero
    ///       │   
    ///       │   set uid, set gid, sticky bit
    ///       │   │  
    ///       │   │  owner read/write/execute
    ///       │   │  │  
    ///       │   │  │  group read/write/execute
    ///       │   │  │  │  
    ///       │   │  │  │  other read/write/execute
    ///       │   │  │  │  │
    /// (msb) 0000xxxuuugggooo (lsb)
    /// ```
    ///
    /// See `st_mode` in [inode(7)][inode] for more details.
    ///
    /// [inode]: https://www.man7.org/linux/man-pages/man7/inode.7.html
    #[must_use]
    pub fn mode(&self) -> u16 {
        self.mode.bits() & 0o7777
    }

    /// Owner user ID.
    #[must_use]
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// Owner group ID.
    #[must_use]
    pub fn gid(&self) -> u32 {
        self.gid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_from_raw() {
        // Whole seconds only.
        assert_eq!(
            Timestamp::from_raw(1_700_000_000, None),
            Timestamp {
                seconds: 1_700_000_000,
                nanoseconds: 0
            }
        );
        // A time before the epoch: the base field is signed.
        assert_eq!(Timestamp::from_raw(0xffff_ffff, None).seconds, -1);
        // The extra field: two epoch bits and the nanoseconds.
        let extra = (123_456_789u32 << 2) | 0b01;
        assert_eq!(
            Timestamp::from_raw(0xffff_ffff, Some(extra)),
            Timestamp {
                seconds: -1 + (1 << 32),
                nanoseconds: 123_456_789
            }
        );
    }
}
