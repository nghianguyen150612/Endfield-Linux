use std::path::{Path, PathBuf};

use endfield_unix_platform_linux::{
    ContainerMarker, DataSourceError, DataSourceErrorKind, LinuxDataSource, OsReleaseFile,
    RawKernelIdentity, RuntimeDirectoryMetadata,
};

#[derive(Clone)]
pub struct FixtureSource {
    pub etc_os_release: Option<Vec<u8>>,
    pub usr_lib_os_release: Option<Vec<u8>>,
    pub etc_error: Option<DataSourceErrorKind>,
    pub usr_lib_error: Option<DataSourceErrorKind>,
    pub kernel: RawKernelIdentity,
    pub kernel_error: Option<DataSourceErrorKind>,
    pub markers: [Result<bool, DataSourceErrorKind>; 3],
    pub runtime_path: Option<PathBuf>,
    pub runtime_path_error: Option<DataSourceErrorKind>,
    pub runtime_metadata: Option<RuntimeDirectoryMetadata>,
    pub runtime_metadata_error: Option<DataSourceErrorKind>,
    pub uid: u32,
    pub observed_at: Option<u64>,
}

impl Default for FixtureSource {
    fn default() -> Self {
        Self {
            etc_os_release: Some(
                b"ID=arch\nNAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nVERSION_ID=rolling\n"
                    .to_vec(),
            ),
            usr_lib_os_release: None,
            etc_error: None,
            usr_lib_error: None,
            kernel: RawKernelIdentity {
                system_name: "Linux".to_owned(),
                release: "6.12.0-41-generic".to_owned(),
                machine: "x86_64".to_owned(),
            },
            kernel_error: None,
            markers: [Ok(false), Ok(false), Ok(false)],
            runtime_path: Some(PathBuf::from("/run/user/1000")),
            runtime_path_error: None,
            runtime_metadata: Some(RuntimeDirectoryMetadata {
                is_directory: true,
                is_symlink: false,
                owner_uid: 1000,
                mode: 0o700,
            }),
            runtime_metadata_error: None,
            uid: 1000,
            observed_at: Some(1_761_286_800_000_000_000),
        }
    }
}

impl FixtureSource {
    #[allow(dead_code)]
    pub fn missing_os_release() -> Self {
        Self {
            etc_os_release: None,
            usr_lib_os_release: None,
            ..Self::default()
        }
    }

    fn error(kind: DataSourceErrorKind) -> DataSourceError {
        DataSourceError::new(kind)
    }
}

impl LinuxDataSource for FixtureSource {
    fn read_os_release(&self, file: OsReleaseFile) -> Result<Vec<u8>, DataSourceError> {
        let (bytes, error) = match file {
            OsReleaseFile::Etc => (&self.etc_os_release, self.etc_error),
            OsReleaseFile::UsrLib => (&self.usr_lib_os_release, self.usr_lib_error),
        };
        if let Some(error) = error {
            return Err(Self::error(error));
        }
        bytes
            .clone()
            .ok_or_else(|| Self::error(DataSourceErrorKind::NotFound))
    }

    fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError> {
        if let Some(error) = self.kernel_error {
            return Err(Self::error(error));
        }
        Ok(self.kernel.clone())
    }

    fn container_marker_present(&self, marker: ContainerMarker) -> Result<bool, DataSourceError> {
        let result = match marker {
            ContainerMarker::Docker => &self.markers[0],
            ContainerMarker::Podman => &self.markers[1],
            ContainerMarker::Systemd => &self.markers[2],
        };
        result.map_err(Self::error)
    }

    fn runtime_directory_path(&self) -> Result<Option<PathBuf>, DataSourceError> {
        if let Some(error) = self.runtime_path_error {
            return Err(Self::error(error));
        }
        Ok(self.runtime_path.clone())
    }

    fn runtime_directory_metadata(
        &self,
        _path: &Path,
    ) -> Result<RuntimeDirectoryMetadata, DataSourceError> {
        if let Some(error) = self.runtime_metadata_error {
            return Err(Self::error(error));
        }
        self.runtime_metadata
            .ok_or_else(|| Self::error(DataSourceErrorKind::NotFound))
    }

    fn effective_uid(&self) -> u32 {
        self.uid
    }

    fn observation_time_unix_nanos(&self) -> Option<u64> {
        self.observed_at
    }
}
