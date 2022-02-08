//! Simple Rust library for merging directories using symlinks
//!
//! Currently supports only Unix-like operating systems

use std::fs::{DirEntry, read_dir, remove_dir_all, remove_file};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};

/// Represents the type of action, that takes place when an existing path is to be replaced by a symlink.
///
/// - Put `.keep` file into the target directory that you do not want to overwrite, and nor its' nested files or directories
/// - Put `.keep_files` file into the target directory that you do not want to overwrite, and nor its' nested files
/// - Put `.keep_dirs` file into the target directory that you do not want to overwrite, and nor its' nested directories
pub enum Overwrite {
    /// Automatically overwrite existing paths to introduce a symlink
    All,
    /// Automatically overwrite existing target directories with symlinks
    Dirs,
    /// Automatically overwrite existing target files (or symlinks) with symlinks
    Files,
    /// Don't overwrite any existing paths with symlinks
    None
}

/// Generate symlinks pointing to the `source` directory content in the `target` directory.
///
/// Simply said, everything from the `source` directory will be symlinked to the `target` directory.
///
/// For overwriting options, see [Overwrite] enum.
pub fn generate_symlinks(source: &Path, target: &Path, overwrite: Overwrite) -> Result<()> {

    let source = source.canonicalize().with_context(|| "Couldn't resolve source path")?;
    let target = target.canonicalize().with_context(|| "Couldn't resolve target path")?;

    // Both source and target have to be directories for this to work
    if !source.is_dir() || !target.is_dir() {
        bail!("Make sure both source and target paths are directories");
    }

    let mut stack = Vec::new();
    go_deeper(&mut stack, &resolve_symlink(&source).with_context(|| "Couldn't resolve source path")?)
        .with_context(|| format!("Directory listing ({source:?}) failed"))?;

    loop {

        let source_entry = match stack.pop() {
            Some(source_path) => source_path,
            None => break
        }.with_context(|| "Reading source directory entry has failed")?;

        let source_path = source_entry.path();
        let mut target_path = target.to_path_buf();
        target_path.push(
            &source_path.strip_prefix(&source)
                .with_context(|| format!("Couldn't strip base path ({source:?}) from source path ({source_path:?})"))?
        );

        // Overwrite existing target path
        if target_path.exists() {

            match overwrite {
                Overwrite::All => {
                    match target_path.is_file() {
                        true => {

                            // Check for .keep or .keep_files file existence
                            if keep_path(&target_path, &[".keep", ".keep_files"]) {
                                continue;
                            }

                            remove_path(&target_path).with_context(|| format!("Error while deleting file ({target_path:?}) before overwriting it with ({source_path:?})"))?;

                        },
                        false => {

                            // Check for .keep or .keep_dirs file existence
                            if keep_path(&target_path, &[".keep", ".keep_dirs"]) {
                                go_deeper(&mut stack, &source_path).with_context(|| format!("Directory listing ({source_path:?}) failed"))?;
                                continue;
                            }

                            remove_path(&target_path).with_context(|| format!("Error while deleting directory ({target_path:?}) before overwriting it with ({source_path:?})"))?;

                        }
                    };
                },
                Overwrite::Dirs => {

                    if !target_path.is_dir() {
                        continue;
                    }

                    // Check for .keep or .keep_dirs file existence
                    if keep_path(&target_path, &[".keep", ".keep_dirs"]) {
                        go_deeper(&mut stack, &source_path).with_context(|| format!("Directory listing ({source_path:?}) failed"))?;
                        continue;
                    }

                    remove_path(&target_path).with_context(|| format!("Error while deleting directory ({target_path:?}) before overwriting it with ({source_path:?})"))?;

                },
                Overwrite::Files => {

                    if !target_path.is_file() {

                        if source_path.is_dir() {
                            go_deeper(&mut stack, &source_path).with_context(|| format!("Directory listing ({source_path:?}) failed"))?;
                        }

                        continue;

                    }

                    // Check for .keep or .keep_files file existence
                    if keep_path(&target_path, &[".keep", ".keep_files"]) {
                        continue;
                    }

                    remove_path(&target_path).with_context(|| format!("Error while deleting file ({target_path:?}) before overwriting it with ({source_path:?})"))?;

                },
                Overwrite::None => { // Don't overwrite anything, try to find differences and symlink individual files/folders

                    if source_path.is_dir() {
                        go_deeper(&mut stack, &source_path).with_context(|| format!("Directory listing ({source_path:?}) failed"))?;
                    }

                    continue;

                }
            };

        }

        symlink(&source_path, &target_path).with_context(|| format!("Failed to create symlink from ({source_path:?}) to ({target_path:?})"))?;

    }

    Ok(())

}

fn go_deeper(stack: &mut Vec<std::io::Result<DirEntry>>, path: &Path) -> Result<()> {
    let path = resolve_symlink(&path).with_context(|| format!("Couldn't resolve path ({path:?})"))?;
    let listing = read_dir(&path).with_context(|| format!("Directory listing ({path:?}) has failed"))?;
    stack.extend(listing);
    Ok(())
}

fn resolve_symlink(path: &Path) -> std::io::Result<PathBuf> {
    match path.is_symlink() {
        true => path.read_link(),
        false => Ok(path.to_path_buf())
    }
}

fn keep_path(path: &Path, keep: &[&str]) -> bool {

    if keep.iter().any(|&k| {
        let mut p = path.to_path_buf();
        p.push(k);
        p.exists()
    }) { return true; }

    path.ancestors().any(|ancestor| {
        keep.iter().any(|&k| {
            let mut p = ancestor.to_path_buf();
            p.set_file_name(k);
            p.exists()
        })
    })

}

fn remove_path(path: &Path) -> std::io::Result<()> {
    match path.is_file() {
        true => remove_file(path),
        false => remove_dir_all(path)
    }
}

#[cfg(test)]
mod tests {

    use std::fs::{create_dir, File, remove_dir_all};
    use std::path::Path;
    use crate::{generate_symlinks, Overwrite};

    #[test]
    fn accepts_only_directories() {

        prepare_test_directory();

        assert!(generate_symlinks(Path::new("test_files/test_dir1"), Path::new("test_files/test_file1.txt"), Overwrite::All).is_err());
        assert!(generate_symlinks(Path::new("test_files/test_file2.json"), Path::new("test_files/test_dir2"), Overwrite::All).is_err());
        assert!(generate_symlinks(Path::new("test_files/test_file2.json"), Path::new("test_files/test_file1.txt"), Overwrite::All).is_err());

    }

    #[test]
    fn merge_directories_without_overwrite() {

        prepare_test_directory();

        assert!(generate_symlinks(Path::new("test_files/test_dir1"), Path::new("test_files/test_dir2"), Overwrite::None).is_ok());
            assert!(Path::new("test_files/test_dir2/lorem.txt").is_symlink());
            assert!(!Path::new("test_files/test_dir2/ipsum.php").is_symlink());
            assert!(!Path::new("test_files/test_dir2/keep").is_symlink());
                assert!(!Path::new("test_files/test_dir2/keep/.keep").is_symlink());
                assert!(Path::new("test_files/test_dir2/keep/haha.yml").is_symlink());
                assert!(!Path::new("test_files/test_dir2/keep/do_not_overwrite.txt").is_symlink());
            assert!(!Path::new("test_files/test_dir2/nested").is_symlink());
                assert!(!Path::new("test_files/test_dir2/nested/dolor.cpp").is_symlink());
                assert!(Path::new("test_files/test_dir2/nested/lorem").is_symlink());

    }

    #[test]
    fn merge_directories_with_files_overwrite() {

        prepare_test_directory();

        assert!(generate_symlinks(Path::new("test_files/test_dir1"), Path::new("test_files/test_dir2"), Overwrite::Files).is_ok());
            assert!(Path::new("test_files/test_dir2/lorem.txt").is_symlink());
            assert!(Path::new("test_files/test_dir2/ipsum.php").is_symlink());
            assert!(!Path::new("test_files/test_dir2/keep").is_symlink());
                assert!(!Path::new("test_files/test_dir2/keep/.keep").is_symlink());
                assert!(Path::new("test_files/test_dir2/keep/haha.yml").is_symlink());
                assert!(!Path::new("test_files/test_dir2/keep/do_not_overwrite.txt").is_symlink());
            assert!(!Path::new("test_files/test_dir2/nested").is_symlink());
                assert!(Path::new("test_files/test_dir2/nested/dolor.cpp").is_symlink());
                assert!(Path::new("test_files/test_dir2/nested/lorem").is_symlink());

    }

    #[test]
    fn merge_directories_with_directories_overwrite() {

        prepare_test_directory();

        assert!(generate_symlinks(Path::new("test_files/test_dir1"), Path::new("test_files/test_dir2"), Overwrite::Dirs).is_ok());
            assert!(Path::new("test_files/test_dir2/lorem.txt").is_symlink());
            assert!(!Path::new("test_files/test_dir2/ipsum.php").is_symlink());
            assert!(!Path::new("test_files/test_dir2/keep").is_symlink());
                assert!(!Path::new("test_files/test_dir2/keep/.keep").is_symlink());
                assert!(Path::new("test_files/test_dir2/keep/haha.yml").is_symlink());
                assert!(!Path::new("test_files/test_dir2/keep/do_not_overwrite.txt").is_symlink());
            assert!(Path::new("test_files/test_dir2/nested").is_symlink());
                assert!(!Path::new("test_files/test_dir2/nested/dolor.cpp").is_symlink());
                assert!(!Path::new("test_files/test_dir2/nested/lorem").is_symlink());

    }

    #[test]
    fn merge_directories_with_all_overwrite() {

        prepare_test_directory();

        assert!(generate_symlinks(Path::new("test_files/test_dir1"), Path::new("test_files/test_dir2"), Overwrite::All).is_ok());
            assert!(Path::new("test_files/test_dir2/lorem.txt").is_symlink());
            assert!(Path::new("test_files/test_dir2/ipsum.php").is_symlink());
            assert!(!Path::new("test_files/test_dir2/keep").is_symlink());
                assert!(!Path::new("test_files/test_dir2/keep/.keep").is_symlink());
                assert!(Path::new("test_files/test_dir2/keep/haha.yml").is_symlink());
                assert!(!Path::new("test_files/test_dir2/keep/do_not_overwrite.txt").is_symlink());
            assert!(Path::new("test_files/test_dir2/nested").is_symlink());
                assert!(!Path::new("test_files/test_dir2/nested/dolor.cpp").is_symlink());
                assert!(!Path::new("test_files/test_dir2/nested/lorem").is_symlink());

    }

    fn cleanup_test_directory() {
        remove_dir_all(Path::new("test_files")).unwrap();
    }

    fn prepare_test_directory() {

        cleanup_test_directory();

        // Root
        create_dir(Path::new("test_files")).unwrap();
            File::create(Path::new("test_files/test_file1.txt")).unwrap();
            File::create(Path::new("test_files/test_file2.json")).unwrap();

        // 1
        create_dir(Path::new("test_files/test_dir1")).unwrap();
            File::create(Path::new("test_files/test_dir1/lorem.txt")).unwrap();
            File::create(Path::new("test_files/test_dir1/ipsum.php")).unwrap();
            create_dir(Path::new("test_files/test_dir1/keep")).unwrap();
                File::create(Path::new("test_files/test_dir1/keep/haha.yml")).unwrap();
                File::create(Path::new("test_files/test_dir1/keep/do_not_overwrite.txt")).unwrap();
            create_dir(Path::new("test_files/test_dir1/nested")).unwrap();
                create_dir(Path::new("test_files/test_dir1/nested/lorem")).unwrap();
                File::create(Path::new("test_files/test_dir1/nested/dolor.cpp")).unwrap();

        // 2
        create_dir(Path::new("test_files/test_dir2")).unwrap();
            File::create(Path::new("test_files/test_dir2/index.html")).unwrap();
            File::create(Path::new("test_files/test_dir2/ipsum.php")).unwrap();
            create_dir(Path::new("test_files/test_dir2/keep")).unwrap();
                File::create(Path::new("test_files/test_dir2/keep/.keep")).unwrap();
                File::create(Path::new("test_files/test_dir2/keep/do_not_overwrite.txt")).unwrap();
            create_dir(Path::new("test_files/test_dir2/nested")).unwrap();
                File::create(Path::new("test_files/test_dir2/nested/dolor.cpp")).unwrap();
                File::create(Path::new("test_files/test_dir2/nested/original.rs")).unwrap();

    }

}
