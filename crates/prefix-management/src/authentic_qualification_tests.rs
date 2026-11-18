//! Native fixture only. The ephemeral signing key exercises the P024 consumer
//! and is scoped to a synthetic-test provider; it is no publisher trust anchor.
use super::*;
use crate::wine_artifact_profile::{CORE, ComponentKind};
use ed25519_dalek::{Signer, SigningKey};
use endfield_unix_runtime_audit::{self as audit, Baseline};
use endfield_unix_runtime_install as install;
use endfield_unix_runtime_verification as verification;
use serde_json::{Value, json};
use std::{
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
};

const NATIVE: &str = r#"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static void write_text(const char *path, const char *text) {
    FILE *file = fopen(path, "w");
    assert(file != NULL);
    assert(fputs(text, file) >= 0);
    assert(fclose(file) == 0);
}

int main(int argc, char **argv) {
    assert(argc == 3);
    assert(strcmp(argv[0], "wine") == 0);
    assert(strcmp(argv[1], "wineboot.exe") == 0);
    assert(strcmp(argv[2], "--init") == 0);
    assert(!getenv("DISPLAY") && !getenv("DBUS_SESSION_BUS_ADDRESS"));
    assert(!getenv("PATH") && !getenv("LD_PRELOAD") && !getenv("WINEDLLPATH"));
    char path[4096];
    const char *variables[] = {
        "HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_RUNTIME_DIR"
    };
    for (unsigned i = 0; i < sizeof(variables) / sizeof(variables[0]); ++i) {
        const char *dir = getenv(variables[i]);
        assert(dir);
        int n = snprintf(path, sizeof(path), "%s/native-fixture", dir);
        assert(n > 0 && (size_t)n < sizeof(path));
        write_text(path, variables[i]);
    }
    if (FIXTURE_MODE == 1) return 17;
    assert(getcwd(path, sizeof(path)));
    assert(strcmp(path, getenv("WINEPREFIX")) == 0);
    assert(mkdir("drive_c", 0700) == 0);
    assert(mkdir("dosdevices", 0700) == 0);
    assert(symlink("../drive_c", "dosdevices/c:") == 0);
    write_text("system.reg", "WINE REGISTRY Version 2\n");
    if (FIXTURE_MODE != 3) write_text("user.reg", "WINE REGISTRY Version 2\n");
    ssize_t len = readlink("/proc/self/exe", path, sizeof(path) - 1);
    assert(len > 0 && (size_t)len < sizeof(path));
    path[len] = 0;
    char *basename = strrchr(path, '/');
    assert(basename && (size_t)(basename - path) + sizeof("/../share/wine/wine.inf") < sizeof(path));
    strcpy(basename, "/../share/wine/wine.inf");
    struct stat inf;
    assert(stat(path, &inf) == 0);
    assert(snprintf(path, sizeof(path), "%lu\n", (unsigned long)inf.st_mtime) > 0);
    write_text(".update-timestamp", path);
    if (FIXTURE_MODE != 2) {
        struct stat prefix;
        assert(stat(".", &prefix) == 0);
        int n = snprintf(path, sizeof(path), "%s/.wine-%u", getenv("TMPDIR"), getuid());
        assert(n > 0 && (size_t)n < sizeof(path));
        assert(mkdir(path, 0700) == 0);
        int m = snprintf(path + n, sizeof(path) - (size_t)n, "/server-%llx-%llx",
                         (unsigned long long)prefix.st_dev, (unsigned long long)prefix.st_ino);
        assert(m > 0 && (size_t)(n + m) < sizeof(path));
        assert(mkdir(path, 0700) == 0);
        assert((size_t)(n + m) + sizeof("/lock") < sizeof(path));
        strcat(path, "/lock");
        umask(0077);
        write_text(path, "");
    }
    puts("native qualification fixture");
    return 0;
}
"#;

struct Fixture {
    temp: tempfile::TempDir,
    request: QualificationRequest,
    installed: PathBuf,
    sidecar: PathBuf,
    profile: Value,
    manifest: Value,
    runtime: pins::ManagedRuntimeReference,
}

impl Fixture {
    fn new(mode: u32) -> Self {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let workspace = temp.path();
        let source_paths = AppPaths::from_base_dirs(
            workspace.join("source-config"),
            workspace.join("source-data"),
            workspace.join("source-cache"),
        )
        .unwrap();
        private_create(source_paths.data_dir().parent().unwrap()).unwrap();
        let staging = workspace.join("staging");
        private_create(&staging).unwrap();
        let native = workspace.join("native.c");
        let module = workspace.join("module.c");
        let executable = workspace.join("launcher");
        let shared = workspace.join("module.so");
        fs::write(&native, NATIVE).unwrap();
        fs::write(&module, b"int fixture_module(void) { return 0; }\n").unwrap();
        assert!(
            Command::new("cc")
                .arg(format!("-DFIXTURE_MODE={mode}"))
                .arg(&native)
                .arg("-o")
                .arg(&executable)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("cc")
                .args(["-shared", "-fPIC"])
                .arg(&module)
                .arg("-o")
                .arg(&shared)
                .status()
                .unwrap()
                .success()
        );
        let interpreter =
            endfield_unix_game_launch::elf_dependencies::inspect(&fs::read(&executable).unwrap())
                .unwrap()
                .interpreter
                .unwrap();
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut components = Vec::new();
        for &(role, path, kind) in CORE {
            let path = if path.starts_with("x86_64-") || path.starts_with("i386-") {
                format!("lib/wine/{path}")
            } else {
                path.into()
            };
            let mut dir = staging.clone();
            let parent = Path::new(&path).parent().unwrap();
            for part in parent.components() {
                dir.push(part);
                if !dir.exists() {
                    private_create(&dir).unwrap();
                }
            }
            let (bytes, permissions) = match kind {
                ComponentKind::NativeExecutable => (fs::read(&executable).unwrap(), 0o700),
                ComponentKind::UnixModule => (fs::read(&shared).unwrap(), 0o600),
                _ => (b"synthetic role payload".to_vec(), 0o600),
            };
            fs::write(staging.join(&path), &bytes).unwrap();
            fs::set_permissions(staging.join(&path), fs::Permissions::from_mode(permissions))
                .unwrap();
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(permissions);
            header.set_cksum();
            tar.append_data(&mut header, &path, bytes.as_slice())
                .unwrap();
            components
                .push(json!({"role": role, "path": path, "kind": kind, "usage": "initialization"}));
        }
        let version = b"wine-10.0\n";
        fs::write(staging.join("version"), version).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(version.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        tar.append_data(&mut header, "version", version.as_slice())
            .unwrap();
        let archive = tar.into_inner().unwrap().finish().unwrap();
        let artifact_sha256 = verification::sha256(&archive);
        let archive_path = workspace.join("runtime.tar.gz");
        fs::write(&archive_path, &archive).unwrap();
        let baseline = audit::create_baseline(
            &staging,
            "wine-synthetic",
            1,
            "wine",
            "wine-10.0",
            &artifact_sha256,
            &"a".repeat(64),
            audit::TrustClassification::IndependentlyAuthenticatedPublisher,
        )
        .unwrap();
        let profile = json!({
            "schema_version": 1, "build": "wine-10.0", "source_ref": "wine-10.0",
            "source_commit": "b073859675060c9211fcbccfd90e4e87520dc2c2",
            "architecture": "x86_64", "build_configuration": "linux-x86_64-wow64-no-preloader-headless",
            "invocation": "wineboot.exe --init", "configure_prefix": "/opt/endfield-qualification-fixture",
            "libdir": "lib", "interpreter": interpreter, "host_libraries": [],
            "inventory_sha256": baseline.inventory_sha256, "components": components,
        });
        let profile_bytes = serde_json::to_vec(&profile).unwrap();
        let manifest = json!({
            "schema_version": 2, "manifest_id": "synthetic-qualification", "manifest_version": 1,
            "provider_id": "synthetic-test", "project_id": "synthetic-test", "channel": "test",
            "runtime_family":"wine", "runtime_lineage":"upstream-wine", "runtime_build":"wine-10.0",
            "key_id":"synthetic-test", "signature_algorithm":"ed25519",
            "issued_at":"1970-01-01T00:00:00Z", "expires_at":"2099-01-01T00:00:00Z",
            "artifacts":[{"artifact_id":"fixture", "filename":"runtime.tar.gz", "size_bytes":archive.len(),
            "sha256":artifact_sha256, "wine_qualification_profile_sha256":verification::sha256(&profile_bytes)}]
        });
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let key = SigningKey::from_bytes(&[73; 32]);
        let anchor = verification::TrustAnchor {
            key_id: "synthetic-test".into(),
            public_key: key.verifying_key().to_bytes(),
            scope: verification::Scope {
                provider: "synthetic-test".into(),
                project: "synthetic-test".into(),
                channel: "test".into(),
                family: "wine".into(),
                lineage: "upstream-wine".into(),
            },
            provenance: verification::Provenance::IndependentlyAuthenticatedPublisher,
            revoked: false,
            valid_from: None,
            valid_until: None,
        };
        let checkpoint = verification::TrustedCheckpoint {
            scope: anchor.scope.clone(),
            manifest_id: "synthetic-qualification".into(),
            version: 1,
            content_sha256: verification::sha256(&manifest_bytes),
        };
        let verified = verification::verify_manifest(
            &manifest_bytes,
            &key.sign(&verification::signing_input(&manifest_bytes))
                .to_bytes(),
            &[anchor],
            Some(crate::now() as i64),
            Some(&checkpoint),
        )
        .unwrap();
        let installation =
            install::RuntimeInstallationPlan::new(&source_paths, &verified, "fixture", 1).unwrap();
        install::install(
            &source_paths,
            &installation,
            installation.approve(&installation.identity()).unwrap(),
            &verified,
            &archive_path,
        )
        .unwrap();
        let installed = installation.destination().to_owned();
        let installed_identity =
            install::pin_identity(&source_paths, installation.installation_id()).unwrap();
        let runtime = pins::ManagedRuntimeReference {
            installation_id: installed_identity.installation_id,
            provider: installed_identity.provider,
            project: installed_identity.project,
            family: installed_identity.family,
            lineage: installed_identity.lineage,
            build: installed_identity.build,
            generation: installed_identity.generation,
            artifact_sha256: installed_identity.artifact_sha256,
            manifest_sha256: installed_identity.manifest_sha256,
            ownership_record_sha256: installed_identity.ownership_record_sha256,
            trust: pins::Provenance::IndependentlyAuthenticatedPublisher,
        };
        let sidecar = source_paths
            .data_dir()
            .join("wine-qualification")
            .join(&runtime.installation_id);
        fs::create_dir_all(&sidecar).unwrap();
        fs::write(sidecar.join("manifest.json"), manifest_bytes).unwrap();
        fs::write(sidecar.join("profile.json"), profile_bytes).unwrap();
        let pin = pins::RuntimePin::new(
            "synthetic-test",
            pins::RuntimePinScope::Machine {
                machine: pins::MachineId::from_digest("c".repeat(64)).unwrap(),
            },
            PinnedRuntimeReference::Managed(runtime.clone()),
            Some(pins::AdapterPreference::ManagedWine),
            1,
        )
        .unwrap();
        pins::PinStore::for_app(&source_paths)
            .set(pin, 0, false)
            .unwrap();
        let request = QualificationRequest {
            source_paths,
            pin_id: "synthetic-test".into(),
            pin_revision: 1,
            artifact_sha256,
            test_root: workspace.join("qualification"),
        };
        Self {
            temp,
            request,
            installed,
            sidecar,
            profile,
            manifest,
            runtime,
        }
    }

    fn evidence(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.request.test_root.join(name)).unwrap()).unwrap()
    }

    fn profile_result(
        &self,
        profile: &Value,
        baseline: &Baseline,
    ) -> Result<VerifiedProfile, ArtifactProfileBlocker> {
        let bytes = serde_json::to_vec(profile).unwrap();
        let mut manifest = self.manifest.clone();
        manifest["artifacts"][0]["wine_qualification_profile_sha256"] =
            verification::sha256(&bytes).into();
        let manifest = serde_json::to_vec(&manifest).unwrap();
        let mut runtime = self.runtime.clone();
        runtime.manifest_sha256 = verification::sha256(&manifest);
        let mut baseline = baseline.clone();
        baseline.manifest_sha256 = runtime.manifest_sha256.clone();
        VerifiedProfile::validate(&manifest, &bytes, &runtime, &baseline)
    }
}

#[test]
fn exact_two_stage_review_reaches_native_execution_and_keeps_source_read_only() {
    let fixture = Fixture::new(0);
    let sentinel = fixture.temp.path().join("unrelated-data");
    fs::write(&sentinel, b"unrelated").unwrap();
    let pins = pins::PinStore::for_app(&fixture.request.source_paths);
    let original = serde_json::to_vec(&pins.load_read_only().unwrap()).unwrap();
    let plan = fixture.request.prepare().unwrap();
    assert_eq!(
        fixture.evidence("evidence/preflight.json")["actual_wine_execution"],
        "NOT_RUN"
    );
    assert!(matches!(
        fixture.request.execute("", &plan),
        Err(QualificationBlocker::ApprovalRequired)
    ));
    assert_eq!(
        fixture.request.recorded_status(),
        RecordedQualificationStatus::NoAttempt
    );
    let wrong = format!("sha256:{}", "0".repeat(64));
    assert!(matches!(
        fixture.request.execute(ACCEPT, &wrong),
        Err(QualificationBlocker::PlanChanged)
    ));
    assert!(
        !fixture
            .request
            .test_root
            .join("records/attempt.json")
            .exists()
    );
    assert!(
        !fixture
            .request
            .test_root
            .join("home/native-fixture")
            .exists()
    );
    assert_eq!(
        fixture.request.execute(ACCEPT, &plan).unwrap(),
        QualificationResult::Qualified
    );
    assert_eq!(
        fixture.evidence("evidence/result.json")["helper_completion"],
        "OBSERVED"
    );
    assert_eq!(
        fixture.evidence("evidence/result.json")["process_outcome"]["exit_code"],
        0
    );
    assert_eq!(
        fixture.evidence("records/attempt.json")["approval_id"],
        plan
    );
    assert!(matches!(
        fixture.request.execute(ACCEPT, &plan),
        Err(QualificationBlocker::AttemptExists)
    ));
    assert_eq!(
        serde_json::to_vec(&pins.load_read_only().unwrap()).unwrap(),
        original
    );
    assert_eq!(
        fixture.request.recorded_status(),
        RecordedQualificationStatus::OutcomeRecorded
    );
    assert_eq!(fs::read(sentinel).unwrap(), b"unrelated");
    assert!(!fixture.request.source_paths.config_file().exists());
    assert!(
        fixture
            .request
            .test_root
            .join("runtime/native-fixture")
            .is_file()
    );
}

#[test]
fn failed_or_unverified_native_outcomes_remain_ineligible() {
    for (mode, expected) in [
        (1, QualificationResult::ExecutionFailed),
        (2, QualificationResult::Inconclusive),
        (3, QualificationResult::VerificationFailed),
    ] {
        let fixture = Fixture::new(mode);
        let plan = fixture.request.prepare().unwrap();
        assert_eq!(fixture.request.execute(ACCEPT, &plan).unwrap(), expected);
        let evidence = fixture.evidence("evidence/result.json");
        assert_eq!(
            evidence["helper_completion"],
            if mode == 3 {
                "OBSERVED"
            } else {
                "QUIESCENCE_UNVERIFIED"
            }
        );
        assert!(evidence["prefix_verification"].is_null());
    }
}

#[test]
fn partial_attempt_marker_blocks_replay_even_without_final_result() {
    let fixture = Fixture::new(0);
    let plan = fixture.request.prepare().unwrap();
    write_new(
        &fixture.request.test_root.join("records/attempt.json"),
        b"{",
    )
    .unwrap();
    assert_eq!(
        fixture.request.recorded_status(),
        RecordedQualificationStatus::Inconclusive
    );
    assert!(matches!(
        fixture.request.execute(ACCEPT, &plan),
        Err(QualificationBlocker::AttemptExists)
    ));
    assert!(
        !fixture
            .request
            .test_root
            .join("home/native-fixture")
            .exists()
    );
}

#[test]
fn changed_source_or_disposable_root_cannot_reuse_review() {
    let fixture = Fixture::new(0);
    let plan = fixture.request.prepare().unwrap();
    let mut request = fixture.request.clone();
    request.artifact_sha256 = "0".repeat(64);
    assert!(matches!(
        request.execute(ACCEPT, &plan),
        Err(QualificationBlocker::PinMismatch)
    ));
    request = fixture.request.clone();
    request.pin_revision += 1;
    assert!(matches!(
        request.execute(ACCEPT, &plan),
        Err(QualificationBlocker::PinMismatch)
    ));
    let home = fixture.request.test_root.join("home");
    fs::rename(&home, fixture.temp.path().join("old-home")).unwrap();
    private_create(&home).unwrap();
    assert!(matches!(
        fixture.request.execute(ACCEPT, &plan),
        Err(QualificationBlocker::PlanChanged)
    ));
    fs::write(fixture.installed.join("share/wine/wine.inf"), b"replaced").unwrap();
    assert!(matches!(
        fixture.request.execute(ACCEPT, &plan),
        Err(QualificationBlocker::Runtime(_))
    ));
}

#[test]
fn private_environment_refuses_existing_paths_links_permissions_and_traversal() {
    let fixture = Fixture::new(0);
    let mut request = fixture.request.clone();
    IsolatedEnvironment::create(&request).unwrap();
    assert!(IsolatedEnvironment::create(&request).is_err());
    fs::set_permissions(&request.test_root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        IsolatedEnvironment::open(&request),
        Err(QualificationBlocker::UnsafeEnvironment)
    ));
    request.test_root = fixture.temp.path().join("../outside");
    assert!(matches!(
        IsolatedEnvironment::create(&request),
        Err(QualificationBlocker::UnsafeEnvironment)
    ));
    let link = fixture.temp.path().join("link");
    symlink(&fixture.request.test_root, &link).unwrap();
    request.test_root = link.join("side-effect");
    assert!(matches!(
        IsolatedEnvironment::create(&request),
        Err(QualificationBlocker::UnsafeEnvironment)
    ));
}

#[test]
fn role_map_schema_and_stale_baselines_fail_closed() {
    use ArtifactProfileBlocker as Error;
    let fixture = Fixture::new(0);
    let mut baseline = audit::load_baseline(&fixture.installed).unwrap();
    assert!(fixture.profile_result(&fixture.profile, &baseline).is_ok());
    let mut profile = fixture.profile.clone();
    profile["components"].as_array_mut().unwrap().pop();
    assert!(matches!(
        fixture.profile_result(&profile, &baseline),
        Err(Error::MissingRole)
    ));
    let mut profile = fixture.profile.clone();
    profile["components"][1]["role"] = profile["components"][0]["role"].clone();
    assert!(matches!(
        fixture.profile_result(&profile, &baseline),
        Err(Error::AmbiguousRole)
    ));
    profile["components"][1]["role"] = "wineserver".into();
    profile["components"][1]["path"] = "../wineserver".into();
    assert!(matches!(
        fixture.profile_result(&profile, &baseline),
        Err(Error::UnsafePath)
    ));
    for (field, value, expected) in [
        ("architecture", "aarch64", Error::WrongArchitecture),
        (
            "build_configuration",
            "distro-default",
            Error::UnsupportedBuild,
        ),
        ("inventory_sha256", &"0".repeat(64), Error::BaselineMismatch),
    ] {
        let mut profile = fixture.profile.clone();
        profile[field] = value.into();
        assert_eq!(
            fixture.profile_result(&profile, &baseline).err(),
            Some(expected)
        );
    }
    baseline.generation += 1;
    assert!(matches!(
        fixture.profile_result(&fixture.profile, &baseline),
        Err(Error::BaselineMismatch)
    ));
}

#[test]
fn profile_provenance_and_helper_paths_must_match_the_verified_bytes() {
    let fixture = Fixture::new(0);
    let baseline = audit::load_baseline(&fixture.installed).unwrap();
    let root = trusted_directory(&fixture.installed).unwrap();
    let profile = fixture.sidecar.join("profile.json");
    let original = fs::read(&profile).unwrap();
    fs::write(&profile, b"{}").unwrap();
    let load = || {
        VerifiedProfile::load(
            &fixture.request.source_paths,
            &fixture.runtime,
            &root,
            &baseline,
        )
    };
    assert!(matches!(
        load(),
        Err(ArtifactProfileBlocker::ProvenanceMismatch)
    ));
    fs::write(profile, original).unwrap();
    let helper = fixture.installed.join("bin/wineserver");
    fs::remove_file(&helper).unwrap();
    symlink("wine", &helper).unwrap();
    assert!(matches!(
        load(),
        Err(ArtifactProfileBlocker::ComponentIntegrity(_))
    ));
}

#[test]
fn host_policy_reports_missing_declared_library_and_interpreter() {
    let fixture = Fixture::new(0);
    let baseline = audit::load_baseline(&fixture.installed).unwrap();
    let root = trusted_directory(&fixture.installed).unwrap();
    let mut profile = fixture
        .profile_result(&fixture.profile, &baseline)
        .unwrap()
        .profile;
    let mut evidence = wine_host_abi::inspect(&root, &baseline, &profile);
    assert_eq!(evidence.state, HostAbiState::SupportedUnderHostAssumptions);
    assert!(evidence.authentic_loading_observation_required);
    profile
        .host_libraries
        .push("libendfield-test-missing.so.0".into());
    evidence = wine_host_abi::inspect(&root, &baseline, &profile);
    assert_eq!(evidence.state, HostAbiState::MissingRequiredDependency);
    profile.interpreter = "/nonexistent/ld-linux-x86-64.so.2".into();
    evidence = wine_host_abi::inspect(&root, &baseline, &profile);
    assert_eq!(evidence.state, HostAbiState::MissingRequiredDependency);
    assert_eq!(evidence.object.as_deref(), Some("interpreter"));
}

#[test]
fn visible_prefix_use_blocks_approved_attempt_before_consumption() {
    let fixture = Fixture::new(0);
    let plan = fixture.request.prepare().unwrap();
    let environment = IsolatedEnvironment::open(&fixture.request).unwrap();
    let prefix = environment
        .store()
        .target_path(&PrefixId::new(PREFIX).unwrap());
    let process = fixture
        .request
        .test_root
        .join("records/process-table-fixture/1729");
    private_create(&process).unwrap();
    fs::write(
        process.join("environ"),
        format!("WINEPREFIX={}\0", prefix.display()),
    )
    .unwrap();
    assert!(matches!(
        fixture.request.execute(ACCEPT, &plan),
        Err(QualificationBlocker::PrefixPlanBlocked(blockers))
            if blockers.contains(&crate::PrefixInitializationBlocker::PrefixActive)
    ));
    assert!(
        !fixture
            .request
            .test_root
            .join("records/attempt.json")
            .exists()
    );
}
