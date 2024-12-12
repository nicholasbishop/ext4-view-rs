// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::ext4::load_test_disk1;
use ext4_view::Ext4Error;

#[test]
fn test_file_metadata() {
    let fs = load_test_disk1();

    let file = fs.open("/small_file").unwrap();
    let metadata = file.metadata();

    assert!(metadata.file_type().is_regular_file());
    assert!(!metadata.is_dir());
    assert!(!metadata.is_symlink());
    assert_eq!(metadata.mode(), 0o644);
    assert_eq!(metadata.len(), 13);
}

#[test]
fn test_file_open_errors() {
    let fs = load_test_disk1();

    assert!(matches!(
        fs.open("not_absolute").unwrap_err(),
        Ext4Error::NotAbsolute
    ));
    assert!(matches!(
        fs.open("/empty_dir").unwrap_err(),
        Ext4Error::IsADirectory
    ));
}

#[test]
fn test_file_debug() {
    let fs = load_test_disk1();
    let file = fs.open("/small_file").unwrap();

    let s = format!("{:?}", file);
    assert!(s.starts_with("File { inode: "));
    assert!(s.ends_with(".. }"));
}
