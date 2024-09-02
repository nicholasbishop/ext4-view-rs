// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! This module implements a simple compression scheme used to shrink
//! the size of generated test data. First chunk compression is applied,
//! then RLE compression. See `compress_chunks` and `compress_rle` for
//! details.
//!
//! Shrinking the test data is helpful because its stored via Git LFS,
//! and Github charges somewhat aggressively for LFS bandwidth. CI jobs
//! that download LFS blobs are counted against the bandwidth limit,
//! even though it's presumably very cheap for the vendor. Rather than
//! give in to this rather greedy scheme, add a little bit of code
//! complexity and decrease the amount of data being stored.
//!
//! The reason for implementing our own scheme, rather than some
//! standard compression such as lz4, is to minimize dependencies in
//! ext4-view. Even though the decompression code is only needed in
//! tests, and therefore only needs to be a dev-dependency, users
//! sometimes have to do extra work to vet or import dependencies, and
//! these requirements don't always exempt dev-dependencies.
//!
//! This scheme shrinks the current disk data to about 2.4% of the
//! original size. For comparison, lz4 shinks to about 1.8%. Of course,
//! the custom scheme here is highly dependent on the type of data being
//! compressed, and it might get better or worse with future changes to
//! the test data.

use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

include!("../../src/decompress.rs");

/// Read file data from `path`, apply chunk and RLE compression, then
/// write the compressed data out to a new file. The new file's path is
/// the same as the input, but with a ".compressed" extension.
pub fn compress_file(path: &Path) -> Result<()> {
    let input = fs::read(path)?;

    let mut compressed = COMPRESSED_MAGIC.to_vec();
    compressed.extend(compress_rle(&compress_chunks(&input)));

    // Ensure that decompressing the compressed data produces identical
    // bytes to the input.
    assert_eq!(decompress(&compressed), input);

    let input_len = input.len() as f64;
    let compressed_len = compressed.len() as f64;
    let bytes_in_mib = 1024.0 * 1024.0;
    println!(
        "compressed {:.02} MiB to {:.02} MiB ({:.02}% of original size)",
        input_len / bytes_in_mib,
        compressed_len / bytes_in_mib,
        (compressed_len / input_len) * 100.0,
    );

    let mut output_path = path.as_os_str().to_os_string();
    output_path.push(".compressed");

    fs::write(output_path, compressed)?;
    Ok(())
}

/// Encode a `usize` as a variable-length quantity.
/// See <https://en.wikipedia.org/wiki/Variable-length_quantity>.
fn usize_to_vlq(mut val: usize) -> Vec<u8> {
    if val == 0 {
        return vec![0];
    }
    let mut output = Vec::new();
    while val > 0 {
        let mut byte = u8::try_from(val & 0b0111_1111).unwrap();
        val >>= 7;
        if !output.is_empty() {
            byte |= 0b1000_0000;
        }
        output.insert(0, byte);
    }

    output
}

/// Compress the input with a chunk-based scheme.
///
/// The input is divided into chunks of `CHUNK_SIZE`. Each unique chunk
/// is assigned an index; the chunks are sorted so that the most common
/// chunk has an index of zero, and less common chunks have higher
/// indices. The output contains:
/// 1. Number of chunks (VLQ).
/// 2. List of chunks. This is just the raw chunk data in the order
///    described above.
/// 3. List of chunk indices. Each index is a VLQ.
///
/// To decompress, read each chunk index and output the corresponding
/// chunk data to the output stream.
fn compress_chunks(input: &[u8]) -> Vec<u8> {
    // Ensure that the input size is an even multiple of the chunk size.
    assert_eq!(input.len() % CHUNK_SIZE, 0);

    let mut output: Vec<u8> = Vec::new();

    // Get a map from the chunk to the number of times that chunk
    // appears in the input.
    let mut chunk_to_count = HashMap::new();
    for chunk in input.chunks(CHUNK_SIZE) {
        chunk_to_count
            .entry(chunk)
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }

    // Convert the map to a vec, then sort by the chunk count from high
    // to low.
    let mut chunk_to_count: Vec<(&[u8], usize)> =
        chunk_to_count.into_iter().collect();
    chunk_to_count.sort_unstable_by_key(|(_, count)| *count);
    chunk_to_count.reverse();

    // Write the number of chunks to the output as a VLQ.
    output.extend(usize_to_vlq(chunk_to_count.len()));

    // Create a map from chunk to chunk index. At the same time, write
    // each chunk to the output.
    let mut chunk_to_index = HashMap::new();
    for (index, (chunk, _)) in chunk_to_count.into_iter().enumerate() {
        chunk_to_index.insert(chunk, index);
        output.extend(chunk);
    }

    // For each chunk, write the chunk index to the output as a VLQ.
    for chunk in input.chunks(CHUNK_SIZE) {
        output.extend(usize_to_vlq(chunk_to_index[chunk]));
    }

    output
}

/// Compress `input` with an [RLE] (run-length encoding) scheme.
///
/// Only zeros are treated as runs; all other byte values are copied
/// unchanged. Runs of zeros are encoded as a zero followed by the
/// length of the run encoded as a VLQ.
///
/// [RLE]: https://en.wikipedia.org/wiki/Run-length_encoding
fn compress_rle(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();

    let mut i = 0;
    while i < input.len() {
        if input[i] == 0 {
            // Find how long this run of zeros is.
            let zero_len =
                input[i..].iter().position(|elem| *elem != 0).unwrap_or(
                    // The rest of the file is zero.
                    input.len() - i,
                );
            // Write out a zero followed by the VLQ-encoded run length.
            output.push(0u8);
            output.extend(usize_to_vlq(zero_len));
            i += zero_len;
        } else {
            // Write out non-zero values unchanged.
            output.push(input[i]);
            i += 1;
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vlq() {
        // Test values from:
        // https://en.wikipedia.org/wiki/Variable-length_quantity.
        let data: [(usize, &[u8]); 10] = [
            (0, &[0]),
            (127, &[0x7f]),
            (128, &[0x81, 0x00]),
            (8_192, &[0xc0, 0x00]),
            (16_383, &[0xff, 0x7f]),
            (16_384, &[0x81, 0x80, 0x00]),
            (2_097_151, &[0xff, 0xff, 0x7f]),
            (2_097_152, &[0x81, 0x80, 0x80, 0x00]),
            (134_217_728, &[0xc0, 0x80, 0x80, 0x00]),
            (268_435_455, &[0xff, 0xff, 0xff, 0x7f]),
        ];

        for (input, mut encoded) in data {
            assert_eq!(usize_to_vlq(input), encoded);

            assert_eq!(usize_from_vlq(&mut encoded), input);
            // `usize_from_vlq` advances past the VLQ bytes.
            assert!(encoded.is_empty());
        }
    }

    #[test]
    fn test_compress_rle() {
        assert_eq!(compress_rle(&[1, 2, 3]), [1, 2, 3]);
        assert_eq!(compress_rle(&[0]), [0, 1]);
        assert_eq!(compress_rle(&[0, 0, 0]), [0, 3]);
        assert_eq!(compress_rle(&vec![0; 16_384]), [0, 0x81, 0x80, 0x00]);
        assert_eq!(
            compress_rle(&[1, 2, 3, 0, 0, 0, 4, 5, 6]),
            [1, 2, 3, 0, 3, 4, 5, 6]
        );
    }

    #[test]
    fn test_compress_chunks() {
        let chunk_a = [0xab; 32];
        let chunk_b = [0xcd; 32];
        let chunk_c = [0xef; 32];

        let mut input = Vec::new();
        input.extend(chunk_a);
        input.extend(chunk_b);
        input.extend(chunk_b);
        input.extend(chunk_c);
        input.extend(chunk_b);
        input.extend(chunk_b);
        input.extend(chunk_c);

        // Expected output starts with the number of chunks, then the
        // chunks sorted by frequency, then chunk indices.
        let mut expected_output = vec![3];
        expected_output.extend(chunk_b);
        expected_output.extend(chunk_c);
        expected_output.extend(chunk_a);
        expected_output.push(2);
        expected_output.push(0);
        expected_output.push(0);
        expected_output.push(1);
        expected_output.push(0);
        expected_output.push(0);
        expected_output.push(1);

        let output = compress_chunks(&input);
        assert_eq!(output, expected_output);
    }
}
