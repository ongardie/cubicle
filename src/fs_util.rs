use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::HostPath;
use crate::somehow::{somehow as anyhow, Context, Result};

pub fn rmtree(path: &HostPath) -> Result<()> {
    rmtree_(path).with_context(|| format!("Failed to recursively remove directory: {:?}", path))
}

fn rmtree_(path: &HostPath) -> Result<()> {
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

    let dir = match cap_std::fs::Dir::open_ambient_dir(
        path.as_host_raw(),
        cap_std::ambient_authority(),
    ) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).todo_context(),
    };
    match dir.remove_open_dir_all() {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            // continue below
        }
        Err(e) => return Err(e).todo_context(),
    }

    fn rm_contents(dir: &cap_std::fs::Dir) -> std::io::Result<()> {
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

    let dir = cap_std::fs::Dir::open_ambient_dir(path.as_host_raw(), cap_std::ambient_authority())
        .todo_context()?;
    let _ = rm_contents(&dir); // ignore this error
    dir.remove_open_dir_all().todo_context()?; // prefer this one
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

#[derive(Debug)]
pub struct DirSummary {
    pub errors: bool,
    pub total_size: u64,
    pub last_modified: SystemTime,
}

impl DirSummary {
    pub fn new_with_errors() -> Self {
        Self {
            errors: true,
            total_size: 0,
            last_modified: UNIX_EPOCH,
        }
    }
}

pub fn summarize_dir(path: &HostPath) -> Result<DirSummary> {
    fn handle_entry(summary: &mut DirSummary, entry: Result<WalkDirEntry>) {
        match entry {
            Ok(WalkDirEntry { entry, .. }) => {
                let metadata = if let Ok(metadata) = entry.metadata() {
                    metadata
                } else {
                    summary.errors = true;
                    return;
                };
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
                if !metadata.is_dir() {
                    match file_size_cap(&metadata) {
                        Some(size) => summary.total_size += size,
                        None => summary.errors = true,
                    }
                }
            }
            Err(_) => summary.errors = true,
        }
    }

    let mut summary = DirSummary {
        errors: false,
        total_size: 0,
        last_modified: UNIX_EPOCH,
    };
    for entry in WalkDir::new(path)? {
        handle_entry(&mut summary, entry);
    }
    Ok(summary)
}

pub fn try_iterdir(path: &HostPath) -> Result<Vec<OsString>> {
    let readdir = std::fs::read_dir(path.as_host_raw());
    if matches!(&readdir, Err(e) if e.kind() == io::ErrorKind::NotFound) {
        return Ok(Vec::new());
    };
    let mut names = readdir
        .todo_context()?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()
        .todo_context()?;
    names.sort_unstable();
    Ok(names)
}

pub struct WalkDirCursor {
    path: PathBuf,
    dir: Rc<cap_std::fs::Dir>,
    entries: cap_std::fs::ReadDir,
}

pub struct WalkDirEntry {
    parent: Rc<cap_std::fs::Dir>,
    path: PathBuf,
    entry: cap_std::fs::DirEntry,
    file_type: cap_std::fs::FileType,
}

pub struct WalkDir {
    stack: Vec<WalkDirCursor>,
}

impl WalkDir {
    pub fn new(path: &HostPath) -> Result<WalkDir> {
        let dir =
            cap_std::fs::Dir::open_ambient_dir(path.as_host_raw(), cap_std::ambient_authority())
                .todo_context()?;
        let entries = dir.entries().todo_context()?;
        Ok(WalkDir {
            stack: vec![WalkDirCursor {
                path: PathBuf::new(),
                dir: Rc::new(dir),
                entries,
            }],
        })
    }
}

impl Iterator for WalkDir {
    type Item = Result<WalkDirEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        let (parent, path, entry) = loop {
            match self.stack.pop() {
                Some(mut cursor) => match cursor.entries.next() {
                    Some(entry) => {
                        match entry
                            .with_context(|| format!("Failed to list directory {:#?}", cursor.path))
                        {
                            Ok(entry) => {
                                let entry_path = cursor.path.join(entry.file_name());
                                let parent = cursor.dir.clone();
                                self.stack.push(cursor);
                                break (parent, entry_path, entry);
                            }
                            Err(e) => {
                                self.stack.push(cursor);
                                return Some(Err(e));
                            }
                        }
                    }
                    None => continue,
                },
                None => return None,
            };
        };

        match entry
            .file_type()
            .with_context(|| format!("Failed to get file type for {path:#?}"))
        {
            Ok(file_type) => {
                if file_type.is_dir() {
                    match entry
                        .open_dir()
                        .and_then(|dir| dir.entries().map(|entries| (dir, entries)))
                        .with_context(|| format!("Failed to list directory {path:#?}"))
                    {
                        Ok((dir, contents)) => self.stack.push(WalkDirCursor {
                            path: path.clone(),
                            dir: Rc::new(dir),
                            entries: contents,
                        }),
                        Err(e) => return Some(Err(e)),
                    }
                }
                Some(Ok(WalkDirEntry {
                    parent,
                    path,
                    entry,
                    file_type,
                }))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

#[derive(Default)]
pub struct TarOptions {
    pub prefix: Option<PathBuf>,
    pub exclude: Vec<PathBuf>,
}

pub fn create_tar_from_dir<W: io::Write>(dir: &HostPath, w: W, opts: &TarOptions) -> Result<()> {
    let mut builder = tar::Builder::new(w);
    for entry in WalkDir::new(dir)? {
        let WalkDirEntry {
            parent,
            path,
            entry,
            file_type,
        } = entry?;
        let mut add = || {
            if opts.exclude.contains(&path) {
                return Ok(());
            }
            let append_path = match &opts.prefix {
                Some(prefix) => prefix.join(&path),
                None => path.clone(),
            };
            if file_type.is_file() {
                let file = entry.open().todo_context()?;
                builder
                    .append_file(append_path, &mut file.into_std())
                    .todo_context()?;
                return Ok(());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let metadata = entry.metadata().todo_context()?;
                let mut header = tar::Header::new_gnu();
                header.set_mtime(metadata.mtime() as u64);
                header.set_uid(metadata.uid() as u64);
                header.set_gid(metadata.gid() as u64);
                header.set_mode(metadata.mode());
                if file_type.is_dir() {
                    header.set_entry_type(tar::EntryType::Directory);
                    builder
                        .append_data(&mut header, append_path, io::empty())
                        .todo_context()?;
                    return Ok(());
                } else if file_type.is_symlink() {
                    header.set_entry_type(tar::EntryType::Symlink);
                    let target = parent.read_link(path.file_name().unwrap()).todo_context()?;
                    builder
                        .append_link(&mut header, append_path, target)
                        .todo_context()?;
                    return Ok(());
                }
            }
            Err(anyhow!("Unsupported file type: {file_type:?}"))
        };
        add().with_context(|| format!("Failed to add {:#?} to tar archive", dir.join(path)))?;
    }
    builder
        .into_inner()
        .and_then(|mut f| f.flush())
        .todo_context()?;
    Ok(())
}

pub fn try_exists(path: &HostPath) -> io::Result<bool> {
    // Adapted from rust `library/std/src/sys_common/fs.rs`
    // since `std::fs::try_exists` is unstable
    // and `Path::try_exists` feels dirty.
    match std::fs::metadata(path.as_host_raw()) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
