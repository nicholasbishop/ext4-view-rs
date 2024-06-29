// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use ext4_view::{Ext4, Ext4Error, PathBuf};

fn load_test_disk1() -> Ext4 {
    const DATA: &[u8] = include_bytes!("../../test_data/test_disk1.bin");
    Ext4::load(Box::new(DATA.to_vec())).unwrap()
}

#[test]
fn test_canonicalize() {
    let fs = load_test_disk1();

    assert_eq!(fs.canonicalize("/empty_file").unwrap(), "/empty_file");

    assert_eq!(fs.canonicalize("/").unwrap(), "/");
    assert_eq!(fs.canonicalize("/..").unwrap(), "/");
    assert_eq!(fs.canonicalize("/dir1").unwrap(), "/dir1");
    assert_eq!(fs.canonicalize("/dir1/").unwrap(), "/dir1");
    assert_eq!(fs.canonicalize("/dir1/.").unwrap(), "/dir1");
    assert_eq!(fs.canonicalize("/dir1/./").unwrap(), "/dir1");
    assert_eq!(fs.canonicalize("/dir1/../dir1").unwrap(), "/dir1");
    assert_eq!(fs.canonicalize("/dir1/../dir1/").unwrap(), "/dir1");
    assert_eq!(
        fs.canonicalize("/dir1/dir2/sym_abs").unwrap(),
        "/small_file"
    );
    assert_eq!(
        fs.canonicalize("/dir1/dir2/sym_rel").unwrap(),
        "/small_file"
    );
    assert_eq!(fs.canonicalize("/dir1/dir2/sym_abs_dir").unwrap(), "/dir1");
    assert_eq!(fs.canonicalize("/dir1/dir2/sym_abs_dir/").unwrap(), "/dir1");
    assert_eq!(fs.canonicalize("/dir1/dir2/sym_rel_dir").unwrap(), "/dir1");
    assert_eq!(fs.canonicalize("/dir1/dir2/sym_rel_dir/").unwrap(), "/dir1");

    // Error: does not exist.
    assert!(matches!(
        fs.canonicalize("/does_not_exist").unwrap_err(),
        Ext4Error::NotFound
    ));

    // Error: child of a non-directory.
    assert!(matches!(
        fs.canonicalize("/small_file/invalid").unwrap_err(),
        Ext4Error::NotADirectory
    ));

    // Error: malformed path.
    assert!(matches!(
        fs.canonicalize("\0").unwrap_err(),
        Ext4Error::MalformedPath
    ));

    // Error: path is not absolute.
    assert!(matches!(
        fs.canonicalize("not_absolute").unwrap_err(),
        Ext4Error::NotAbsolute
    ));
}

#[test]
fn test_read() {
    let fs = load_test_disk1();

    // Empty file.
    assert_eq!(fs.read("/empty_file").unwrap(), []);

    // Small file.
    assert_eq!(fs.read("/small_file").unwrap(), b"hello, world!");

    // File with holes.
    let mut expected = vec![];
    for i in 0..5 {
        expected.extend(vec![0xa5; 4096]);
        if i != 4 {
            expected.extend(vec![0; 8192]);
        }
    }
    assert_eq!(fs.read("/holes").unwrap(), expected);

    // Errors.
    assert!(fs.read("not_absolute").is_err());
    assert!(fs.read("/does_not_exist").is_err());
}

#[test]
fn test_read_to_string() {
    let fs = load_test_disk1();

    // Empty file.
    assert_eq!(fs.read_to_string("/empty_file").unwrap(), "");

    // Small file.
    assert_eq!(fs.read_to_string("/small_file").unwrap(), "hello, world!");

    // Errors:
    assert!(matches!(
        fs.read_to_string("/holes").unwrap_err(),
        Ext4Error::NotUtf8
    ));
    assert!(matches!(
        fs.read_to_string("/empty_dir").unwrap_err(),
        Ext4Error::IsADirectory
    ));
    assert!(matches!(
        fs.read_to_string("not_absolute").unwrap_err(),
        Ext4Error::NotAbsolute
    ));
    assert!(matches!(
        fs.read_to_string("/does_not_exist").unwrap_err(),
        Ext4Error::NotFound
    ));
    assert!(matches!(
        fs.read_to_string("\0").unwrap_err(),
        Ext4Error::MalformedPath
    ));
}

#[test]
fn test_read_link() {
    let fs = load_test_disk1();

    // Basic success test.
    assert_eq!(fs.read_link("/sym_simple").unwrap(), "small_file");

    // Symlinks prior to the final component are expanded as normal.
    assert_eq!(
        fs.read_link("/dir1/dir2/sym_abs_dir/../sym_simple")
            .unwrap(),
        "small_file"
    );

    // Short symlink target is inline, longer symlink is stored in extents.
    assert_eq!(fs.read_link("/sym_59").unwrap(), "a".repeat(59));
    assert_eq!(fs.read_link("/sym_60").unwrap(), "a".repeat(60));

    // Error: path is not absolute.
    assert!(matches!(
        fs.read_link("not_absolute").unwrap_err(),
        Ext4Error::NotAbsolute
    ));

    // Error: malformed path.
    assert!(matches!(
        fs.read_link("\0").unwrap_err(),
        Ext4Error::MalformedPath
    ));

    // Error: does not exist.
    assert!(matches!(
        fs.read_link("/does_not_exist").unwrap_err(),
        Ext4Error::NotFound
    ));

    // Error: not a symlink.
    assert!(matches!(
        fs.read_link("/small_file").unwrap_err(),
        Ext4Error::NotASymlink
    ));
}

#[test]
fn test_read_dir() {
    let fs = load_test_disk1();

    // Get contents of directory `/big_dir`.
    let dir = fs
        .read_dir("/big_dir")
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Get the sorted list of entry names.
    let mut entry_names: Vec<String> = dir
        .iter()
        .map(|e| e.file_name().as_str().unwrap().to_owned())
        .collect();
    entry_names.sort_unstable();

    // Get the sorted list of entry paths.
    let mut entry_paths: Vec<PathBuf> = dir.iter().map(|e| e.path()).collect();
    entry_paths.sort_unstable();

    // Get expected entry names, 0-9999.
    let mut expected_names = vec![".".to_owned(), "..".to_owned()];
    expected_names.extend((0u32..10_000u32).map(|n| n.to_string()));
    expected_names.sort_unstable();

    // Get expected entry paths.
    let expected_paths = expected_names
        .iter()
        .map(|n| PathBuf::try_from(format!("/big_dir/{n}").as_bytes()).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(entry_names, expected_names);
    assert_eq!(entry_paths, expected_paths);

    // Errors:
    assert!(matches!(
        fs.read_dir("not_absolute").unwrap_err(),
        Ext4Error::NotAbsolute
    ));
    assert!(matches!(
        fs.read_dir("/empty_file").unwrap_err(),
        Ext4Error::NotADirectory
    ));
    assert!(matches!(
        fs.read_dir("\0").unwrap_err(),
        Ext4Error::MalformedPath
    ));
}

#[test]
fn test_exists() {
    let fs = load_test_disk1();

    // Success: exists.
    assert!(fs.exists("/empty_file").unwrap());

    // Success: does not exist.
    assert!(!fs.exists("/does_not_exist").unwrap());

    // Error: malformed path.
    assert!(matches!(
        fs.exists("\0").unwrap_err(),
        Ext4Error::MalformedPath
    ));

    // Error: path is not absolute.
    assert!(matches!(
        fs.exists("not_absolute").unwrap_err(),
        Ext4Error::NotAbsolute
    ));
}

#[test]
fn test_metadata() {
    let fs = load_test_disk1();

    let metadata = fs.metadata("/small_file").unwrap();
    assert!(metadata.file_type().is_regular_file());
    assert!(!metadata.is_dir());
    assert!(!metadata.is_symlink());
    assert_eq!(metadata.mode(), 0o644);
    assert_eq!(
        metadata.len(),
        u64::try_from("hello, world!".len()).unwrap()
    );

    // Error: malformed path.
    assert!(matches!(
        fs.metadata("\0").unwrap_err(),
        Ext4Error::MalformedPath
    ));

    // Error: path is not absolute.
    assert!(matches!(
        fs.metadata("not_absolute").unwrap_err(),
        Ext4Error::NotAbsolute
    ));
}

#[test]
fn test_symlink_metadata() {
    let fs = load_test_disk1();

    // Final component is a symlink.
    let metadata = fs.symlink_metadata("/sym_simple").unwrap();
    assert!(metadata.is_symlink());
    assert_eq!(metadata.mode(), 0o777);

    // Symlinks prior to the final component are followed as normal.
    assert_eq!(
        fs.symlink_metadata("/dir1/dir2/sym_abs_dir/../sym_simple")
            .unwrap(),
        metadata
    );

    // Final component not a symlink behaves same as `metadata`.
    assert_eq!(
        fs.symlink_metadata("/small_file").unwrap(),
        fs.metadata("/small_file").unwrap()
    );

    // Error: malformed path.
    assert!(matches!(
        fs.symlink_metadata("\0").unwrap_err(),
        Ext4Error::MalformedPath
    ));

    // Error: path is not absolute.
    assert!(matches!(
        fs.symlink_metadata("not_absolute").unwrap_err(),
        Ext4Error::NotAbsolute
    ));
}
