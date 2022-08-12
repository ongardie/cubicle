use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};
use std::ffi::OsString;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

pub struct DiskUsage {
    pub error: bool,
    pub size: u64,
    pub mtime: SystemTime,
}

pub fn du(path: &Path) -> Result<DiskUsage> {
    let output = Command::new("du")
        .args(["-cs", "--block-size=1", "--time", "--time-style=+%s"])
        .arg(path)
        .output()?;
    // ignore permissions errors
    let error = !&output.stderr.is_empty();

    let stdout = String::from_utf8(output.stdout)?;

    lazy_static! {
        static ref RE: Regex = RegexBuilder::new(r#"^(?P<size>[0-9]+)\t(?P<mtime>[0-9]+)\ttotal$"#)
            .multi_line(true)
            .build()
            .unwrap();
    }
    match RE.captures(&stdout) {
        Some(caps) => {
            let size = caps.name("size").unwrap().as_str();
            let size = u64::from_str(size).unwrap();
            let mtime = caps.name("mtime").unwrap().as_str();
            let mtime = u64::from_str(mtime).unwrap();
            let mtime = UNIX_EPOCH + Duration::from_secs(mtime);
            Ok(DiskUsage { error, size, mtime })
        }
        None => Err(anyhow!("Unexpected output from du: {:#?}", stdout)),
    }
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
