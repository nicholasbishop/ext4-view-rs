// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

/// How symlinks are treated when looking up an inode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FollowSymlinks {
    /// All symlinks are followed.
    All,

    /// Symlinks are followed, except for the final component. If the
    /// final component is a symlink, the inode for that symlink is
    /// returned rather than the symlink's target.
    ///
    /// This is used for `Ext4::symlink_metadata`, which has similar
    /// behavior to `lstat`:
    /// https://www.man7.org/linux/man-pages/man2/lstat.2.html
    ExcludeFinalComponent,
}
