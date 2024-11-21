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

    // OK to unwrap `i + N` below: both operands are small.

    // round 1
    for i in [0usize, 4] {
        a = op(f, a, b, c, d, data[i], 3);
        d = op(f, d, a, b, c, data[i.checked_add(1).unwrap()], 7);
        c = op(f, c, d, a, b, data[i.checked_add(2).unwrap()], 11);
        b = op(f, b, c, d, a, data[i.checked_add(3).unwrap()], 19);
    }

    // round 2
    for &i in &[1usize, 0] {
        a = op(g, a, b, c, d, data[i] + K1, 3);
        d = op(g, d, a, b, c, data[i.checked_add(2).unwrap()] + K1, 5);
        c = op(g, c, d, a, b, data[i.checked_add(4).unwrap()] + K1, 9);
        b = op(g, b, c, d, a, data[i.checked_add(6).unwrap()] + K1, 13);
    }

    // round 3
    for &i in &[2usize, 0] {
        a = op(h, a, b, c, d, data[i.checked_add(1).unwrap()] + K2, 3);
        d = op(h, d, a, b, c, data[i.checked_add(5).unwrap()] + K2, 9);
        c = op(h, c, d, a, b, data[i] + K2, 11);
        b = op(h, b, c, d, a, data[i.checked_add(4).unwrap()] + K2, 15);
    }

    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
}

// Using `as` is currently the best way to get sign extension.
#[allow(clippy::as_conversions)]
fn sign_extend_byte_to_u32(byte: u8) -> u32 {
    let sbyte = byte as i8;
    sbyte as u32
}

/// Create the 32-byte block of data that will be hashed.
fn create_hash_block(mut src: &[u8]) -> HashBlock {
    let mut dst = HashBlock::default();

    // Get padding value. If `src` is smaller than the block size (32
    // bytes), the remaining bytes will be padded with the length of
    // `src` (as a `u8`).
    let pad = u32::from_le_bytes([src.len().to_le_bytes()[0]; 4]);

    for dst in dst.iter_mut() {
        let mut elem = pad;

        // Process up to four bytes of `src`.
        for _ in 0..4 {
            if let Some(src_byte) = src.first() {
                // Sign extend the byte into a `u32`.
                let src_u32 = sign_extend_byte_to_u32(*src_byte);
                elem = src_u32.wrapping_add(elem << 8);

                src = &src[1..];
            }
        }
        *dst = Wrapping(elem);
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

        // Test a name with non-ASCII characters.
        let name = DirEntryName::try_from(
            "NetLock_Arany_=Class_Gold=_Főtanúsítvány.pem",
        )
        .unwrap();
        assert_eq!(dir_hash_md4_half(name, &seed_from_uuid(seed1)), 0xb40a2038);

        // Test a max-length name.
        let name = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTU";
        assert_eq!(name.len(), 255);
        let name = DirEntryName::try_from(name).unwrap();
        assert_eq!(dir_hash_md4_half(name, &seed_from_uuid(seed1)), 0xe40e82e0);
    }

    /// Generate random names and compare the hash generated by this
    /// module with the correct value produced by `debugfs`.
    ///
    /// This test is ignored by default because it requires `debugfs` to
    /// be installed and because it's slow.
    #[cfg(all(feature = "std", unix))]
    #[test]
    #[ignore]
    fn test_random_names() {
        use std::ffi::OsStr;
        use std::fs::File;
        use std::io::Read;
        use std::os::unix::ffi::OsStrExt;
        use std::process::Command;

        const TOTAL_ITERATIONS: usize = 5000;

        /// Get `len` random bytes.
        fn read_random_bytes(len: usize) -> Vec<u8> {
            let mut f = File::open("/dev/urandom").unwrap();
            let mut bytes = vec![0; len];
            f.read_exact(&mut bytes).unwrap();
            bytes
        }

        /// Generate a random UUID.
        fn gen_random_seed() -> String {
            let mut s: String = read_random_bytes(16)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            for i in [8, 13, 18, 23] {
                s.insert(i, '-');
            }
            s
        }

        /// Generate the data to hash.
        ///
        /// If the generated data contains invalid characters, returns
        /// `None`.
        fn gen_data_to_hash() -> Vec<u8> {
            // Generate a random length. Must be between 1 and 255, as
            // required by `DirEntryName`.
            let len = usize::from(read_random_bytes(1)[0]).max(1);

            let mut to_hash = read_random_bytes(len);

            // Replace a few characters with a different arbitrary
            // character. Null and path separator are not allowed in
            // `DirEntryName`. Double quotes and dashes confuse the
            // debugfs command.
            let bad_chars = [0, b'/', b'"', b'-'];
            for b in &mut to_hash {
                if bad_chars.contains(b) {
                    // Replace with an arbitrary safe character.
                    *b = b'a';
                }
            }

            to_hash
        }

        /// Construct the debugfs command to get calculate a hash. The
        /// command looks like this:
        /// debugfs -R 'dx_hash -s seed -h half_md4 <to_hash>'
        fn debugfs_cmd(to_hash: &[u8], seed: &str) -> Command {
            let mut req = b"dx_hash -s ".to_vec();
            req.extend(seed.as_bytes());
            req.extend(" -h half_md4 \"".as_bytes());
            req.extend(to_hash);
            req.push(b'"');

            let mut cmd = Command::new("debugfs");
            cmd.arg("-R").arg(OsStr::from_bytes(&req));
            cmd
        }

        /// Use `debugfs` to get the correct hash value.
        fn get_expected_hash(to_hash: &[u8], seed: &str) -> u32 {
            let output = debugfs_cmd(to_hash, seed).output().unwrap();
            assert!(output.status.success());

            // Parse the hash from the output. The output looks like this:
            // Hash of <name> is 0x13c16a1c (minor 0x96c543ac)
            let stdout = &output.stdout;
            let mut prefix = b"Hash of ".to_vec();
            prefix.extend(to_hash);
            prefix.extend(b" is 0x");
            let rest = &stdout[prefix.len()..];
            let space_index = rest.iter().position(|b| *b == b' ').unwrap();
            let hash = &rest[..space_index];

            let hash = core::str::from_utf8(hash).unwrap();
            u32::from_str_radix(hash, 16).unwrap()
        }

        for _ in 0..TOTAL_ITERATIONS {
            let to_hash = gen_data_to_hash();
            let seed = gen_random_seed();

            let expected_hash = get_expected_hash(&to_hash, &seed);

            let actual_hash = dir_hash_md4_half(
                DirEntryName::try_from(to_hash.as_slice()).unwrap(),
                &seed_from_uuid(&seed),
            );

            if actual_hash != expected_hash {
                // The data is random, so print everything out on
                // failure so it can be reproduced.
                println!("actual_hash={actual_hash:#08x}");
                println!("expected_hash={expected_hash:#08x}");
                println!("seed={seed}");
                println!("to_hash={to_hash:02x?}");
                panic!("actual_hash != expected_hash");
            }
        }
    }
}
