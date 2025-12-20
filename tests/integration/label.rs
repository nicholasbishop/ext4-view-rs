// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::test_util::load_test_disk1;
use ext4_view::Label;

#[test]
fn test_label_empty() {
    let label = Label::new([0; 16]);
    assert_eq!(label.to_str().unwrap(), "");
    assert_eq!(*label.as_bytes(), [0; 16]);
    assert_eq!(format!("{label:?}"), r#""""#);
    assert_eq!(format!("{}", label.display()), "");
}

#[test]
fn test_label_utf8_with_null() {
    let bytes = b"test label\0\0\0\0\0\0";
    let label = Label::new(*bytes);
    assert_eq!(label.to_str().unwrap(), "test label");
    assert_eq!(label.as_bytes(), bytes);
    assert_eq!(format!("{label:?}"), r#""test label""#);
    assert_eq!(format!("{}", label.display()), "test label");
}

#[test]
fn test_label_utf8_with_interior_null() {
    let bytes = b"abc\0def\0ghi\0jkl\0";
    let label = Label::new(*bytes);
    assert_eq!(label.to_str().unwrap(), "abc");
    assert_eq!(label.as_bytes(), bytes);
    assert_eq!(format!("{label:?}"), r#""abc""#);
    assert_eq!(format!("{}", label.display()), "abc");
}

#[test]
fn test_label_utf8_max_len() {
    let bytes = b"0123456789abcdef";
    let label = Label::new(*bytes);
    assert_eq!(label.to_str().unwrap(), "0123456789abcdef");
    assert_eq!(label.as_bytes(), bytes);
    assert_eq!(format!("{label:?}"), r#""0123456789abcdef""#);
    assert_eq!(format!("{}", label.display()), "0123456789abcdef");
}

#[test]
fn test_label_not_utf8() {
    let bytes = [0xc0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let label = Label::new(bytes);
    assert!(label.to_str().is_err());
    assert_eq!(*label.as_bytes(), bytes);
    assert_eq!(format!("{label:?}"), r#""\xc0""#);
    assert_eq!(format!("{}", label.display()), "�");
}

#[test]
fn test_get_label() {
    let fs = load_test_disk1();
    assert_eq!(fs.label().to_str().unwrap(), "ext4-view testfs");
}
