// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::error::{Corrupt, Ext4Error};
use crate::format::{format_bytes_debug, BytesDisplay};
use crate::inode::InodeIndex;
use crate::path::{Path, PathBuf};
use crate::util::{read_u16le, read_u32le};
use alloc::rc::Rc;
use core::fmt::{self, Debug, Display, Formatter};
use core::hash::{Hash, Hasher};
use core::str::Utf8Error;

/// Error returned when [`DirEntryName`] construction fails.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DirEntryNameError {
    /// Name is empty.
    Empty,

    /// Name is longer than [`DirEntryName::MAX_LEN`].
    TooLong,

    /// Name contains a null byte.
    ContainsNull,

    /// Name contains a path separator.
    ContainsSeparator,
}

impl Display for DirEntryNameError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "direntry name is empty"),
            Self::TooLong => {
                write!(f, "directory entry name is longer than 255 bytes")
            }
            Self::ContainsNull => {
                write!(f, "directory entry name contains a null byte")
            }
            Self::ContainsSeparator => {
                write!(f, "directory entry name contains a path separator")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for DirEntryNameError {}

/// Name of a [`DirEntry`], stored as a reference.
///
/// This is guaranteed at construction to be a valid directory entry
/// name.
#[derive(Clone, Copy, Eq, Ord, PartialOrd, Hash)]
pub struct DirEntryName<'a>(pub(crate) &'a [u8]);

impl<'a> DirEntryName<'a> {
    /// Maximum length of a `DirEntryName`.
    pub const MAX_LEN: usize = 255;

    /// Convert to a `&str` if the name is valid UTF-8.
    #[inline]
    pub fn as_str(&self) -> Result<&'a str, Utf8Error> {
        core::str::from_utf8(self.0)
    }

    /// Get an object that implements [`Display`] to allow conveniently
    /// printing names that may or may not be valid UTF-8. Non-UTF-8
    /// characters will be replaced with '�'.
    ///
    /// [`Display`]: core::fmt::Display
    pub fn display(&self) -> BytesDisplay {
        BytesDisplay(self.0)
    }
}

impl<'a> AsRef<[u8]> for DirEntryName<'a> {
    fn as_ref(&self) -> &'a [u8] {
        self.0
    }
}

impl<'a> Debug for DirEntryName<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        format_bytes_debug(self.0, f)
    }
}

impl<'a, T> PartialEq<T> for DirEntryName<'a>
where
    T: AsRef<[u8]>,
{
    fn eq(&self, other: &T) -> bool {
        self.0 == other.as_ref()
    }
}

impl<'a> TryFrom<&'a [u8]> for DirEntryName<'a> {
    type Error = DirEntryNameError;

    fn try_from(bytes: &'a [u8]) -> Result<Self, DirEntryNameError> {
        if bytes.is_empty() {
            Err(DirEntryNameError::Empty)
        } else if bytes.len() > Self::MAX_LEN {
            Err(DirEntryNameError::TooLong)
        } else if bytes.contains(&0) {
            Err(DirEntryNameError::ContainsNull)
        } else if bytes.contains(&Path::SEPARATOR) {
            Err(DirEntryNameError::ContainsSeparator)
        } else {
            Ok(Self(bytes))
        }
    }
}

impl<'a, const N: usize> TryFrom<&'a [u8; N]> for DirEntryName<'a> {
    type Error = DirEntryNameError;

    fn try_from(bytes: &'a [u8; N]) -> Result<Self, DirEntryNameError> {
        Self::try_from(bytes.as_slice())
    }
}

impl<'a> TryFrom<&'a str> for DirEntryName<'a> {
    type Error = DirEntryNameError;

    fn try_from(s: &'a str) -> Result<Self, DirEntryNameError> {
        Self::try_from(s.as_bytes())
    }
}

#[derive(Clone, Eq, Ord, PartialOrd)]
struct DirEntryNameBuf {
    data: [u8; DirEntryName::MAX_LEN],
    len: u8,
}

impl DirEntryNameBuf {
    #[inline]
    #[must_use]
    fn as_bytes(&self) -> &[u8] {
        &self.data[..usize::from(self.len)]
    }

    #[inline]
    #[must_use]
    fn as_dir_entry_name(&self) -> DirEntryName<'_> {
        DirEntryName(self.as_bytes())
    }
}

impl Debug for DirEntryNameBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        format_bytes_debug(self.as_bytes(), f)
    }
}

// Manual implementation of `PartialEq` because we don't want to compare
// the entire `data` array, only up to `len`.
impl PartialEq<DirEntryNameBuf> for DirEntryNameBuf {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

// Manual implementation of `Hash` because we don't want to include the
// entire `data` array, only up to `len` (see also `PartialEq` impl).
impl Hash for DirEntryNameBuf {
    fn hash<H>(&self, hasher: &mut H)
    where
        H: Hasher,
    {
        self.as_bytes().hash(hasher);
    }
}

impl TryFrom<&[u8]> for DirEntryNameBuf {
    type Error = DirEntryNameError;

    fn try_from(bytes: &[u8]) -> Result<Self, DirEntryNameError> {
        // This performs all the necessary validation of the input.
        DirEntryName::try_from(bytes)?;

        let mut name = DirEntryNameBuf {
            data: [0; DirEntryName::MAX_LEN],
            // OK to unwrap: already checked against `MAX_LEN`.
            len: u8::try_from(bytes.len()).unwrap(),
        };
        name.data[..bytes.len()].copy_from_slice(bytes);
        Ok(name)
    }
}

/// Directory entry.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DirEntry {
    /// Number of the inode that this entry points to.
    pub(crate) inode: InodeIndex,

    /// Raw name of the entry.
    name: DirEntryNameBuf,

    /// Path that `read_dir` was called with. This is shared via `Rc` so
    /// that only one allocation is required.
    path: Rc<PathBuf>,
}

impl DirEntry {
    /// Read a `DirEntry` from a byte slice.
    ///
    /// If no error occurs, this returns `(Option<DirEntry>, usize)`.
    /// * The first value in this tuple is an `Option` because some
    ///   special data is stored in directory blocks that aren't
    ///   actually directory entries. If the inode pointed to by the
    ///   entry is zero, this value is set to None.
    /// * The `usize` in this tuple is the overall length of the entry's
    ///   data. This is used when iterating over raw dir entry data.
    pub(crate) fn from_bytes(
        bytes: &[u8],
        inode: InodeIndex,
        path: Rc<PathBuf>,
    ) -> Result<(Option<Self>, usize), Ext4Error> {
        const NAME_OFFSET: usize = 8;

        let err = || Ext4Error::Corrupt(Corrupt::DirEntry(inode.get()));

        // Check size (the full entry will usually be larger than this),
        // but these header fields must be present.
        if bytes.len() < NAME_OFFSET {
            return Err(err());
        }

        // Get the inode that this entry points to. If zero, this is a
        // special type of entry (such as a checksum entry or hash tree
        // node entry).
        let points_to_inode = read_u32le(bytes, 0);

        // Get the full size of the entry.
        let rec_len = read_u16le(bytes, 4);
        let rec_len = usize::from(rec_len);

        // Check that the rec_len is somewhat reasonable. Too small a
        // value could indicate the wrong data is being read. And
        // notably, a value of zero would cause an infinite loop when
        // iterating over entries.
        if rec_len < NAME_OFFSET {
            return Err(err());
        }

        // As described above, an inode of zero is used for special
        // entries. Return early since the rest of the fields won't be
        // valid.
        let Some(points_to_inode) = InodeIndex::new(points_to_inode) else {
            return Ok((None, rec_len));
        };

        // Get the size of the entry's name field.
        // OK to unwrap: already checked length.
        let name_len = *bytes.get(6).unwrap();
        let name_len_usize = usize::from(name_len);

        // Get the entry's name.
        let name_slice = bytes
            .get(NAME_OFFSET..NAME_OFFSET + name_len_usize)
            .ok_or(err())?;

        let name = DirEntryNameBuf::try_from(name_slice).map_err(|_| err())?;
        let entry = DirEntry {
            inode: points_to_inode,
            name,
            path,
        };
        Ok((Some(entry), rec_len))
    }

    /// Get the directory entry's name.
    #[must_use]
    #[inline]
    pub fn file_name(&self) -> DirEntryName<'_> {
        self.name.as_dir_entry_name()
    }

    /// Get the entry's path.
    ///
    /// This appends the entry's name to the path that `Ext4::read_dir`
    /// was called with.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.path.join(self.name.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::DefaultHasher;

    #[test]
    fn test_dir_entry_debug() {
        let src = "abc😁\n".as_bytes();
        let expected = "abc😁\\n"; // Note the escaped slash.
        assert_eq!(format!("{:?}", DirEntryName(src)), expected);

        let mut src_vec = src.to_vec();
        src_vec.resize(255, 0);
        assert_eq!(
            format!(
                "{:?}",
                DirEntryNameBuf {
                    data: src_vec.try_into().unwrap(),
                    len: src.len().try_into().unwrap(),
                }
            ),
            expected
        );
    }

    #[test]
    fn test_dir_entry_display() {
        let name = DirEntryName([0xc3, 0x28].as_slice());
        assert_eq!(format!("{}", name.display()), "�(");
    }

    #[test]
    fn test_dir_entry_construction() {
        let expected_name = DirEntryName(b"abc");
        let mut v = b"abc".to_vec();
        v.resize(255, 0);
        let expected_name_buf = DirEntryNameBuf {
            data: v.try_into().unwrap(),
            len: 3,
        };

        // Successful construction from a byte slice.
        let src: &[u8] = b"abc";
        assert_eq!(DirEntryName::try_from(src).unwrap(), expected_name);
        assert_eq!(DirEntryNameBuf::try_from(src).unwrap(), expected_name_buf);

        // Successful construction from a string.
        let src: &str = "abc";
        assert_eq!(DirEntryName::try_from(src).unwrap(), expected_name);

        // Successful construction from a byte array.
        let src: &[u8; 3] = b"abc";
        assert_eq!(DirEntryName::try_from(src).unwrap(), expected_name);

        // Error: empty.
        let src: &[u8] = b"";
        assert_eq!(DirEntryName::try_from(src), Err(DirEntryNameError::Empty));
        assert_eq!(
            DirEntryNameBuf::try_from(src),
            Err(DirEntryNameError::Empty)
        );

        // Error: too long.
        let src: &[u8] = [1; 256].as_slice();
        assert_eq!(
            DirEntryName::try_from(src),
            Err(DirEntryNameError::TooLong)
        );
        assert_eq!(
            DirEntryNameBuf::try_from(src),
            Err(DirEntryNameError::TooLong)
        );

        // Error:: contains null.
        let src: &[u8] = b"\0".as_slice();
        assert_eq!(
            DirEntryName::try_from(src),
            Err(DirEntryNameError::ContainsNull)
        );
        assert_eq!(
            DirEntryNameBuf::try_from(src),
            Err(DirEntryNameError::ContainsNull)
        );

        // Error: contains separator.
        let src: &[u8] = b"/".as_slice();
        assert_eq!(
            DirEntryName::try_from(src),
            Err(DirEntryNameError::ContainsSeparator)
        );
        assert_eq!(
            DirEntryNameBuf::try_from(src),
            Err(DirEntryNameError::ContainsSeparator)
        );
    }

    #[test]
    fn test_dir_entry_name_buf_hash() {
        fn get_hash<T: Hash>(v: T) -> u64 {
            let mut s = DefaultHasher::new();
            v.hash(&mut s);
            s.finish()
        }

        let name = DirEntryNameBuf::try_from(b"abc".as_slice()).unwrap();
        assert_eq!(get_hash(name), get_hash(b"abc"));
    }

    #[test]
    fn test_dir_entry_from_bytes() {
        let inode1 = InodeIndex::new(1).unwrap();
        let inode2 = InodeIndex::new(2).unwrap();
        let path = Rc::new(PathBuf::new("path"));

        // Read a normal entry.
        let mut bytes = Vec::new();
        bytes.extend(2u32.to_le_bytes()); // inode
        bytes.extend(72u16.to_le_bytes()); // record length
        bytes.push(3u8); // name length
        bytes.push(8u8); // file type
        bytes.extend("abc".bytes()); // name
        bytes.resize(72, 0u8);
        let (entry, len) =
            DirEntry::from_bytes(&bytes, inode1, path.clone()).unwrap();
        let entry = entry.unwrap();
        assert_eq!(len, 72);
        assert_eq!(
            entry,
            DirEntry {
                inode: inode2,
                name: DirEntryNameBuf::try_from("abc".as_bytes()).unwrap(),
                path: path.clone(),
            },
        );
        assert_eq!(entry.file_name(), "abc");
        assert_eq!(entry.path(), "path/abc");

        // Special entry: inode is zero.
        let mut bytes = Vec::new();
        bytes.extend(0u32.to_le_bytes()); // inode
        bytes.extend(72u16.to_le_bytes()); // record length
        bytes.resize(72, 0u8);
        assert_eq!(
            DirEntry::from_bytes(&bytes, inode1, path.clone()).unwrap(),
            (None, 72)
        );

        // Error: not enough data.
        let err = DirEntry::from_bytes(&[], inode1, path.clone()).unwrap_err();
        assert_eq!(*err.as_corrupt().unwrap(), Corrupt::DirEntry(1));

        // Error: not enough data for the name.
        let mut bytes = Vec::new();
        bytes.extend(2u32.to_le_bytes()); // inode
        bytes.extend(72u16.to_le_bytes()); // record length
        bytes.push(3u8); // name length
        bytes.push(8u8); // file type
        bytes.extend("a".bytes()); // name
        assert!(DirEntry::from_bytes(&bytes, inode1, path.clone()).is_err());

        // Error: name contains invalid characters.
        let mut bytes = Vec::new();
        bytes.extend(2u32.to_le_bytes()); // inode
        bytes.extend(72u16.to_le_bytes()); // record length
        bytes.push(3u8); // name length
        bytes.push(8u8); // file type
        bytes.extend("ab/".bytes()); // name
        bytes.resize(72, 0u8);
        assert!(DirEntry::from_bytes(&bytes, inode1, path).is_err());
    }

    #[test]
    fn test_dir_entry_name_as_ref() {
        let name = DirEntryName::try_from(b"abc".as_slice()).unwrap();
        let bytes: &[u8] = name.as_ref();
        assert_eq!(bytes, b"abc");
    }

    #[test]
    fn test_dir_entry_name_partial_eq() {
        let name = DirEntryName::try_from(b"abc".as_slice()).unwrap();
        assert_eq!(name, name);

        let v: &str = "abc";
        assert_eq!(name, v);

        let v: &[u8] = b"abc";
        assert_eq!(name, v);

        let v: &[u8; 3] = b"abc";
        assert_eq!(name, v);
    }

    #[test]
    fn test_dir_entry_name_buf_as_dir_entry_name() {
        let name = DirEntryNameBuf::try_from(b"abc".as_slice()).unwrap();
        let r: DirEntryName<'_> = name.as_dir_entry_name();
        assert_eq!(r, "abc");
    }

    #[test]
    fn test_dir_entry_name_as_str() {
        let name = DirEntryName::try_from(b"abc".as_slice()).unwrap();
        assert_eq!(name.as_str().unwrap(), "abc");

        let name = DirEntryName([0xc3, 0x28].as_slice());
        assert!(name.as_str().is_err());
    }
}
