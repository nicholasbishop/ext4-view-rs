// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::format::format_bytes_debug;
use crate::inode::InodeIndex;
use crate::path::Path;
use core::fmt::{self, Debug, Formatter};
use core::str::Utf8Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirEntryNameError {
    Empty,
    TooLong,
    ContainsNull,
    ContainsSeparator,
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DirEntryName<'a>(pub(crate) &'a [u8]);

impl<'a> DirEntryName<'a> {
    /// Maximum length of a `DirEntryName`.
    pub const MAX_LEN: usize = 255;

    /// Convert to a `&str` if the name is valid UTF-8.
    #[inline]
    pub fn as_str(&self) -> Result<&'a str, Utf8Error> {
        core::str::from_utf8(self.0)
    }
}

impl<'a> Debug for DirEntryName<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        format_bytes_debug(self.0, f)
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
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DirEntry {
    /// Number of the inode that this entry points to.
    inode: InodeIndex,

    /// Raw name of the entry.
    name: DirEntryNameBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
