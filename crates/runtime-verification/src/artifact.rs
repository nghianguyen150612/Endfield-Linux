use crate::{Error, MAX_ARTIFACT_BYTES, io_error};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, Metadata},
    io::Read,
    path::Path,
};

fn same(a: &Metadata, b: &Metadata) -> bool {
    let common =
        a.len() == b.len() && matches!((a.modified(), b.modified()), (Ok(x), Ok(y)) if x == y);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        common
            && a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        common
    }
}
/// Opens once, rejects final symlinks, and hashes the same read-only handle.
/// Linux O_NONBLOCK prevents FIFO open from blocking before type validation.
/// Parent symlinks are allowed. Concurrent hostile writes remain a limitation.
pub fn open_artifact(path: &Path) -> Result<File, Error> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| {
                if e.raw_os_error() == Some(libc::ELOOP) {
                    Error::FileType
                } else {
                    io_error(e)
                }
            })?;
        if !f.metadata().map_err(io_error)?.is_file() {
            return Err(Error::FileType);
        }
        Ok(f)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(Error::FileType) // no portable race-safe path open available; use an owned handle
    }
}
#[derive(Debug)]
pub struct FileDigest {
    pub size: u64,
    pub sha256: String,
}
/// Consumes the caller's handle; hashes from offset zero with bounded memory.
pub fn hash_artifact(file: File) -> Result<FileDigest, Error> {
    hash_with_observer(file, |_| {})
}

fn hash_with_observer(mut file: File, observer: impl FnOnce(&File)) -> Result<FileDigest, Error> {
    use std::io::{Seek, SeekFrom};
    let before = file.metadata().map_err(io_error)?;
    if !before.is_file() {
        return Err(Error::FileType);
    }
    if before.len() > MAX_ARTIFACT_BYTES {
        return Err(Error::TooLarge);
    }
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    observer(&file);
    let mut h = Sha256::new();
    let mut count = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = match file.read(&mut buf) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            r => r.map_err(io_error)?,
        };
        if n == 0 {
            break;
        }
        count = count
            .checked_add(u64::try_from(n).map_err(|_| Error::TooLarge)?)
            .ok_or(Error::TooLarge)?;
        if count > MAX_ARTIFACT_BYTES {
            return Err(Error::TooLarge);
        }
        h.update(&buf[..n]);
    }
    let after = file.metadata().map_err(io_error)?;
    if count != before.len() || !same(&before, &after) {
        return Err(Error::ChangedDuringRead);
    }
    Ok(FileDigest {
        size: count,
        sha256: format!("{:x}", h.finalize()),
    })
}
/// Bounded read for manifest/signature/key files using the same safe file policy.
pub fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, Error> {
    let f = open_artifact(path)?;
    let before = f.metadata().map_err(io_error)?;
    let mut bytes = Vec::new();
    (&f).take(u64::try_from(maximum).map_err(|_| Error::TooLarge)? + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > maximum {
        return Err(Error::TooLarge);
    }
    if !same(&before, &f.metadata().map_err(io_error)?)
        || u64::try_from(bytes.len()).map_err(|_| Error::TooLarge)? != before.len()
    {
        return Err(Error::ChangedDuringRead);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_change_between_metadata_and_read() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("artifact");
        std::fs::write(&p, b"before").unwrap();
        let f = open_artifact(&p).unwrap();
        let result = hash_with_observer(f, |_| std::fs::write(&p, b"after and larger").unwrap());
        assert!(matches!(result, Err(Error::ChangedDuringRead)));
    }
    #[test]
    fn read_failure_and_size_bound() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("artifact");
        let f = File::create(&p).unwrap();
        assert!(matches!(hash_artifact(f), Err(Error::ReadFailure)));
        let f = File::create(&p).unwrap();
        f.set_len(MAX_ARTIFACT_BYTES + 1).unwrap();
        drop(f);
        assert!(matches!(
            hash_artifact(open_artifact(&p).unwrap()),
            Err(Error::TooLarge)
        ));
    }
}
