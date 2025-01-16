// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! To run this example program:
//!
//!     cargo run -F std --example cat <filesystem> <path>

use anyhow::{Context, Result};
use ext4_view::Ext4;
use std::io::{self, ErrorKind, Read, Write};
use std::{env, process};

fn print_usage() {
    println!("Usage: cargo run -F std --example cat <filesystem> <path>");
    println!();
    println!("Read <path> from <filesystem> and print it to standard output.");
    println!();
    println!("Arguments:");
    println!("  <filesystem>: Path of a file containing an ext4 filesystem");
    println!("  <path>:       Absolute path of a file within the filesystem");
}

fn parse_args() -> Result<(std::path::PathBuf, ext4_view::PathBuf)> {
    let args: Vec<_> = env::args_os().collect();

    if args.len() != 3 || args.iter().any(|arg| arg == "-h" || arg == "--help")
    {
        print_usage();
        process::exit(1);
    }

    let filesystem = std::path::PathBuf::from(&args[1]);
    let path = ext4_view::PathBuf::try_from(args[2].clone())
        .context("Invalid ext4 path")?;

    Ok((filesystem, path))
}

fn main() -> Result<()> {
    let (path_to_filesystem, path_within_filesystem) = parse_args()?;

    // Load the filesystem.
    let fs = Ext4::load_from_path(&path_to_filesystem).with_context(|| {
        format!("Failed to load {}", path_to_filesystem.display())
    })?;

    // Open a file within the filesystem for reading.
    let mut file = fs.open(&path_within_filesystem).with_context(|| {
        format!("Failed to open {}", path_within_filesystem.display())
    })?;

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let mut chunk = vec![0; 4096];
    loop {
        // Read a chunk of data from the file.
        //
        // Note: in a no_std program, `File::read_bytes` can be used
        // instead of `std::io::Read`.
        let bytes_read = file.read(&mut chunk).with_context(|| {
            format!("Failed to read {}", path_within_filesystem.display())
        })?;

        if bytes_read == 0 {
            // End of file reached.
            return Ok(());
        }

        // Write the chunk to stdout.
        if let Err(err) = stdout.write_all(&chunk[..bytes_read]) {
            if err.kind() == ErrorKind::BrokenPipe {
                return Ok(());
            } else {
                return Err(err).context("Failed to write to stdout");
            }
        }
    }
}
