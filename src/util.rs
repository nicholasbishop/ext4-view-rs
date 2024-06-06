// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use core::mem::size_of;

/// Convert a `u32` to a `usize`.
///
/// Rust allows `usize` to be as small as `u16`, but on platforms
/// supported by this crate, this conversion is infallible.
///
/// # Panics
///
/// Panics if `val` does not fit in this platform's `usize`.
#[inline]
#[must_use]
pub(crate) fn usize_from_u32(val: u32) -> usize {
    usize::try_from(val).unwrap()
}

/// Create a `u64` from two `u32` values.
#[inline]
#[must_use]
pub(crate) fn u64_from_hilo(hi: u32, lo: u32) -> u64 {
    (u64::from(hi) << 32) | u64::from(lo)
}

/// Create a `u32` from two `u16` values.
#[inline]
#[must_use]
pub(crate) fn u32_from_hilo(hi: u16, lo: u16) -> u32 {
    (u32::from(hi) << 16) | u32::from(lo)
}

/// Read the first two bytes from `bytes` as a little-endian [`u16`].
///
/// # Panics
///
/// Panics if `bytes` is less than two bytes in length.
#[inline]
#[must_use]
pub(crate) fn read_u16le(bytes: &[u8], offset: usize) -> u16 {
    let bytes = bytes.get(offset..offset + size_of::<u16>()).unwrap();
    u16::from_le_bytes(bytes.try_into().unwrap())
}

/// Read the first four bytes from `bytes` as a little-endian [`u32`].
///
/// # Panics
///
/// Panics if `bytes` is less than four bytes in length.
#[inline]
#[must_use]
pub(crate) fn read_u32le(bytes: &[u8], offset: usize) -> u32 {
    let bytes = bytes.get(offset..offset + size_of::<u32>()).unwrap();
    u32::from_le_bytes(bytes.try_into().unwrap())
}
