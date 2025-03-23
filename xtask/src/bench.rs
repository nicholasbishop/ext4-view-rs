// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod walk;

use anyhow::Result;
use ext4_view::Ext4;
use std::path::Path;
use std::time::SystemTime;

/// Run a simple wall-time performance benchmark.
pub fn run_bench(path: &Path, iters: u32) -> Result<()> {
    bench_impl(iters, || {
        // Load the filesystem and recursively walk all directories and
        // files. Each file is fully read and hashed.
        let ext4 = Ext4::load_from_path(path).unwrap();
        let digest = walk::walk(&ext4).unwrap();
        println!("filesystem hash: {digest}");
    });

    Ok(())
}

/// Run `f` a total of `iters` times. Measure the duration of each
/// iteration, and report statistics.
fn bench_impl<F>(iters: u32, f: F)
where
    F: Fn(),
{
    let mut durations = Vec::new();
    for i in 1..=iters {
        println!("iter {i}:");

        let start = SystemTime::now();
        f();
        let duration = SystemTime::now().duration_since(start).unwrap();

        println!("{duration:.2?}");
        durations.push(duration);
    }
    durations.sort();

    let min = durations[0];
    let max = durations.last().unwrap();
    let median = durations[durations.len() / 2];
    println!("range: {min:.2?} - {max:.2?}");
    println!("median: {median:.2?}");
}
