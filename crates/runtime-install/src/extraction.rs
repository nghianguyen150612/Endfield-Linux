use crate::{Error, Result, fs};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
};
pub const MAX_EXPANDED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 200_000;
pub const MAX_PATH: usize = 4096;
pub const MAX_DEPTH: usize = 32;
pub const MAX_TOTAL_PATH_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SYMLINKS: usize = 8192;
pub const MAX_SYMLINK_TARGET_BYTES: usize = 16 * 1024 * 1024;
fn components(path: &str) -> Result<Vec<&str>> {
    if path.is_empty()
        || path.len() > MAX_PATH
        || path.starts_with('/')
        || path.contains(['\\', ':', '\0'])
    {
        return Err(Error::UnsafePath);
    }
    let parts: Vec<_> = path.trim_end_matches('/').split('/').collect();
    if parts.len() > MAX_DEPTH
        || parts
            .iter()
            .any(|s| s.is_empty() || *s == ".." || *s == ".")
    {
        return Err(Error::UnsafePath);
    }
    Ok(parts)
}
fn parent(root: &File, parts: &[&str]) -> Result<File> {
    let mut f = root.try_clone()?;
    for p in parts {
        f = fs::dir(&f, p, true)?;
    }
    Ok(f)
}
pub fn extract(file: File, root: &File, compressed: u64) -> Result<()> {
    let mut magic = [0; 2];
    (&file).read_exact(&mut magic)?;
    if magic != [0x1f, 0x8b] {
        return Err(Error::UnsupportedFormat);
    }
    use std::io::{Seek, SeekFrom};
    let mut file = file;
    file.seek(SeekFrom::Start(0))?;
    let decoder = flate2::read::MultiGzDecoder::new(file);
    // Bound the entire decoded stream, including headers, padding and metadata.
    let budget =
        MAX_EXPANDED_BYTES.min(compressed.saturating_mul(1000).saturating_add(1024 * 1024));
    let mut archive = tar::Archive::new(decoder.take(budget + 1));
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    let mut path_bytes = 0usize;
    let mut link_bytes = 0usize;
    let mut links = Vec::new();
    for (index, entry) in archive.entries()?.raw(true).enumerate() {
        if index >= MAX_ENTRIES {
            return Err(Error::Limit);
        }
        let mut entry = entry?;
        let raw = entry.path_bytes();
        let path = std::str::from_utf8(&raw)
            .map_err(|_| Error::UnsafePath)?
            .to_owned();
        let parts = components(&path)?;
        path_bytes = path_bytes.checked_add(path.len()).ok_or(Error::Limit)?;
        if path_bytes > MAX_TOTAL_PATH_BYTES {
            return Err(Error::Limit);
        }
        let normalized = parts.join("/");
        if normalized == ".endfield-installation.json" || !seen.insert(normalized) {
            return Err(Error::UnsafePath);
        }
        let size = entry.size();
        total = total.checked_add(size).ok_or(Error::Limit)?;
        if size > MAX_FILE_BYTES || total > budget {
            return Err(Error::Limit);
        }
        let kind = entry.header().entry_type();
        // Raw iteration rejects PAX/GNU metadata before the parser can allocate it.
        if !(kind.is_file() || kind.is_dir() || kind.is_symlink()) {
            return Err(Error::UnsupportedEntry);
        }
        let name = parts.last().ok_or(Error::UnsafePath)?;
        let dir = parent(root, &parts[..parts.len() - 1])?;
        if kind.is_dir() {
            if size != 0 {
                return Err(Error::UnsupportedEntry);
            }
            fs::dir(&dir, name, true)?.sync_all()?;
        } else if kind.is_file() {
            let mut out = fs::open(
                &dir,
                name,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )?;
            if std::io::copy(&mut entry, &mut out)? != size {
                return Err(Error::Layout);
            }
            let executable = entry.header().mode()? & 0o111 != 0;
            out.set_permissions(std::fs::Permissions::from_mode(if executable {
                0o700
            } else {
                0o600
            }))?;
            out.flush()?;
            out.sync_all()?;
        } else {
            let raw = entry.link_name_bytes().ok_or(Error::UnsafePath)?;
            let target = std::str::from_utf8(&raw)
                .map_err(|_| Error::UnsafePath)?
                .to_owned();
            link_bytes = link_bytes.checked_add(target.len()).ok_or(Error::Limit)?;
            if links.len() >= MAX_SYMLINKS || link_bytes > MAX_SYMLINK_TARGET_BYTES {
                return Err(Error::Limit);
            }
            // Conservative relative-link subset: no parent components. Materialize last,
            // require a real regular-file/directory target, so cycles/chains fail closed.
            let target_parts = components(&target)?;
            let mut resolved = parts[..parts.len() - 1].to_vec();
            resolved.extend(target_parts);
            let resolved = resolved.join("/");
            links.push((path.clone(), target, resolved));
        }
        dir.sync_all()?;
    }
    let mut decoded = archive.into_inner();
    let mut sink = std::io::sink();
    std::io::copy(&mut decoded, &mut sink)?;
    if decoded.limit() == 0 {
        return Err(Error::Limit);
    }
    for (path, target, resolved) in links {
        let parts = components(&resolved)?;
        let p = parent_existing(root, &parts[..parts.len() - 1])?;
        let f = fs::open(
            &p,
            parts.last().ok_or(Error::UnsafePath)?,
            libc::O_RDONLY,
            0,
        )?;
        let m = f.metadata()?;
        if !m.is_file() && !m.is_dir() {
            return Err(Error::UnsafePath);
        }
        let parts = components(&path)?;
        let p = parent_existing(root, &parts[..parts.len() - 1])?;
        fs::link(&p, parts.last().ok_or(Error::UnsafePath)?, &target)?;
        p.sync_all()?;
    }
    root.sync_all()?;
    Ok(())
}
fn parent_existing(root: &File, parts: &[&str]) -> Result<File> {
    let mut f = root.try_clone()?;
    for p in parts {
        f = fs::dir(&f, p, false)?;
    }
    Ok(f)
}
pub fn validate(root: &File, family: &str, build: &str) -> Result<()> {
    match fs::open(root, "endfield-layout-v1.json", libc::O_RDONLY, 0) {
        Ok(mut descriptor) => {
            if family != "wine" || build != "wine-10.0" {
                return Err(Error::Layout);
            }
            let mut raw = Vec::new();
            (&mut descriptor).take(16_385).read_to_end(&mut raw)?;
            if raw.len() > 16_384 {
                return Err(Error::Limit);
            }
            return validate_dualarch(root, build, &raw);
        }
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let (launcher, support) = if family == "proton" {
        ("proton", "files")
    } else {
        ("bin/wine", "lib")
    };
    let parts = components(launcher)?;
    let p = parent_existing(root, &parts[..parts.len() - 1])?;
    let file = fs::open(&p, parts.last().ok_or(Error::Layout)?, libc::O_RDONLY, 0)?;
    let m = file.metadata()?;
    if !m.is_file() || m.len() == 0 || m.permissions().mode() & 0o100 == 0 {
        return Err(Error::Layout);
    }
    fs::dir(root, support, false)?;
    let mut metadata = String::new();
    fs::open(root, "version", libc::O_RDONLY, 0)?
        .take(4097)
        .read_to_string(&mut metadata)?;
    if metadata.len() > 4096 || metadata.trim() != build {
        return Err(Error::Layout);
    }
    Ok(())
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DerivedWineLayout {
    schema_version: u32,
    layout: String,
    build: String,
    source_inventory_sha256: String,
    source_packages: Vec<SourcePackage>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcePackage {
    package: String,
    sha256: String,
}
fn validate_dualarch(root: &File, build: &str, raw: &[u8]) -> Result<()> {
    let layout: DerivedWineLayout = serde_json::from_slice(raw).map_err(|_| Error::Layout)?;
    let digest = |v: &str| {
        v.len() == 64
            && v.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    let names: BTreeSet<_> = layout
        .source_packages
        .iter()
        .map(|p| p.package.as_str())
        .collect();
    if layout.schema_version != 1
        || layout.layout != "winehq-dualarch-v1"
        || layout.build != build
        || !digest(&layout.source_inventory_sha256)
        || layout.source_packages.len() != 3
        || names != BTreeSet::from(["wine-stable", "wine-stable-amd64", "wine-stable-i386"])
        || layout.source_packages.iter().any(|p| !digest(&p.sha256))
    {
        return Err(Error::Layout);
    }
    let regular = |path: &str, executable: bool| -> Result<File> {
        let parts = components(path)?;
        let parent = parent_existing(root, &parts[..parts.len() - 1])?;
        let file = fs::open(
            &parent,
            parts.last().ok_or(Error::Layout)?,
            libc::O_RDONLY,
            0,
        )?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.len() == 0
            || (executable && metadata.permissions().mode() & 0o100 == 0)
        {
            return Err(Error::Layout);
        }
        Ok(file)
    };
    let mut version = String::new();
    regular("version", false)?
        .take(4097)
        .read_to_string(&mut version)?;
    if version.len() > 4096 || version.trim() != build {
        return Err(Error::Layout);
    }
    for (path, class, machine) in [
        ("opt/wine-stable/bin/wine64", 2, 62),
        ("opt/wine-stable/bin/wine64-preloader", 2, 62),
        ("opt/wine-stable/bin/wine", 1, 3),
        ("opt/wine-stable/bin/wine-preloader", 1, 3),
        ("opt/wine-stable/bin/wineserver", 2, 62),
    ] {
        let mut header = [0; 64];
        regular(path, true)?.read_exact(&mut header)?;
        if &header[..4] != b"\x7fELF"
            || header[4] != class
            || header[5] != 1
            || header[18] != machine
            || header[19] != 0
            || !matches!(header[16], 2 | 3)
            || header[17] != 0
        {
            return Err(Error::Layout);
        }
    }
    for directory in ["lib/wine/i386", "lib64/wine/x86_64"] {
        for (suffix, names) in [
            ("-unix", &["ntdll.so"][..]),
            (
                "-windows",
                &[
                    "ntdll.dll",
                    "wineboot.exe",
                    "kernel32.dll",
                    "kernelbase.dll",
                    "services.exe",
                    "rundll32.exe",
                    "setupapi.dll",
                    "advapi32.dll",
                    "shell32.dll",
                    "ws2_32.dll",
                    "newdev.dll",
                ][..],
            ),
        ] {
            for name in names {
                regular(
                    &format!("opt/wine-stable/{directory}{suffix}/{name}"),
                    false,
                )?;
            }
        }
    }
    regular("opt/wine-stable/share/wine/wine.inf", false)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packed(items: &[(&str, tar::EntryType, Option<&str>, &[u8])]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (path, kind, target, bytes) in items {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(*kind);
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            if let Some(target) = target {
                header.set_link_name(target).unwrap();
            }
            if path.starts_with('/') || path.starts_with("../") || path.contains("/../") {
                let path_bytes = path.as_bytes();
                header.as_mut_bytes()[..path_bytes.len()].copy_from_slice(path_bytes);
                header.set_cksum();
                builder.append(&header, *bytes).unwrap();
                continue;
            }
            header.set_cksum();
            builder.append_data(&mut header, *path, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }
    fn extract_fixture(bytes: &[u8]) -> Result<()> {
        let root = tempfile::tempdir().unwrap();
        extract(
            File::open({
                let archive = root.path().join("archive");
                std::fs::write(&archive, bytes).unwrap();
                archive
            })?,
            &File::open(root.path())?,
            bytes.len() as u64,
        )
    }
    #[test]
    fn hostile_paths() {
        for p in [
            "../escape",
            "/absolute",
            "a/../../x",
            "a\\b",
            "C:foo",
            "a//b",
            "a/./b",
            "a\0b",
        ] {
            assert!(components(p).is_err(), "{p}");
        }
    }
    #[test]
    fn bounds() {
        assert!(components(&"a/".repeat(33)).is_err());
        assert!(components(&"a".repeat(4097)).is_err());
    }
    #[test]
    fn rejects_absolute_and_parent_archive_entries() {
        for path in ["/outside", "../outside", "a/../../outside"] {
            let bytes = packed(&[(path, tar::EntryType::Regular, None, b"x")]);
            assert!(matches!(extract_fixture(&bytes), Err(Error::UnsafePath)));
        }
    }
    #[test]
    fn rejects_hardlinks_and_special_entries() {
        let hardlink = packed(&[("escape", tar::EntryType::Link, Some("/etc/passwd"), b"")]);
        assert!(matches!(
            extract_fixture(&hardlink),
            Err(Error::UnsupportedEntry)
        ));
        for kind in [
            tar::EntryType::Fifo,
            tar::EntryType::Char,
            tar::EntryType::Block,
        ] {
            let bytes = packed(&[("special", kind, None, b"")]);
            assert!(matches!(
                extract_fixture(&bytes),
                Err(Error::UnsupportedEntry)
            ));
        }
    }
    #[test]
    fn accepts_internal_symlink_and_rejects_escape_or_loop() {
        let valid = packed(&[
            ("version", tar::EntryType::Regular, None, b"build"),
            (
                "version-link",
                tar::EntryType::Symlink,
                Some("version"),
                b"",
            ),
        ]);
        extract_fixture(&valid).unwrap();
        let escape = packed(&[("link", tar::EntryType::Symlink, Some("../../host"), b"")]);
        assert!(matches!(extract_fixture(&escape), Err(Error::UnsafePath)));
        let cycle = packed(&[
            ("a", tar::EntryType::Symlink, Some("b"), b""),
            ("b", tar::EntryType::Symlink, Some("a"), b""),
        ]);
        assert!(extract_fixture(&cycle).is_err());
        let pivot = packed(&[
            ("pivot", tar::EntryType::Symlink, Some("safe"), b""),
            ("pivot/child", tar::EntryType::Regular, None, b"payload"),
        ]);
        assert!(extract_fixture(&pivot).is_err());
    }
    #[test]
    fn rejects_duplicate_paths_and_corrupt_streams() {
        let duplicate = packed(&[
            ("same", tar::EntryType::Regular, None, b"1"),
            ("same", tar::EntryType::Regular, None, b"2"),
        ]);
        assert!(extract_fixture(&duplicate).is_err());
        assert!(extract_fixture(b"not gzip").is_err());
        let mut truncated = packed(&[("file", tar::EntryType::Regular, None, b"payload")]);
        truncated.truncate(truncated.len() - 5);
        assert!(extract_fixture(&truncated).is_err());
    }
    #[test]
    fn rejects_oversized_header_before_creating_the_file() {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_path("large").unwrap();
        header.set_size(MAX_FILE_BYTES + 1);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, std::io::empty()).unwrap();
        let bytes = builder.into_inner().unwrap().finish().unwrap();
        assert!(matches!(extract_fixture(&bytes), Err(Error::Limit)));
    }
}

#[cfg(test)]
mod candidate_dry_run {
    use super::*;
    #[test]
    fn unknown_or_incomplete_layout_never_falls_back_to_legacy() {
        let directory = tempfile::tempdir().unwrap();
        let root = File::open(directory.path()).unwrap();
        std::fs::write(directory.path().join("endfield-layout-v1.json"), b"{}").unwrap();
        assert!(validate(&root, "wine", "wine-10.0").is_err());
        assert!(validate(&root, "proton", "wine-10.0").is_err());
        let descriptor = serde_json::json!({"schema_version":1,"layout":"winehq-dualarch-v1","build":"wine-10.0","source_inventory_sha256":"0".repeat(64),"source_packages":[{"package":"wine-stable-amd64","sha256":"1".repeat(64)}]});
        std::fs::write(
            directory.path().join("endfield-layout-v1.json"),
            serde_json::to_vec(&descriptor).unwrap(),
        )
        .unwrap();
        assert!(validate(&root, "wine", "wine-10.0").is_err());
        assert!(validate(&root, "wine", "wine-11.0").is_err());
    }
    #[test]
    #[ignore = "requires explicitly acquired derived candidate; no runtime execution"]
    fn acquired_dualarch_package_contract() {
        let archive =
            std::path::PathBuf::from(std::env::var("ENDFIELD_DERIVED_WINE_ARCHIVE").unwrap());
        let destination = tempfile::tempdir().unwrap();
        let artifact_sha256 =
            endfield_unix_runtime_verification::sha256(&std::fs::read(&archive).unwrap());
        let file = File::open(&archive).unwrap();
        let length = file.metadata().unwrap().len();
        let root = File::open(destination.path()).unwrap();
        extract(file, &root, length).unwrap();
        validate(&root, "wine", "wine-10.0").unwrap();
        // No P025 record or installation authority is created by this dry run.
        assert!(
            !destination
                .path()
                .join(".endfield-installation.json")
                .exists()
        );
        let baseline = endfield_unix_runtime_audit::create_baseline(
            destination.path(),
            "candidate-dry-run",
            1,
            "wine",
            "wine-10.0",
            &"0".repeat(64),
            &"0".repeat(64),
            endfield_unix_runtime_audit::TrustClassification::UserPinnedLimitedTrust,
        )
        .unwrap();
        let expected = std::env::var("ENDFIELD_DERIVED_WINE_INVENTORY").unwrap();
        let entries: Vec<endfield_unix_runtime_audit::Entry> =
            serde_json::from_slice(&std::fs::read(expected).unwrap()).unwrap();
        assert_eq!(baseline.entries, entries);
        assert_eq!(
            endfield_unix_runtime_audit::audit(destination.path(), &baseline).completeness,
            endfield_unix_runtime_audit::Completeness::Complete
        );
        if let Ok(report) = std::env::var("ENDFIELD_DERIVED_WINE_DRY_RUN_REPORT") {
            std::fs::write(report,serde_json::to_vec_pretty(&serde_json::json!({"structural_layout":"PASS","artifact_sha256":artifact_sha256,"candidate_generation":1,"p026_inventory_sha256":baseline.inventory_sha256,"entries":baseline.entries.len(),"installation_created":false,"pin_created":false,"authentic_execution":"NOT_RUN","provenance":"user_pinned_limited_trust_observation_only"})).unwrap()).unwrap();
        }
    }
}
