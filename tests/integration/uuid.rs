// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use ext4_view::Uuid;

/// Arbitrary test UUID as raw bytes.
const TEST_UUID_BYTES: [u8; 16] = [
    0x6d, 0x74, 0x86, 0xe2, 0xf1, 0x53, 0x48, 0x57, 0x9f, 0x16, 0xc8, 0x95,
    0x65, 0x70, 0xba, 0xe2,
];

const TEST_UUID: Uuid = Uuid::new(TEST_UUID_BYTES);

#[test]
fn test_uuid_as_bytes() {
    assert_eq!(*TEST_UUID.as_bytes(), TEST_UUID_BYTES);
}

/// Test Debug and Display impls of UUID.
#[test]
fn test_uuid_format() {
    let expected = "6d7486e2-f153-4857-9f16-c8956570bae2";
    assert_eq!(format!("{TEST_UUID:?}"), expected);
    assert_eq!(format!("{TEST_UUID}"), expected);
}
