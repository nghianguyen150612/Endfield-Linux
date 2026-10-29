//! Linux directory capabilities. Every component is opened without following links.
use crate::{Error, Result};
use std::{
    ffi::CString,
    fs::File,
    os::fd::{AsRawFd, FromRawFd},
    path::{Component, Path},
};
fn c(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::UnsafePath)
}
pub fn open(parent: &File, name: &str, flags: i32, mode: u32) -> Result<File> {
    let name = c(name)?;
    // SAFETY: live directory descriptor, terminated name; returned descriptor is owned once.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            mode,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
pub fn dir(parent: &File, name: &str, create: bool) -> Result<File> {
    if create {
        let n = c(name)?;
        let rc = unsafe { libc::mkdirat(parent.as_raw_fd(), n.as_ptr(), 0o700) };
        if rc == 0 {
            parent.sync_all()?;
        } else if std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    open(parent, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)
}
pub fn root(path: &Path, create: bool) -> Result<File> {
    if !path.is_absolute() {
        return Err(Error::UnsafePath);
    }
    let mut f = File::open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(s) => f = dir(&f, s.to_str().ok_or(Error::UnsafePath)?, create)?,
            _ => return Err(Error::UnsafePath),
        }
    }
    private(&f)?;
    Ok(f)
}
pub fn private(f: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let m = f.metadata()?;
    if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
        return Err(Error::UnsafeOwnership);
    }
    Ok(())
}
pub fn private_lock(f: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let m = f.metadata()?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o7777 != 0o600
        || m.nlink() != 1
    {
        return Err(Error::UnsafeOwnership);
    }
    Ok(())
}
pub fn publish(from: &File, name: &str, to: &File, target: &str) -> Result<()> {
    let a = c(name)?;
    let b = c(target)?;
    let rc = unsafe {
        libc::renameat2(
            from.as_raw_fd(),
            a.as_ptr(),
            to.as_raw_fd(),
            b.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if to.sync_all().is_err() || from.sync_all().is_err() {
        return Err(Error::ReconciliationRequired);
    }
    Ok(())
}
pub fn link(parent: &File, name: &str, target: &str) -> Result<()> {
    let n = c(name)?;
    let t = c(target)?;
    if unsafe { libc::symlinkat(t.as_ptr(), parent.as_raw_fd(), n.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// Remove a child tree using directory-relative, no-follow operations. Callers
/// must first establish that the named tree is application-owned and authorized.
pub fn remove_tree(parent: &File, name: &str) -> Result<()> {
    let name_c = c(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if matches!(
            error.raw_os_error(),
            Some(libc::ENOTDIR) | Some(libc::ELOOP)
        ) {
            if unsafe { libc::unlinkat(parent.as_raw_fd(), name_c.as_ptr(), 0) } == 0 {
                return Ok(());
            }
            return Err(std::io::Error::last_os_error().into());
        }
        return Err(error.into());
    }
    let directory = unsafe { File::from_raw_fd(fd) };
    for entry in std::fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))? {
        let entry = entry?;
        let Some(child) = entry.file_name().to_str().map(str::to_owned) else {
            return Err(Error::UnsafePath);
        };
        if child == "." || child == ".." {
            continue;
        }
        remove_tree(&directory, &child)?;
    }
    drop(directory);
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name_c.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
