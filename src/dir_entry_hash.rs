// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//
// The `md4_half` implementation is adapted from the md4 crate [1]. That
// code is licensed under Apache-2.0/MIT. MIT copyright/permission notice:
//
//   Copyright (c) 2016 bacher09, Artyom Pavlov
//   Copyright (c) 2016-2024 The RustCrypto Project Developers
//
//   Permission is hereby granted, free of charge, to any
//   person obtaining a copy of this software and associated
//   documentation files (the "Software"), to deal in the
//   Software without restriction, including without
//   limitation the rights to use, copy, modify, merge,
//   publish, distribute, sublicense, and/or sell copies of
//   the Software, and to permit persons to whom the Software
//   is furnished to do so, subject to the following
//   conditions:
//
//   The above copyright notice and this permission notice
//   shall be included in all copies or substantial portions
//   of the Software.
//
// [1]: https://github.com/RustCrypto/hashes/blob/89989057f560e54d319885f222ff011adf38165a/md4/src/lib.rs

use crate::dir_entry::DirEntryName;
use core::mem;
use core::num::Wrapping;

type Wu32 = Wrapping<u32>;
type StateBlock = [Wu32; 4];
type HashBlock = [Wu32; 8];

/// Hash the `data` block into the `state` block.
///
/// The hash algorithm is based on MD4, but cut down to fewer operations
/// for speed. (This was added to the Linux kernel decades ago; the
/// speed difference is negligible on modern machines, but disk formats
/// are forever.)
fn md4_half(state: &mut StateBlock, data: &HashBlock) {
    const K1: Wu32 = Wrapping(0x5a82_7999);
    const K2: Wu32 = Wrapping(0x6ed9_eba1);

    fn f(x: Wu32, y: Wu32, z: Wu32) -> Wu32 {
        z ^ (x & (y ^ z))
    }

    fn g(x: Wu32, y: Wu32, z: Wu32) -> Wu32 {
        (x & y) | (x & z) | (y & z)
    }

    fn h(x: Wu32, y: Wu32, z: Wu32) -> Wu32 {
        x ^ y ^ z
    }

    fn op<F>(f: F, a: Wu32, b: Wu32, c: Wu32, d: Wu32, k: Wu32, s: u32) -> Wu32
    where
        F: Fn(Wu32, Wu32, Wu32) -> Wu32,
    {
        let t = a + f(b, c, d) + k;
        Wrapping(t.0.rotate_left(s))
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];

    // round 1
    for i in [0, 4] {
        a = op(f, a, b, c, d, data[i], 3);
        d = op(f, d, a, b, c, data[i + 1], 7);
        c = op(f, c, d, a, b, data[i + 2], 11);
        b = op(f, b, c, d, a, data[i + 3], 19);
    }

    // round 2
    for &i in &[1, 0] {
        a = op(g, a, b, c, d, data[i] + K1, 3);
        d = op(g, d, a, b, c, data[i + 2] + K1, 5);
        c = op(g, c, d, a, b, data[i + 4] + K1, 9);
        b = op(g, b, c, d, a, data[i + 6] + K1, 13);
    }

    // round 3
    for &i in &[2, 0] {
        a = op(h, a, b, c, d, data[i + 1] + K2, 3);
        d = op(h, d, a, b, c, data[i + 5] + K2, 9);
        c = op(h, c, d, a, b, data[i] + K2, 11);
        b = op(h, b, c, d, a, data[i + 4] + K2, 15);
    }

    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
}

/// Create the 32-byte block of data that will be hashed.
///
/// If `src` is smaller than the block size (32 bytes), the remaining
/// bytes will be padded with the length of `src` (as a `u8`). The
/// ordering is a little weird though (possibly due to confusion about
/// endianness).
fn create_hash_block(src: &[u8]) -> HashBlock {
    let mut dst = HashBlock::default();

    // Get padding value.
    // OK to unwrap: the `src` length is always less than 256.
    let pad = u8::try_from(src.len()).unwrap();

    // Copy src to dst. Fill the rest with the pad byte.
    let mut src_index = 0;
    for elem in dst.iter_mut() {
        let bytes = if src_index < src.len() {
            let src = &src[src_index..];
            src_index += 4;

            if src.len() >= 4 {
                // At least 4 bytes remaining in `src`, copy directly.
                src[..4].try_into().unwrap()
            } else {
                // Less than 4 bytes remaining in `src`, left-pad with
                // the pad byte.
                let mut bytes = [pad; 4];
                let mut offset = 4 - src.len();
                for b in src {
                    bytes[offset] = *b;
                    offset += 1;
                }
                bytes
            }
        } else {
            // No more data to copy; fill the rest with the pad byte.
            [pad; 4]
        };

        *elem = Wrapping(u32::from_be_bytes(bytes));
    }

    dst
}

/// Hash `name` using the Linux kernel's bespoke "half MD4" scheme.
///
/// The `seed` value comes from the `s_hash_seed` field of the
/// superblock. If the `seed` is all zeroes, it's replaced with a
/// standard default seed.
pub(crate) fn dir_hash_md4_half(
    name: DirEntryName<'_>,
    mut seed: &[u32; 4],
) -> u32 {
    // Replace all-zero seed with a standard default seed.
    if seed == &[0; 4] {
        seed = &[0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    }

    // Initialize the `state` block with the seed, converting to
    // wrapping integers for the hash operations.
    let mut state = StateBlock::default();
    for i in 0..4 {
        state[i] = Wrapping(seed[i]);
    }

    // Hash the name in 32-byte chunks.
    for chunk in name.as_ref().chunks(mem::size_of::<HashBlock>()) {
        let inp = create_hash_block(chunk);
        md4_half(&mut state, &inp);
    }

    // Finalize the hash.
    state[1].0 & !1
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str;

    /// Check that `create_hash_block(src)` is equal to `expected`.
    #[track_caller]
    fn check_hash_block(src: &[u8], expected: [u32; 8]) {
        assert_eq!(
            create_hash_block(src)
                // Convert from `Wu32` to `u32`.
                .iter()
                .map(|n| n.0)
                .collect::<Vec<_>>(),
            expected
        );
    }

    /// Test creating a hash block from a message long enough to fill it.
    #[test]
    fn test_create_hash_block_full() {
        let src = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        assert_eq!(src.len(), 52);
        #[rustfmt::skip]
        let expected = [
            u32::from_be_bytes(*b"abcd"),
            u32::from_be_bytes(*b"efgh"),
            u32::from_be_bytes(*b"ijkl"),
            u32::from_be_bytes(*b"mnop"),
            u32::from_be_bytes(*b"qrst"),
            u32::from_be_bytes(*b"uvwx"),
            u32::from_be_bytes(*b"yzAB"),
            u32::from_be_bytes(*b"CDEF"),
        ];
        check_hash_block(src, expected);
    }

    /// Test creating a hash block from a message shorter than the
    /// block. Unused bytes are filled with the message length.
    #[test]
    fn test_create_hash_block_padding() {
        let src = b"abcdefghijklmnopqr";
        assert_eq!(src.len(), 0x12);
        #[rustfmt::skip]
        let expected = [
            u32::from_be_bytes(*b"abcd"),
            u32::from_be_bytes(*b"efgh"),
            u32::from_be_bytes(*b"ijkl"),
            u32::from_be_bytes(*b"mnop"),
            // Two bytes of padding (0x1212) then "qr" (0x7172).
            0x1212_7172,
            // Rest is padding.
            0x1212_1212,
            0x1212_1212,
            0x1212_1212,
        ];
        check_hash_block(src, expected);
    }

    /// Parse a UUID as a seed value.
    ///
    /// Internally this library doesn't actually use UUIDs, but it's
    /// nice to use UUIDs in the test for easy comparison with the
    /// `debugfs` tool, which uses UUID inputs.
    fn seed_from_uuid(uuid: &str) -> [u32; 4] {
        assert_eq!(uuid.len(), 36);
        let uuid = uuid.replace('-', "");

        let bytes: Vec<u32> = uuid
            .as_bytes()
            .chunks(8)
            .map(|chunk| {
                u32::from_str_radix(str::from_utf8(chunk).unwrap(), 16)
                    .unwrap()
                    .swap_bytes()
            })
            .collect();
        bytes.try_into().unwrap()
    }

    #[test]
    fn test_seed_from_uuid() {
        assert_eq!(
            seed_from_uuid("333fa1eb-588c-456e-b81c-d1d343cd0e01"),
            [0xeba13f33, 0x6e458c58, 0xd3d11cb8, 0x010ecd43]
        );
    }

    #[test]
    fn test_dir_hash_md4() {
        // To manually check the expected values, run:
        // debugfs -R 'dx_hash [-s <seed>] -h half_md4 <name>'
        //
        // (If `-s` isn't provided, it's the same as passing in an
        // all-zero hash, which will be replaced with the default seed
        // value seen in dir_hash_md4.)

        let seed0 = "00000000-0000-0000-0000-000000000000";
        let seed1 = "333fa1eb-588c-456e-b81c-d1d343cd0e01";
        let seed2 = "0fc48be0-17dc-4791-b120-39964e159a31";

        // Test a short name.
        let name = DirEntryName::try_from(b"abc").unwrap();
        assert_eq!(dir_hash_md4_half(name, &seed_from_uuid(seed1)), 0x25783134);
        assert_eq!(dir_hash_md4_half(name, &seed_from_uuid(seed2)), 0x4599f742);
        assert_eq!(dir_hash_md4_half(name, &seed_from_uuid(seed0)), 0xd196a868);

        // Test a max-length name.
        let name = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTU";
        assert_eq!(name.len(), 255);
        let name = DirEntryName::try_from(name).unwrap();
        assert_eq!(dir_hash_md4_half(name, &seed_from_uuid(seed1)), 0xe40e82e0);
    }
}
