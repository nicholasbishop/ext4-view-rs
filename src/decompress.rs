// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// Note that in addition to being used as a normal module for tests in
// ext4-view, this module is directly included in `xtask`.

/// Magic bytes at the start of a compressed file.
pub(crate) const COMPRESSED_MAGIC: [u8; 4] = *b"nb88";

/// Chunk size for `compress_chunks` and `decompress_chunks`. Chunk size
/// found experimentally.
pub(crate) const CHUNK_SIZE: usize = 32;

/// Apply RLE decompression, then chunk decompression.
pub(crate) fn decompress(mut data: &[u8]) -> Vec<u8> {
    if data[..4] != COMPRESSED_MAGIC {
        panic!("invalid magic for compressed file");
    }
    data = &data[4..];

    decompress_chunks(&decompress_rle(data))
}

/// Simple run-length-encoding decompression. See
/// `xtask/src/compress.rs` for details.
fn decompress_rle(mut data: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    while !data.is_empty() {
        let val = data[0];
        data = &data[1..];

        if val == 0 {
            // Get the VLQ-encoded run length.
            let zero_len = usize_from_vlq(&mut data);
            output.resize(output.len() + zero_len, 0u8);
        } else {
            output.push(val);
        }
    }
    output
}

/// Simple chunk-based decompression. See `xtask/src/compress.rs` for
/// details.
fn decompress_chunks(mut data: &[u8]) -> Vec<u8> {
    let num_chunks = usize_from_vlq(&mut data);

    let mut chunks = Vec::new();
    for _ in 0..num_chunks {
        chunks.push(&data[..CHUNK_SIZE]);
        data = &data[CHUNK_SIZE..];
    }

    let mut output = Vec::new();
    while !data.is_empty() {
        let chunk_index = usize_from_vlq(&mut data);
        output.extend(chunks[chunk_index]);
    }

    output
}

/// Decode a `usize` from a variable-length quantity encoding.
/// See <https://en.wikipedia.org/wiki/Variable-length_quantity>.
///
/// The `bytes` parameter is a a mutable reference to a slice; it is
/// advanced to the end of the VLQ.
fn usize_from_vlq(bytes: &mut &[u8]) -> usize {
    let mut val = 0usize;
    while !bytes.is_empty() {
        let byte = bytes[0];
        *bytes = &bytes[1..];

        val = (val << 7) | usize::from(byte & 0b0111_1111);

        if (byte & 0b1000_0000) == 0 {
            break;
        }
    }
    val
}
