// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::dir::get_dir_entry_inode_by_name;
use crate::inode::Inode;
use crate::{DirEntryName, Ext4, Ext4Error, Path, PathBuf};
use alloc::vec::Vec;

/// How symlinks are treated when looking up an inode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FollowSymlinks {
    /// Symlinks are followed, except for the final component. If the
    /// final component is a symlink, the inode for that symlink is
    /// returned rather than the symlink's target.
    ///
    /// This is used for `Ext4::symlink_metadata`, which has similar
    /// behavior to `lstat`:
    /// https://www.man7.org/linux/man-pages/man2/lstat.2.html
    ExcludeFinalComponent,

    /// All symlinks are followed.
    All,
}

/// Resolve a path to get both the inode it points to and a
/// canonicalized path representation:
///   * Path separators deduplicated ("a//" becomes "a/").
///   * Trailing separators removed ("a/" becomes "a").
///   * "." components removed.
///   * ".." components resolved.
///   * symlink components resolved (except possibly for the last
///     component, depending on `follow`).
///
/// The behavior here should match the way Linux resolves paths. See
/// https://www.man7.org/linux/man-pages/man7/path_resolution.7.html for
/// more details.
///
/// # Errors
///
/// An error will be returned if:
/// * `path` is not absolute.
/// * Over 40 symlinks are encountered.
/// * Path length ever exceeds 4096 bytes.
///
/// # Panics
///
/// This function panics if path resolution takes over 1000
/// iterations. This should never occur in practice due to other
/// restrictions, this is just a hedge against unforeseen bugs.
pub(crate) fn resolve_path(
    fs: &Ext4,
    path: Path<'_>,
    follow: FollowSymlinks,
) -> Result<(Inode, PathBuf), Ext4Error> {
    // Maximum number of symlinks to resolve (for the whole path, not
    // individual components).
    const MAX_SYMLINKS: usize = 40;
    // Maximum path length in bytes. In general this library does not
    // enforce a path length limit, but during path resolution the
    // length can grow quite a bit due to symlinks.
    const MAX_PATH_LEN: usize = 4096;
    // Maximum number of iterations. This limit should never be reached
    // in practice, this is just to guard against unknown bugs that
    // could cause an infinite loop.
    const MAX_ITERATIONS: usize = 1000;

    let mut inode = fs.read_root_inode()?;

    if !path.is_absolute() {
        return Err(Ext4Error::NotAbsolute);
    }

    if path.as_ref().len() > MAX_PATH_LEN {
        return Err(Ext4Error::PathTooLong);
    }

    let mut path = path.as_ref().to_vec();
    // Remove duplicate separators to make the rest of the logic simpler.
    path_dedup_sep(&mut path);

    let mut num_symlinks = 0;
    let mut num_iterations = 0;

    let mut index = 1;
    while index < path.len() {
        // Guard against infinite loops. Max iterations should never be
        // reachable in practice due to the other restrictions
        // (MAX_SYMLINKS and MAX_PATH_LEN), so panic rather than
        // returning an error.
        num_iterations += 1;
        assert!(num_iterations <= MAX_ITERATIONS);

        // Find the end of the component. This is either the next '/',
        // or the end of the path.
        let next_sep = find_next_sep(&path, index);
        let comp_end = next_sep.unwrap_or(path.len());
        let is_last_component = next_sep.is_none();

        let comp = &path[index..comp_end];

        if !inode.file_type.is_dir() {
            // Can't look up a child of a non-directory;
            // path is invalid. This handles a case like
            // "/a/b", where "a" is a regular file instead
            // of a directory.
            return Err(Ext4Error::NotFound);
        }

        // Lookup the entry in the directory.
        let child_inode = get_dir_entry_inode_by_name(
            fs,
            &inode,
            DirEntryName::try_from(comp).unwrap(),
        )?;

        if comp == b"." {
            // Remove this component and continue from the same index.
            let remove_end = if comp_end == path.len() {
                comp_end
            } else {
                // Remove the separator at the end of this component too.
                comp_end + 1
            };
            path.drain(index..remove_end);
        } else if comp == b".." {
            // `index - 1` is OK because there must always be a
            // separator before this component.
            let remove_start = if index == 1 {
                // This is the first component after the root, and the
                // parent of root is still the root. So just remove this
                // component, but not the previous component.
                index
            } else {
                // Remove this component and the previous component.

                // Minus 2: -1 is the separator before this component,
                // so we want to start searching back from one earlier.
                let prev_sep = find_prev_sep(&path, index - 2).unwrap();
                prev_sep + 1
            };

            let remove_end = if comp_end == path.len() {
                comp_end
            } else {
                // Remove the separator at the end of this component too.
                comp_end + 1
            };

            path.drain(remove_start..remove_end);
            index = remove_start;
            inode = child_inode;
        } else if child_inode.file_type.is_symlink()
            && (follow == FollowSymlinks::All || !is_last_component)
        {
            num_symlinks += 1;
            if num_symlinks > MAX_SYMLINKS {
                return Err(Ext4Error::TooManySymlinks);
            }

            let target = child_inode.symlink_target(fs)?;

            let replace_start = if target.is_absolute() {
                // Reset back to the root component.
                inode = fs.read_root_inode()?;
                index = 1;

                // Symlink target is absolute, replace everything up to
                // and including the current component.
                0
            } else {
                // Symlink path is relative. Replace the current
                // component with the target and continue from the
                // current index. Do not update the inode.
                index
            };
            path.splice(
                replace_start..comp_end,
                target.as_ref().iter().cloned(),
            );

            if path.len() > MAX_PATH_LEN {
                return Err(Ext4Error::PathTooLong);
            }
            path_dedup_sep(&mut path);
        } else {
            // Normal file or directory. Continue on to the next
            // component.
            index = comp_end + 1;
            inode = child_inode;
        }
    }

    // TODO: construct pathbuf directly.
    let output_path = PathBuf::try_from(path).unwrap();

    Ok((inode, output_path))
}

/// Find the index of the next path separator, starting at `start`
/// (inclusive).
///
/// If found, the return value is the absolute index, not an offset from
/// `start`.
///
/// Panics if `start` is not a valid index.
fn find_next_sep(path: &[u8], start: usize) -> Option<usize> {
    assert!(start < path.len());
    (start..path.len()).find(|&i| path[i] == Path::SEPARATOR)
}

/// Find the index of the previous path separator, starting at `start`
/// (inclusive).
///
/// If found, the return value is the absolute index, not an offset from
/// `start`.
///
/// Panics if `start` is not a valid index.
fn find_prev_sep(path: &[u8], start: usize) -> Option<usize> {
    assert!(start < path.len());
    (0..=start).rev().find(|&i| path[i] == Path::SEPARATOR)
}

/// Replace any duplicate path separators with a single
/// separator. E.g. "a///b" becomes "a/b".
fn path_dedup_sep(path: &mut Vec<u8>) {
    // TODO: would be more efficient to go from end, and to delete in chunks.
    let mut i = 1;
    while i < path.len() {
        if path[i - 1] == Path::SEPARATOR && path[i] == Path::SEPARATOR {
            path.remove(i);
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_next_sep() {
        assert_eq!(find_next_sep(b"/", 0), Some(0));
        assert_eq!(find_next_sep(b"/abc/", 0), Some(0));
        assert_eq!(find_next_sep(b"/abc/", 1), Some(4));
        assert_eq!(find_next_sep(b"/abc", 1), None);
    }

    #[test]
    fn test_find_prev_sep() {
        assert_eq!(find_prev_sep(b"/", 0), Some(0));
        assert_eq!(find_prev_sep(b"/abc/", 0), Some(0));
        assert_eq!(find_prev_sep(b"/abc/", 1), Some(0));
        assert_eq!(find_prev_sep(b"/abc/", 3), Some(0));
        assert_eq!(find_prev_sep(b"/abc/", 4), Some(4));
        assert_eq!(find_prev_sep(b"abc/", 2), None);
    }

    #[test]
    fn test_path_dedup_sep() {
        let mut p = b"///a///abc///".to_vec();
        path_dedup_sep(&mut p);
        assert_eq!(p, b"/a/abc/");
    }

    // TODO
    #[cfg(feature = "std")]
    #[test]
    fn test_resolve() {
        let fs_path = std::path::Path::new("test_data/test_disk1.bin");
        let fs = &Ext4::load_from_path(fs_path).unwrap();

        let follow = FollowSymlinks::All;
        let mkp = |s| Path::new(s);

        // Test various things that should all resolve to the root.
        let resolve_to_root = [
            "/",
            "/.",
            "/./",
            "/..",
            "/../",
            "/../..",
            "/../../",
            "/empty_dir/..",
            "/empty_dir/../",
            "/empty_dir/../empty_dir/..",
            "/empty_dir/../empty_dir/../",
            "/dir1/dir2/sym_abs_dir/..",
            "/dir1/dir2/sym_abs_dir/../",
            "/dir1/dir2/sym_rel_dir/..",
            "/dir1/dir2/sym_rel_dir/../",
        ];
        for input in resolve_to_root {
            let (inode, path) = resolve_path(fs, mkp(input), follow).unwrap();
            assert_eq!((inode.index.get(), path.as_path()), (2, mkp("/")));
        }

        // Check directories.
        let (inode, path) =
            resolve_path(fs, mkp("/dir1/dir2"), follow).unwrap();
        assert_eq!(path, "/dir1/dir2");
        assert!(inode.file_type.is_dir());

        let small_file_path = PathBuf::new("/small_file");

        // Check absolute symlink.
        let (inode, path) =
            resolve_path(fs, mkp("/dir1/dir2/sym_abs"), follow).unwrap();
        assert_eq!(path, small_file_path);
        assert_eq!(fs.read_inode_file(&inode).unwrap(), b"hello, world!");
        let small_file_inode = inode.index;

        // Check relative symlink.
        let (inode, path) =
            resolve_path(fs, mkp("/dir1/dir2/sym_rel"), follow).unwrap();
        assert_eq!((inode.index, &path), (small_file_inode, &small_file_path));

        // Check absolute symlink followed by additional components.
        let (inode, path) = resolve_path(
            fs,
            mkp("/dir1/dir2/sym_abs_dir/../small_file"),
            follow,
        )
        .unwrap();
        assert_eq!((inode.index, &path), (small_file_inode, &small_file_path));

        // Check relative symlink followed by additional components.
        let (inode, path) = resolve_path(
            fs,
            mkp("/dir1/dir2/sym_rel_dir/../small_file"),
            follow,
        )
        .unwrap();
        assert_eq!((inode.index, &path), (small_file_inode, &small_file_path));

        // Check that the final symlink is not followed with
        // `ExcludeFinalComponent`.
        let (inode, path) = resolve_path(
            fs,
            mkp("/dir1/dir2/sym_abs"),
            FollowSymlinks::ExcludeFinalComponent,
        )
        .unwrap();
        assert_eq!(path, "/dir1/dir2/sym_abs");
        assert!(inode.file_type.is_symlink());
        let (inode, path) = resolve_path(
            fs,
            mkp("/dir1/dir2/sym_rel"),
            FollowSymlinks::ExcludeFinalComponent,
        )
        .unwrap();
        assert_eq!(path, "/dir1/dir2/sym_rel");
        assert!(inode.file_type.is_symlink());

        // Error: not absolute.
        assert!(matches!(
            resolve_path(fs, mkp("a"), follow),
            Err(Ext4Error::NotAbsolute)
        ));

        // Error: initial path is too long.
        let long_path = "/a".repeat(2049);
        assert!(matches!(
            resolve_path(fs, mkp(&long_path), follow),
            Err(Ext4Error::PathTooLong)
        ));

        // Error: intermediate path is too long. (Same error as above,
        // but difference is visible in code coverage.)
        let long_path = "a/".repeat(2030);
        assert!(matches!(
            dbg!(resolve_path(
                fs,
                mkp("/sym_long").join(long_path).as_path(),
                follow
            )),
            Err(Ext4Error::PathTooLong)
        ));

        // Error: symlink loop.
        assert!(matches!(
            resolve_path(fs, mkp("/sym_loop_a"), follow),
            Err(Ext4Error::TooManySymlinks)
        ));
    }
}
