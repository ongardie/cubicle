use anyhow::Result;
use std::ffi::OsString;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn copyfile_untrusted(
    src_dir: &Path,
    src_path: &Path,
    dst_dir: &Path,
    dst_path: &Path,
) -> Result<()> {
    let src_dir = cap_std::fs::Dir::open_ambient_dir(src_dir, cap_std::ambient_authority())?;
    let dst_dir = cap_std::fs::Dir::open_ambient_dir(dst_dir, cap_std::ambient_authority())?;
    src_dir.copy(src_path, &dst_dir, dst_path)?;
    Ok(())
}

pub fn rmtree(path: &Path) -> Result<()> {
    // This is a bit challenging for a few reasons:
    //
    // 1. Symlinks leading out of the `path` directory must not cause this
    //    function to affect files outside the `path` directory.
    //
    // 2. `remove_dir_all` won't remove the contents of read-only directories,
    //    such as Go's packages. See
    //    <https://github.com/golang/go/issues/27161>.
    //
    // 3. Docker might leave empty directories owned by root. Specifically, it
    //    seems to often leave one where a volume was mounted, like a Cubicle
    //    container's work directory within its home directory. These are
    //    removable but their permissions can't be altered.

    let dir = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
    match dir.remove_open_dir_all() {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            // continue below
        }
        Err(e) => return Err(e.into()),
    }

    fn rm_contents(dir: &cap_std::fs::Dir) -> Result<()> {
        for entry in dir.entries()? {
            let entry = entry?;
            let file_name = entry.file_name();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                let metadata = entry.metadata()?;
                let mut permissions = metadata.permissions();
                if permissions.readonly() {
                    permissions.set_readonly(false);
                    // This may fail for empty directories owned by root.
                    // Continue anyway.
                    let _ = dir.set_permissions(&file_name, permissions);
                }
                let child_dir = entry.open_dir()?;
                rm_contents(&child_dir)?;
                dir.remove_dir(file_name)?;
            } else {
                dir.remove_file(file_name)?;
            }
        }
        Ok(())
    }

    let dir = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
    let _ = rm_contents(&dir); // ignore this error
    dir.remove_open_dir_all()?; // prefer this one
    Ok(())
}

pub fn file_size(metadata: &std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    return {
        use std::os::unix::fs::MetadataExt;
        Some(metadata.size())
    };
    #[cfg(windows)]
    return {
        use std::os::windows::fs::MetadataExt;
        if metadata.is_dir() {
            None
        } else {
            Some(metadata.file_size())
        }
    };
    #[allow(unreachable_code)]
    None
}

pub fn file_size_cap(metadata: &cap_std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    return {
        use std::os::unix::fs::MetadataExt;
        Some(metadata.size())
    };
    #[allow(unreachable_code)]
    None
}

pub struct DirSummary {
    pub errors: bool,
    pub total_size: u64,
    pub last_modified: SystemTime,
}

pub fn summarize_dir(path: &Path) -> Result<DirSummary> {
    fn handle_entry(
        summary: &mut DirSummary,
        entry: io::Result<cap_std::fs::DirEntry>,
    ) -> Result<()> {
        let entry = entry?;
        let metadata = entry.metadata()?;
        match metadata.modified() {
            Ok(time) => {
                let time = time.into_std();
                if time > summary.last_modified {
                    summary.last_modified = time;
                }
            }
            Err(_) => {
                summary.errors = true;
            }
        }
        if metadata.is_dir() {
            let child_dir = entry.open_dir()?;
            handle_dir(summary, child_dir);
        } else {
            match file_size_cap(&metadata) {
                Some(size) => summary.total_size += size,
                None => summary.errors = true,
            }
        }
        Ok(())
    }

    fn handle_dir(summary: &mut DirSummary, dir: cap_std::fs::Dir) {
        match dir.entries() {
            Ok(entries) => {
                for entry in entries {
                    if handle_entry(summary, entry).is_err() {
                        summary.errors = true;
                    }
                }
            }
            Err(_) => {
                summary.errors = true;
            }
        }
    }

    let dir = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
    let mut summary = DirSummary {
        errors: false,
        total_size: 0,
        last_modified: UNIX_EPOCH,
    };
    handle_dir(&mut summary, dir);
    Ok(summary)
}

pub fn try_iterdir(path: &Path) -> Result<Vec<OsString>> {
    let readdir = std::fs::read_dir(path);
    if matches!(&readdir, Err(e) if e.kind() == io::ErrorKind::NotFound) {
        return Ok(Vec::new());
    };
    let mut names = readdir?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    names.sort_unstable();
    Ok(names)
}

pub struct MaybeTempFile(pub PathBuf);

impl Deref for MaybeTempFile {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for MaybeTempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
