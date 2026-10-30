use crate::{
    FilesystemIdentity, MAX_REGISTRY_BYTES, PrefixError, PrefixFilesystem, PrefixGuard, PrefixId,
    read_bounded,
};
use fs2::FileExt;
use std::{
    ffi::{CString, OsStr},
    fs::{self, File},
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

pub struct LinuxPrefixFilesystem {
    data_dir: PathBuf,
    root: PathBuf,
    external_roots: Vec<PathBuf>,
}

impl LinuxPrefixFilesystem {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        let root = data_dir.join("prefixes");
        Self {
            data_dir,
            root,
            external_roots: known_external_roots(),
        }
    }

    fn reject_external_alias(&self) -> Result<(), PrefixError> {
        if self
            .external_roots
            .iter()
            .any(|external| self.root.starts_with(external))
        {
            return Err(PrefixError::Blocked);
        }
        Ok(())
    }

    fn layout(&self, create: bool) -> Result<Option<Layout>, PrefixError> {
        self.reject_external_alias()?;
        let Some(app_data) = open_path_directory(&self.data_dir, create, true)? else {
            return Ok(None);
        };
        verify_private_dir(&app_data)?;
        let root = match child_dir(&app_data, OsStr::new("prefixes"), create)? {
            Some(root) => root,
            None => return Ok(None),
        };
        verify_private_dir(&root)?;
        let root_mount = mount_id(&root)?;
        if mount_id(&app_data)? != root_mount {
            return Err(PrefixError::Blocked);
        }
        let data = match child_dir(&root, OsStr::new("data"), create)? {
            Some(data) => data,
            None => return Ok(None),
        };
        verify_private_dir(&data)?;
        let locks = match child_dir(&root, OsStr::new("locks"), create)? {
            Some(locks) => locks,
            None => return Ok(None),
        };
        verify_private_dir(&locks)?;
        if mount_id(&data)? != root_mount || mount_id(&locks)? != root_mount {
            return Err(PrefixError::Blocked);
        }
        Ok(Some(Layout {
            root,
            data,
            locks,
            root_mount,
        }))
    }

    fn open_lock_file(&self, name: &str) -> Result<Box<dyn PrefixGuard>, PrefixError> {
        let layout = self.layout(true)?.ok_or(PrefixError::Blocked)?;
        let file = open_at(&layout.locks, name, libc::O_RDWR | libc::O_CREAT, 0o600)?;
        verify_private_file(&file)?;
        file.lock_exclusive()?;
        Ok(Box::new(LinuxPrefixGuard { file }))
    }

    fn read_registry_from(
        &self,
        root: &File,
    ) -> Result<Option<(Vec<u8>, FilesystemIdentity)>, PrefixError> {
        let file = match open_at(root, "registry.json", libc::O_RDONLY, 0) {
            Ok(file) => file,
            Err(PrefixError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(PrefixError::Io(error))
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ELOOP) | Some(libc::ENOTDIR)
                ) =>
            {
                return Err(PrefixError::Blocked);
            }
            Err(error) => return Err(error),
        };
        verify_private_file(&file)?;
        let metadata = file.metadata()?;
        let identity = FilesystemIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        let bytes = read_bounded(file, MAX_REGISTRY_BYTES)?;
        Ok(Some((bytes, identity)))
    }

    fn save_registry(
        &self,
        expected: Option<&[u8]>,
        replacement: &[u8],
    ) -> Result<(), PrefixError> {
        if replacement.len() > MAX_REGISTRY_BYTES {
            return Err(PrefixError::RegistryCorrupt);
        }
        let layout = self.layout(true)?.ok_or(PrefixError::Blocked)?;
        let old = self.read_registry_from(&layout.root)?;
        match (expected, old.as_ref()) {
            (None, None) => {}
            (Some(expected), Some((actual, _))) if expected == actual => {}
            _ => return Err(PrefixError::RevisionConflict),
        }

        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = format!("registry.tmp-{}-{sequence}", unsafe { libc::getpid() });
        let file = open_at(
            &layout.root,
            &temporary,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        verify_private_file(&file)?;
        let mut file = file;
        if let Err(error) = file.write_all(replacement).and_then(|()| file.sync_all()) {
            let _ = unlink_at(&layout.root, &temporary, 0);
            return Err(PrefixError::Io(error));
        }

        let temp_c = cstring(&temporary)?;
        let registry_c = cstring("registry.json")?;
        let rename = match old {
            None => unsafe {
                libc::renameat2(
                    layout.root.as_raw_fd(),
                    temp_c.as_ptr(),
                    layout.root.as_raw_fd(),
                    registry_c.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            },
            Some((_, expected_identity)) => {
                let rc = unsafe {
                    libc::renameat2(
                        layout.root.as_raw_fd(),
                        temp_c.as_ptr(),
                        layout.root.as_raw_fd(),
                        registry_c.as_ptr(),
                        libc::RENAME_EXCHANGE,
                    )
                };
                if rc == 0 {
                    let old_at_temp = open_at(&layout.root, &temporary, libc::O_RDONLY, 0)?;
                    let actual = read_bounded(old_at_temp.try_clone()?, MAX_REGISTRY_BYTES)?;
                    let metadata = old_at_temp.metadata()?;
                    let exchanged_identity = FilesystemIdentity {
                        device: metadata.dev(),
                        inode: metadata.ino(),
                    };
                    if actual != expected.unwrap_or_default()
                        || exchanged_identity != expected_identity
                    {
                        let rollback = unsafe {
                            libc::renameat2(
                                layout.root.as_raw_fd(),
                                temp_c.as_ptr(),
                                layout.root.as_raw_fd(),
                                registry_c.as_ptr(),
                                libc::RENAME_EXCHANGE,
                            )
                        };
                        if rollback != 0 {
                            return Err(PrefixError::RecoveryRequired);
                        }
                        let _ = unlink_at(&layout.root, &temporary, 0);
                        return Err(PrefixError::Blocked);
                    }
                    let _ = unlink_at(&layout.root, &temporary, 0);
                }
                rc
            }
        };
        if rename != 0 {
            let error = std::io::Error::last_os_error();
            let _ = unlink_at(&layout.root, &temporary, 0);
            return Err(PrefixError::Io(error));
        }
        layout.root.sync_all()?;
        Ok(())
    }

    fn prefix_lock_name(id: &PrefixId) -> String {
        format!("prefix-{}.lock", id.as_str())
    }
}

impl PrefixFilesystem for LinuxPrefixFilesystem {
    fn root_path(&self) -> &Path {
        &self.root
    }

    fn prepare(&self, create: bool) -> Result<(), PrefixError> {
        let _ = self.layout(create)?;
        Ok(())
    }

    fn preflight_destination(&self, _id: &PrefixId) -> Result<(), PrefixError> {
        self.reject_external_alias()?;
        let mut candidate = self.root.join("data");
        loop {
            if let Some(directory) = open_path_directory(&candidate, false, false)? {
                if candidate.starts_with(&self.data_dir) {
                    verify_private_dir(&directory)?;
                }
                let dot = cstring(".")?;
                let result = unsafe {
                    libc::faccessat(
                        directory.as_raw_fd(),
                        dot.as_ptr(),
                        libc::W_OK | libc::X_OK,
                        libc::AT_EACCESS,
                    )
                };
                if result != 0 {
                    return Err(PrefixError::Blocked);
                }
                return Ok(());
            }
            candidate = candidate
                .parent()
                .ok_or(PrefixError::Blocked)?
                .to_path_buf();
        }
    }

    fn lock_registry(&self) -> Result<Box<dyn PrefixGuard>, PrefixError> {
        self.open_lock_file("registry.lock")
    }

    fn lock_prefix(&self, id: &PrefixId) -> Result<Box<dyn PrefixGuard>, PrefixError> {
        self.open_lock_file(&Self::prefix_lock_name(id))
    }

    fn load_registry_bytes(&self) -> Result<Option<Vec<u8>>, PrefixError> {
        let Some(layout) = self.layout(false)? else {
            return Ok(None);
        };
        Ok(self
            .read_registry_from(&layout.root)?
            .map(|(bytes, _)| bytes))
    }

    fn save_registry_bytes(
        &self,
        expected_current: Option<&[u8]>,
        replacement: &[u8],
    ) -> Result<(), PrefixError> {
        self.save_registry(expected_current, replacement)
    }

    fn inspect_directory(&self, id: &PrefixId) -> Result<Option<FilesystemIdentity>, PrefixError> {
        let Some(layout) = self.layout(false)? else {
            return Ok(None);
        };
        let directory = match open_at(
            &layout.data,
            id.as_str(),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        ) {
            Ok(directory) => directory,
            Err(PrefixError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(PrefixError::Io(error))
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ELOOP) | Some(libc::ENOTDIR)
                ) =>
            {
                return Err(PrefixError::Blocked);
            }
            Err(error) => return Err(error),
        };
        verify_private_dir(&directory)?;
        if mount_id(&directory)? != layout.root_mount {
            return Err(PrefixError::Blocked);
        }
        let metadata = directory.metadata()?;
        Ok(Some(FilesystemIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }))
    }

    fn directory_is_empty(&self, id: &PrefixId) -> Result<bool, PrefixError> {
        let Some(layout) = self.layout(false)? else {
            return Ok(false);
        };
        let Some(directory) = self.open_prefix(&layout, id)? else {
            return Ok(false);
        };
        if let Some(entry) =
            fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))?.next()
        {
            let _entry = entry?;
            return Ok(false);
        }
        Ok(true)
    }

    fn create_directory(&self, id: &PrefixId) -> Result<FilesystemIdentity, PrefixError> {
        let layout = self.layout(true)?.ok_or(PrefixError::Blocked)?;
        let name = cstring(id.as_str())?;
        let result = unsafe { libc::mkdirat(layout.data.as_raw_fd(), name.as_ptr(), 0o700) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            return if error.kind() == std::io::ErrorKind::AlreadyExists {
                Err(PrefixError::DestinationCollision)
            } else {
                Err(PrefixError::Io(error))
            };
        }
        layout.data.sync_all()?;
        self.inspect_directory(id)?.ok_or(PrefixError::Blocked)
    }

    fn remove_directory(
        &self,
        id: &PrefixId,
        expected: FilesystemIdentity,
    ) -> Result<(), PrefixError> {
        let layout = self.layout(false)?.ok_or(PrefixError::Blocked)?;
        let directory = self.open_prefix(&layout, id)?.ok_or(PrefixError::Missing)?;
        let metadata = directory.metadata()?;
        let identity = FilesystemIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        if identity != expected || mount_id(&directory)? != layout.root_mount {
            return Err(PrefixError::Blocked);
        }
        remove_tree(&layout.data, id.as_str(), expected, layout.root_mount)?;
        layout.data.sync_all()?;
        Ok(())
    }

    fn prefix_in_use(&self, id: &PrefixId) -> Result<Option<bool>, PrefixError> {
        let target = self.target_path(id);
        let target_bytes = target.as_os_str().as_bytes();
        let processes = match fs::read_dir("/proc") {
            Ok(processes) => processes,
            Err(_) => return Ok(None),
        };
        for process in processes {
            let process = match process {
                Ok(process) => process,
                Err(_) => return Ok(None),
            };
            if process.file_name().to_str().is_none_or(|name| {
                name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit())
            }) {
                continue;
            }
            let process_dir = process.path();
            let metadata = match fs::metadata(&process_dir) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Ok(None),
            };
            if metadata.uid() != unsafe { libc::geteuid() } {
                continue;
            }
            let environment = match File::open(process_dir.join("environ")) {
                Ok(environment) => environment,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Ok(None),
            };
            let environment = match read_bounded(environment, 1024 * 1024) {
                Ok(environment) => environment,
                Err(_) => return Ok(None),
            };
            for variable in environment.split(|byte| *byte == 0) {
                let Some(separator) = variable.iter().position(|byte| *byte == b'=') else {
                    continue;
                };
                if &variable[..separator] == b"WINEPREFIX"
                    && &variable[separator + 1..] == target_bytes
                {
                    return Ok(Some(true));
                }
            }
        }
        Ok(Some(false))
    }

    fn target_path(&self, id: &PrefixId) -> PathBuf {
        self.root.join("data").join(id.as_str())
    }

    fn free_bytes(&self) -> Option<u64> {
        self.reject_external_alias().ok()?;
        if let Some(layout) = self.layout(false).ok().flatten() {
            return free_bytes_for(&layout.data);
        }

        // A read-only plan should report the capacity of the nearest existing
        // destination ancestor even before the managed prefix directories exist.
        let mut candidate = self.root.join("data");
        loop {
            if let Some(directory) = open_path_directory(&candidate, false, false).ok()? {
                if candidate.starts_with(&self.data_dir) {
                    verify_private_dir(&directory).ok()?;
                }
                return free_bytes_for(&directory);
            }
            candidate = candidate.parent()?.to_path_buf();
        }
    }
}

fn free_bytes_for(directory: &File) -> Option<u64> {
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::fstatvfs(directory.as_raw_fd(), stats.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let stats = unsafe { stats.assume_init() };
    Some(stats.f_bavail.saturating_mul(stats.f_frsize))
}

impl LinuxPrefixFilesystem {
    fn open_prefix(&self, layout: &Layout, id: &PrefixId) -> Result<Option<File>, PrefixError> {
        match open_at(
            &layout.data,
            id.as_str(),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        ) {
            Ok(directory) => {
                verify_private_dir(&directory)?;
                if mount_id(&directory)? != layout.root_mount {
                    return Err(PrefixError::Blocked);
                }
                Ok(Some(directory))
            }
            Err(PrefixError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(PrefixError::Io(error))
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ELOOP) | Some(libc::ENOTDIR)
                ) =>
            {
                Err(PrefixError::Blocked)
            }
            Err(error) => Err(error),
        }
    }
}

struct Layout {
    root: File,
    data: File,
    locks: File,
    root_mount: u64,
}

struct LinuxPrefixGuard {
    file: File,
}

impl PrefixGuard for LinuxPrefixGuard {}

impl Drop for LinuxPrefixGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn known_external_roots() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    [
        ".wine",
        ".local/share/Steam/steamapps/compatdata",
        ".steam/steam/steamapps/compatdata",
        ".var/app/com.valvesoftware.Steam/data/Steam/steamapps/compatdata",
        ".local/share/bottles",
        ".var/app/com.usebottles.bottles/data/bottles",
        ".local/share/lutris",
        ".var/app/net.lutris.Lutris/data/lutris",
        ".config/heroic",
        ".var/app/com.heroicgameslauncher.hgl/config/heroic",
    ]
    .into_iter()
    .map(|relative| home.join(relative))
    .collect()
}

fn open_path_directory(
    path: &Path,
    create: bool,
    secure_final: bool,
) -> Result<Option<File>, PrefixError> {
    let raw = path.as_os_str().as_bytes();
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        || raw
            .split(|byte| *byte == b'/')
            .skip(1)
            .any(|component| component.is_empty() || component == b"." || component == b"..")
    {
        return Err(PrefixError::Blocked);
    }
    let mut current = File::open("/")?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        match child_dir(&current, name, create)? {
            Some(next) => current = next,
            None => return Ok(None),
        }
    }
    if secure_final {
        verify_private_dir(&current)?;
    }
    Ok(Some(current))
}

fn child_dir(parent: &File, name: &OsStr, create: bool) -> Result<Option<File>, PrefixError> {
    let name_c = cstring_os(name)?;
    if create {
        let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name_c.as_ptr(), 0o700) };
        if result == 0 {
            parent.sync_all()?;
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(PrefixError::Io(error));
            }
        }
    }
    match open_at_cstr(parent, &name_c, libc::O_RDONLY | libc::O_DIRECTORY, 0) {
        Ok(directory) => Ok(Some(directory)),
        Err(PrefixError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(PrefixError::Io(error))
            if matches!(
                error.raw_os_error(),
                Some(libc::ELOOP) | Some(libc::ENOTDIR)
            ) =>
        {
            Err(PrefixError::Blocked)
        }
        Err(error) => Err(error),
    }
}

fn open_at(parent: &File, name: &str, flags: i32, mode: u32) -> Result<File, PrefixError> {
    let name = cstring(name)?;
    open_at_cstr(parent, &name, flags, mode)
}

fn open_at_cstr(
    parent: &File,
    name: &std::ffi::CStr,
    flags: i32,
    mode: u32,
) -> Result<File, PrefixError> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            mode,
        )
    };
    if fd < 0 {
        return Err(PrefixError::Io(std::io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn verify_private_dir(directory: &File) -> Result<(), PrefixError> {
    let metadata = directory.metadata()?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(PrefixError::Blocked);
    }
    Ok(())
}

fn verify_private_file(file: &File) -> Result<(), PrefixError> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(PrefixError::Blocked);
    }
    Ok(())
}

fn mount_id(file: &File) -> Result<u64, PrefixError> {
    let text = fs::read_to_string(format!("/proc/self/fdinfo/{}", file.as_raw_fd()))?;
    text.lines()
        .find_map(|line| {
            line.strip_prefix("mnt_id:")
                .and_then(|id| id.trim().parse().ok())
        })
        .ok_or(PrefixError::Blocked)
}

fn cstring(value: &str) -> Result<CString, PrefixError> {
    CString::new(value).map_err(|_| PrefixError::Blocked)
}

fn cstring_os(value: &OsStr) -> Result<CString, PrefixError> {
    CString::new(value.as_bytes()).map_err(|_| PrefixError::Blocked)
}

fn unlink_at(parent: &File, name: &str, flags: i32) -> Result<(), PrefixError> {
    let name = cstring(name)?;
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } != 0 {
        return Err(PrefixError::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}

fn remove_tree(
    parent: &File,
    name: &str,
    expected: FilesystemIdentity,
    expected_mount: u64,
) -> Result<(), PrefixError> {
    let name_c = cstring(name)?;
    let directory_fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if directory_fd < 0 {
        let error = std::io::Error::last_os_error();
        if matches!(
            error.raw_os_error(),
            Some(libc::ENOTDIR) | Some(libc::ELOOP)
        ) {
            return Err(PrefixError::Blocked);
        }
        return Err(PrefixError::Io(error));
    }
    let directory = unsafe { File::from_raw_fd(directory_fd) };
    let metadata = directory.metadata()?;
    if (FilesystemIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }) != expected
        || mount_id(&directory)? != expected_mount
    {
        return Err(PrefixError::Blocked);
    }
    for entry in fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))? {
        let entry = entry?;
        let child_os = entry.file_name();
        let child = child_os.to_str().ok_or(PrefixError::Blocked)?.to_owned();
        if child == "." || child == ".." {
            continue;
        }
        remove_entry(&directory, &child, expected_mount)?;
    }
    drop(directory);
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name_c.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        return Err(PrefixError::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}

fn remove_entry(parent: &File, name: &str, expected_mount: u64) -> Result<(), PrefixError> {
    let name_c = cstring(name)?;
    let directory_fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if directory_fd < 0 {
        let error = std::io::Error::last_os_error();
        if matches!(
            error.raw_os_error(),
            Some(libc::ENOTDIR) | Some(libc::ELOOP)
        ) {
            if unsafe { libc::unlinkat(parent.as_raw_fd(), name_c.as_ptr(), 0) } == 0 {
                return Ok(());
            }
            return Err(PrefixError::Io(std::io::Error::last_os_error()));
        }
        return Err(PrefixError::Io(error));
    }
    let directory = unsafe { File::from_raw_fd(directory_fd) };
    if mount_id(&directory)? != expected_mount {
        return Err(PrefixError::Blocked);
    }
    for entry in fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))? {
        let entry = entry?;
        let child = entry
            .file_name()
            .to_str()
            .ok_or(PrefixError::Blocked)?
            .to_owned();
        if child == "." || child == ".." {
            continue;
        }
        remove_entry(&directory, &child, expected_mount)?;
    }
    drop(directory);
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name_c.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        return Err(PrefixError::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}
