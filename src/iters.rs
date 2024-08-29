// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

/// This macro implements the `Iterator` trait for type `$target`. The
/// iterator's `Item` type is `Result<$item, Ext4Error>`.
///
/// The `target` type must provide two things:
/// 1. A boolean field named `is_done`. If this field is set to true,
///    iteration will end.
/// 2. A method named `next_impl`, which is where most of the actual
///    iteration is implemented.
///
/// The `next_impl` method returns `Result<Option<$item>, Ext4Error`. If
/// `next_impl` returns `Ok(Some(_))`, that value is yielded. If it
/// returns `Ok(None)`, `next_impl` will be called again. If it returns
/// `Err(_)`, the error will be yielded and `is_done` will be set to
/// true.
///
/// This macro makes iterators easier to write in two ways:
/// 1. Since `next_impl` returns a `Result`, normal error propagation
///    with `?` can be used. Without this macro, each error case would
///    have to set `is_done` before yielding the error.
/// 2. Automatically trying again when `next_impl` returns `Ok(None)`
///    makes it much easier to implement iterators that are logically
///    nested.
macro_rules! impl_result_iter {
    ($target:ident, $item:ident) => {
        impl Iterator for $target {
            type Item = Result<$item, Ext4Error>;

            fn next(&mut self) -> Option<Result<$item, Ext4Error>> {
                loop {
                    if self.is_done {
                        return None;
                    }

                    match self.next_impl() {
                        Ok(Some(entry)) => return Some(Ok(entry)),
                        Ok(None) => {
                            // Continue.
                        }
                        Err(err) => {
                            self.is_done = true;
                            return Some(Err(err));
                        }
                    }
                }
            }
        }
    };
}

pub(crate) mod extents;
pub(crate) mod file_blocks;
pub(crate) mod read_dir;
