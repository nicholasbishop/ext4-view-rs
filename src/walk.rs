// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::file_type::FileType;
use crate::inode::{Inode, InodeIndex};
use crate::path::PathBuf;
use crate::{Ext4, Ext4Error, ReadDir};
use alloc::vec;
use alloc::vec::Vec;

struct WalkIterToVisit {
    path: PathBuf,
    inode: InodeIndex,
}

pub struct WalkIterEntry {
    pub path: PathBuf,
    pub(crate) inode: Inode,
}

impl WalkIterEntry {
    pub fn file_type(&self) -> FileType {
        self.inode.file_type
    }

    pub fn read(&self, ext4: &Ext4) -> Result<Vec<u8>, Ext4Error> {
        ext4.read_inode_file(&self.inode)
    }
}

pub struct WalkIter<'a> {
    ext4: &'a Ext4,
    to_visit: Vec<WalkIterToVisit>,
}

impl<'a> WalkIter<'a> {
    pub(crate) fn new(ext4: &'a Ext4) -> Self {
        let root_inode = InodeIndex::new(2).unwrap();

        let entry = WalkIterToVisit {
            inode: root_inode,
            // OK to unwrap: this is a valid path.
            path: PathBuf::try_from("/").unwrap(),
        };

        Self {
            ext4,
            to_visit: vec![entry],
        }
    }
}

impl<'a> Iterator for WalkIter<'a> {
    // TODO: wrap in Result
    type Item = WalkIterEntry;

    fn next(&mut self) -> Option<WalkIterEntry> {
        let entry = self.to_visit.pop()?;

        // TODO: fix unwraps
        let inode = self.ext4.read_inode(entry.inode).unwrap();
        if inode.file_type.is_dir() {
            let mut dir = ReadDir::new(self.ext4, &inode, entry.path.clone())
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            dir.retain(|entry| {
                let name = entry.file_name();
                name != b"." && name != b".."
            });
            self.to_visit.extend(
                dir.iter()
                    .filter(|e| {
                        let name = e.file_name();
                        name != b"." && name != b".."
                    })
                    .map(|e| {
                        let mut path = entry.path.clone();
                        path.push(e.file_name());
                        WalkIterToVisit {
                            path,
                            inode: e.inode(),
                        }
                    }),
            );
        }

        Some(WalkIterEntry {
            path: entry.path,
            inode,
        })
    }
}
