// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use ext4_view::{Component, Path, PathBuf, PathError};

#[test]
fn test_path_construction() {
    let expected_path = b"abc";

    // Successful construction from a string.
    let src: &str = "abc";
    assert_eq!(Path::try_from(src).unwrap(), expected_path);
    assert_eq!(Path::new(src), expected_path);
    assert_eq!(PathBuf::try_from(src).unwrap(), expected_path);
    assert_eq!(PathBuf::new(src), expected_path);

    // Successful construction from a byte slice.
    let src: &[u8] = b"abc";
    assert_eq!(Path::try_from(src).unwrap(), expected_path);
    assert_eq!(Path::new(src), expected_path);
    assert_eq!(PathBuf::try_from(src).unwrap(), expected_path);
    assert_eq!(PathBuf::new(src), expected_path);

    // Successful construction from a byte array.
    let src: &[u8; 3] = b"abc";
    assert_eq!(Path::try_from(src).unwrap(), expected_path);
    assert_eq!(Path::new(src), expected_path);
    assert_eq!(PathBuf::try_from(src).unwrap(), expected_path);
    assert_eq!(PathBuf::new(src), expected_path);

    // Successful construction from a vector (only for PathBuf).
    let src: Vec<u8> = b"abc".to_vec();
    assert_eq!(PathBuf::try_from(src).unwrap(), expected_path);

    // Successful construction of a `Path` from a `&PathBuf`.
    let src: &PathBuf = &PathBuf::new("abc");
    assert_eq!(Path::try_from(src).unwrap(), expected_path);

    // Successful construction of empty PathBuf.
    assert_eq!(PathBuf::empty(), []);
    assert_eq!(PathBuf::default(), []);

    // Error: contains null.
    let src: &str = "\0";
    assert_eq!(Path::try_from(src), Err(PathError::ContainsNull));
    assert_eq!(PathBuf::try_from(src), Err(PathError::ContainsNull));

    // Error: invalid component (too long).
    let src = &[b'a'; 256];
    assert_eq!(Path::try_from(src), Err(PathError::ComponentTooLong));
    assert_eq!(PathBuf::try_from(src), Err(PathError::ComponentTooLong));
}

#[test]
fn test_path_debug() {
    let src = "abc😁\n".as_bytes();
    let expected = "abc😁\\n"; // Note the escaped slash.
    assert_eq!(format!("{:?}", Path::new(src)), expected);
    assert_eq!(format!("{:?}", PathBuf::new(src)), expected);
}

#[test]
fn test_path_display() {
    let path = Path::new([0xc3, 0x28].as_slice());
    assert_eq!(format!("{}", path.display()), "�(");

    let path = PathBuf::new([0xc3, 0x28].as_slice());
    assert_eq!(format!("{}", path.display()), "�(");
}

#[cfg(all(feature = "std", unix))]
#[test]
fn test_to_std() {
    let p = Path::new(b"abc");
    assert_eq!(<&std::path::Path>::from(p), std::path::Path::new("abc"));

    let p = PathBuf::new(b"abc");
    assert_eq!(std::path::PathBuf::from(p), std::path::PathBuf::from("abc"));
}

#[test]
fn test_is_absolute() {
    assert!(Path::new(b"/abc").is_absolute());
    assert!(PathBuf::new(b"/abc").is_absolute());
    assert!(!Path::new(b"abc").is_absolute());
    assert!(!PathBuf::new(b"abc").is_absolute());
    assert!(!Path::new(b"").is_absolute());
    assert!(!PathBuf::new(b"").is_absolute());
}

#[test]
fn test_as_ref() {
    let path = Path::new("abc");
    let b: &[u8] = path.as_ref();
    assert_eq!(b, b"abc");

    let path = PathBuf::new("abc");
    let b: &[u8] = path.as_ref();
    assert_eq!(b, b"abc");
}

#[test]
fn test_partial_eq() {
    let path = Path::new(b"abc".as_slice());
    let pathbuf = PathBuf::new(b"abc".as_slice());
    assert_eq!(path, path);
    assert_eq!(pathbuf, pathbuf);
    assert_eq!(path, pathbuf);
    assert_eq!(pathbuf, path);

    let v: &[u8] = b"abc";
    assert_eq!(path, v);
    assert_eq!(pathbuf, v);
}

#[test]
fn test_path_buf_from_path() {
    let path = Path::new("abc");
    let pathbuf = PathBuf::from(path);
    assert_eq!(pathbuf, "abc");
}

#[test]
fn test_path_buf_push() {
    let mut p = PathBuf::new("");
    p.push("a");
    assert_eq!(p, "a");

    let mut p = PathBuf::new("/");
    p.push("a");
    assert_eq!(p, "/a");

    let mut p = PathBuf::new("a");
    p.push("b");
    assert_eq!(p, "a/b");

    let mut p = PathBuf::new("a/");
    p.push("b");
    assert_eq!(p, "a/b");

    let mut p = PathBuf::new("a/");
    p.push("b/c");
    assert_eq!(p, "a/b/c");

    let mut p = PathBuf::new("a");
    p.push("/b");
    assert_eq!(p, "/b");
}

#[test]
#[should_panic]
fn test_path_buf_push_panic() {
    let mut p = PathBuf::new("");
    p.push("\0");
}

#[test]
fn test_path_join() {
    assert_eq!(Path::new("").join("b"), "b");
    assert_eq!(PathBuf::new("").join("b"), "b");

    assert_eq!(Path::new("/").join("a"), "/a");
    assert_eq!(PathBuf::new("/").join("a"), "/a");

    assert_eq!(Path::new("a").join("b"), "a/b");
    assert_eq!(PathBuf::new("a").join("b"), "a/b");

    assert_eq!(Path::new("a/").join("b"), "a/b");
    assert_eq!(PathBuf::new("a/").join("b"), "a/b");

    assert_eq!(Path::new("a/").join("b/c"), "a/b/c");
    assert_eq!(PathBuf::new("a/").join("b/c"), "a/b/c");

    assert_eq!(Path::new("a").join("/b"), "/b");
    assert_eq!(PathBuf::new("a").join("/b"), "/b");
}

#[test]
#[should_panic]
fn test_path_join_panic() {
    let p = Path::new("");
    let _ = p.join("\0");
}

#[test]
#[should_panic]
fn test_path_buf_join_panic() {
    let p = PathBuf::new("");
    let _ = p.join("\0");
}

#[test]
fn test_component() {
    assert_eq!(Component::normal("abc").unwrap(), "abc");
    assert!(Component::normal("a/b").is_err());

    assert_eq!(Component::RootDir, "/");
    assert_eq!(Component::CurDir, ".");
    assert_eq!(Component::ParentDir, "..");

    assert_eq!(format!("{:?}", Component::RootDir), "RootDir");
    assert_eq!(format!("{:?}", Component::CurDir), "CurDir");
    assert_eq!(format!("{:?}", Component::ParentDir), "ParentDir");
    assert_eq!(
        format!("{:?}", Component::normal("abc").unwrap()),
        "Normal(abc)"
    );
}

#[test]
fn test_path_components() {
    let p = Path::new("");
    let c: Vec<_> = p.components().collect();
    assert!(c.is_empty());

    let p = Path::new("/");
    let c: Vec<_> = p.components().collect();
    assert_eq!(c, [Component::RootDir]);

    let p = Path::new("/ab/cd/ef/../.");
    let c: Vec<_> = p.components().collect();
    assert_eq!(
        c,
        [
            Component::RootDir,
            Component::normal("ab").unwrap(),
            Component::normal("cd").unwrap(),
            Component::normal("ef").unwrap(),
            Component::ParentDir,
            Component::CurDir,
        ]
    );

    let p = Path::new("ab/cd/ef");
    let c: Vec<_> = p.components().collect();
    assert_eq!(
        c,
        [
            Component::normal("ab").unwrap(),
            Component::normal("cd").unwrap(),
            Component::normal("ef").unwrap(),
        ]
    );

    let p = Path::new("///ab///cd///ef//");
    let c: Vec<_> = p.components().collect();
    assert_eq!(
        c,
        [
            Component::RootDir,
            Component::normal("ab").unwrap(),
            Component::normal("cd").unwrap(),
            Component::normal("ef").unwrap(),
        ]
    );

    // PathBuf uses the same implementation, just do one test to
    // verify.
    let p = PathBuf::new("/a/b");
    let c: Vec<_> = p.components().collect();
    assert_eq!(
        c,
        [
            Component::RootDir,
            Component::normal("a").unwrap(),
            Component::normal("b").unwrap(),
        ]
    );
}
