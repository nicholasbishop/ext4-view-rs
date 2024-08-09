// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use clap::builder::{OsStringValueParser, TypedValueParser};
use clap::{Parser, ValueEnum};
use ext4_view::Ext4;
use std::io::{self, Write};
use tabled::builder::Builder;
use tabled::settings::object::Column;
use tabled::settings::{Alignment, Style};

type Error = Box<dyn std::error::Error>;

/// Perform a read-only operation on an ext4 filesystem.
#[derive(Parser)]
struct Opt {
    action: Action,

    /// Path of a file containing an ext4 filesystem.
    fs: std::path::PathBuf,

    /// Path within the ext4 filesystem to operate on.
    #[arg(value_parser = OsStringValueParser::new().try_map(ext4_view::PathBuf::try_from))]
    path: ext4_view::PathBuf,
}

#[derive(Clone, Copy, ValueEnum)]
enum Action {
    Cat,
    Ls,
}

fn ls_to_string(fs: &Ext4, path: ext4_view::Path<'_>) -> Result<String, Error> {
    let path = fs.canonicalize(path)?;
    let metadata = fs.symlink_metadata(&path)?;

    let mut builder = Builder::new();
    builder.push_record(["Path:", "Size:", "Type:", "Mode:"]);

    fn print_entry(
        builder: &mut Builder,
        fs: &Ext4,
        path: ext4_view::Path<'_>,
    ) -> Result<(), Error> {
        let metadata = fs.symlink_metadata(path)?;

        let row = vec![
            path.display().to_string(),
            metadata.len().to_string(),
            if metadata.is_symlink() {
                "symlink".to_string()
            } else if metadata.is_dir() {
                "dir".to_string()
            } else {
                "file".to_string()
            },
            format!("{:04o}", metadata.mode()),
        ];

        builder.push_record(row);

        Ok(())
    }

    if metadata.is_dir() {
        for entry in fs.read_dir(&path)? {
            let entry = entry?;
            print_entry(&mut builder, fs, entry.path().as_path())?;
        }
    } else {
        print_entry(&mut builder, fs, path.as_path())?;
    }

    let table = builder
        .build()
        .modify(Column::from(1), Alignment::right())
        .with(Style::empty())
        .to_string();

    Ok(table)
}

fn run(opt: &Opt) -> Result<(), Error> {
    let fs = Ext4::load_from_path(&opt.fs)?;
    let path = opt.path.as_path();

    match opt.action {
        Action::Ls => {
            let table = ls_to_string(&fs, path)?;
            println!("{table}");
        }
        Action::Cat => {
            let content = fs.read(path)?;
            io::stdout().write_all(&content)?;
        }
    }

    Ok(())
}

fn main() -> Result<(), Error> {
    let opt = Opt::parse();
    run(&opt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ls() {
        fn line_to_words(line: &str) -> Vec<&str> {
            line.split_whitespace().collect()
        }

        /// Search `text` for a line matching `search_for`. Whitespace
        /// within the line is ignored.
        fn line_is_present(text: &str, search_for: &str) -> bool {
            let search_for = line_to_words(search_for);
            text.lines().any(|line| {
                let line = line_to_words(line);
                line == search_for
            })
        }

        let fs = Ext4::load_from_path("../test_data/test_disk1.bin").unwrap();

        // Testing the full output might be fragile as the test data
        // changes, so just check a few specific lines.

        // Test directory.
        let actual = ls_to_string(&fs, ext4_view::Path::new("/")).unwrap();
        assert!(line_is_present(&actual, "/small_file 13 file 0644"));
        assert!(line_is_present(&actual, "/empty_dir 1024 dir 0755"));
        assert!(line_is_present(&actual, "/sym_simple 10 symlink 0777"));

        // Test single file.
        let actual =
            ls_to_string(&fs, ext4_view::Path::new("/small_file")).unwrap();
        assert!(line_is_present(&actual, "/small_file 13 file 0644"));
    }
}
