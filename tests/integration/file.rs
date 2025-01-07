// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::expected_holes_data;
use crate::ext2::load_ext2;
use crate::test_util::load_test_disk1;
use ext4_view::Ext4Error;

#[cfg(feature = "std")]
use std::io::{ErrorKind, Read, Seek, SeekFrom};

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

/// Test reading an empty file.
#[test]
fn test_file_read_empty_file() {
    let fs = load_test_disk1();
    let mut file = fs.open("/empty_file").unwrap();
    let mut buf = [0];
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 0);
}

/// Test reading into an empty buffer.
#[test]
fn test_file_read_empty_buffer() {
    let fs = load_test_disk1();
    let mut file = fs.open("/small_file").unwrap();

    let mut buf = [];
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 0);
}

/// Test reading a small file, reading the whole file at once with a
/// buffer the same size as the file.
#[test]
fn test_file_read_exact() {
    let fs = load_test_disk1();
    let mut file = fs.open("/small_file").unwrap();

    // Read the whole file at once.
    let mut buf = [0; 13];
    assert_eq!(file.read_bytes(&mut buf).unwrap(), buf.len());
    assert_eq!(buf, "hello, world!".as_bytes());

    // Check that reading again does not return any bytes.
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 0);
}

/// Test reading a small file, reading the whole file at once into a
/// buffer larger than the file.
#[test]
fn test_file_read_big_buf() {
    let fs = load_test_disk1();
    let mut file = fs.open("/small_file").unwrap();

    // Read the whole file at once, buffer is larger than the file.
    let mut buf = vec![b'X'; 20];
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 13);
    assert_eq!(buf, "hello, world!XXXXXXX".as_bytes());

    // Check that reading again does not return any bytes.
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 0);
}

/// Test reading a small file, reading one byte at a time.
#[test]
fn test_file_read_by_byte() {
    let fs = load_test_disk1();
    let mut file = fs.open("/small_file").unwrap();

    // Read the file one byte at a time.
    let mut buf = [0];
    let mut all = Vec::new();
    for _ in 0..13 {
        assert_eq!(file.read_bytes(&mut buf).unwrap(), 1);
        all.extend(buf);
    }
    assert_eq!(all, "hello, world!".as_bytes());

    // Check that reading again does not return any bytes.
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 0);
}

/// Test reading a small file, reading two bytes at a time.
#[test]
fn test_file_read_by_twos() {
    let fs = load_test_disk1();
    let mut file = fs.open("/small_file").unwrap();

    // Read the first 12 bytes of the file, two bytes a time.
    let mut buf = [0; 2];
    let mut all = Vec::new();
    for _ in 0..6 {
        assert_eq!(file.read_bytes(&mut buf).unwrap(), 2);
        all.extend(buf);
    }

    // Request two more bytes; should only be one remaining.
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 1);
    all.push(buf[0]);

    assert_eq!(all, "hello, world!".as_bytes());

    // Check that reading again does not return any bytes.
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 0);
}

/// Test reading a file with holes.
#[test]
fn test_file_read_holes() {
    let fs = load_test_disk1();
    let mut file = fs.open("/holes").unwrap();

    let mut all = vec![];
    for _ in 0..10 {
        let mut buf = vec![0xff; 1024];
        assert_eq!(file.read_bytes(&mut buf).unwrap(), buf.len());
        all.extend(buf);
    }

    assert_eq!(all, expected_holes_data());

    // Check that reading again does not return any bytes.
    assert_eq!(file.read_bytes(&mut all).unwrap(), 0);
}

/// Test that each read is limited to at most one block.
#[test]
fn test_file_read_limited_to_block() {
    let fs = load_ext2();
    // Load a file that is larger than one block.
    let mut file = fs.open("/big_file").unwrap();

    let mut buf = vec![0xff; 2048];
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 1024);
    assert_eq!(&buf[..1024], vec![0; 1024]);
    assert_eq!(&buf[1024..], vec![0xff; 1024]);
}

/// Test seeking in a small file.
#[test]
fn test_file_seek_first_block() {
    let fs = load_test_disk1();
    let mut file = fs.open("/small_file").unwrap();
    let mut buf = [0; 5];

    file.seek_to(7).unwrap();
    assert_eq!(file.read_bytes(&mut buf).unwrap(), buf.len());
    assert_eq!(buf, "world".as_bytes());

    file.seek_to(0).unwrap();
    assert_eq!(file.read_bytes(&mut buf).unwrap(), buf.len());
    assert_eq!(buf, "hello".as_bytes());
}

/// Test seeking in a file with multiple blocks.
#[test]
fn test_file_seek_multiple_blocks() {
    let fs = load_ext2();
    // Load a file that is larger than one block.
    let mut file = fs.open("/big_file").unwrap();

    let mut buf = [0; 4];

    // Seek to first byte of the second block.
    file.seek_to(1024).unwrap();
    assert_eq!(file.read_bytes(&mut buf).unwrap(), buf.len());
    assert_eq!(u32::from_le_bytes(buf), 1);

    // Seek to first byte of the third block.
    file.seek_to(2048).unwrap();
    assert_eq!(file.read_bytes(&mut buf).unwrap(), buf.len());
    assert_eq!(u32::from_le_bytes(buf), 2);

    // Seek to the last four bytes of the second block.
    file.seek_to(2044).unwrap();
    assert_eq!(file.read_bytes(&mut buf).unwrap(), buf.len());
    assert_eq!(u32::from_le_bytes(buf), 1);
}

/// Test that seeking past the end is allowed (matching the behavior of
/// `std::io::Seek` and POSIX seek in general).
#[test]
fn test_file_seek_past_end() {
    let fs = load_test_disk1();
    let mut file = fs.open("/small_file").unwrap();

    // Seek way past the end of the file.
    file.seek_to(1000).unwrap();
    assert_eq!(file.position(), 1000);

    // We're past the end of the file, so reading returns zero bytes.
    let mut buf = [0];
    assert_eq!(file.read_bytes(&mut buf).unwrap(), 0);
}

/// Basic test of `std::io::Read` impl.
#[cfg(feature = "std")]
#[test]
fn test_file_std_read() {
    let fs = load_test_disk1();
    let mut file = fs.open("/small_file").unwrap();

    let mut buf = [0; 13];
    assert_eq!(file.read(&mut buf).unwrap(), buf.len());
    assert_eq!(buf, "hello, world!".as_bytes());
}

/// Test `std::io::Seek` impl.
#[cfg(feature = "std")]
#[test]
fn test_file_std_seek() {
    let fs = load_test_disk1();
    let mut file = fs.open("/small_file").unwrap();

    let mut buf = [0; 13];

    // Seek from start.
    assert_eq!(file.seek(SeekFrom::Start(1)).unwrap(), 1);
    assert_eq!(file.read(&mut buf).unwrap(), 12);
    assert_eq!(buf, "ello, world!\0".as_bytes());

    // Seek from end.
    assert_eq!(file.seek(SeekFrom::End(-6)).unwrap(), 7);
    assert_eq!(file.read(&mut buf).unwrap(), 6);
    assert_eq!(&buf[..6], "world!".as_bytes());

    // Seek from current position.
    assert_eq!(file.seek(SeekFrom::Current(-6)).unwrap(), 7);
    assert_eq!(file.read(&mut buf).unwrap(), 6);
    assert_eq!(&buf[..6], "world!".as_bytes());

    // Invalid seek from end.
    assert_eq!(
        file.seek(SeekFrom::End(-100)).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );

    // Invalid seek from current position.
    assert_eq!(
        file.seek(SeekFrom::Current(-100)).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
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
    assert!(s.ends_with(", position: 0, .. }"));
}
