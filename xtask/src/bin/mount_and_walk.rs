// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::Path;
use std::{env, fs};
use xtask::diff_walk::{FileContent, WalkDirEntry};
use xtask::{calc_file_sha256, Mount};

fn new_dir_entry(path: &Path) -> Result<WalkDirEntry> {
    // Test for symlink first, because `is_dir` follows symlinks.
    let content = if path.is_symlink() {
        let target = fs::read_link(path)?;
        FileContent::Symlink(target)
    } else if path.is_dir() {
        FileContent::Dir
    } else {
        FileContent::Regular(calc_file_sha256(path)?)
    };
    Ok(WalkDirEntry {
        path: path.to_owned(),
        content,
    })
}

fn walk_mounted(path: &Path) -> Result<Vec<WalkDirEntry>> {
    assert!(path.is_dir());

    let mut output = Vec::new();

    output.push(new_dir_entry(path)?);

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        if entry.file_type()?.is_dir() {
            output.extend(walk_mounted(&path)?);
        } else {
            output.push(new_dir_entry(&path)?);
        }
    }

    Ok(output)
}

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .context("missing required path argument")?;

    let mount = Mount::new(Path::new(&path))?;
    let mut paths = walk_mounted(mount.path())?;
    paths.sort_unstable();

    for mut dir_entry in paths {
        // Remove the mount point from the beginning of the path. Append
        // that to `/` to make the path absolute again. This makes the
        // output convenient to compare against the library, which
        // produces absolute paths when iterating over directories.
        let path = dir_entry.path.strip_prefix(mount.path())?;
        dir_entry.path = Path::new("/").join(path);

        io::stdout().write_all(&dir_entry.format())?;
        println!();
    }
    Ok(())
}
