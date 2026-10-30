//! Explicit local artifact planning and application. CLI key files are user pins;
//! no local file can assert independently authenticated publisher status.
use endfield_unix_runtime_install::{RuntimeInstallationPlan, TrustClassification, install};
use endfield_unix_runtime_verification as verification;
use endfield_unix_storage::AppPaths;
use std::{
    fs::OpenOptions,
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};

#[derive(Debug, Clone, PartialEq, Eq, clap::Subcommand)]
pub enum Operation {
    /// Verify an explicit local artifact and write a private deterministic plan.
    Plan {
        #[command(flatten)]
        inputs: super::runtime_integrity::Options,
        #[arg(long, value_parser = super::parser::absolute_path)]
        plan_out: PathBuf,
    },
    /// Reverify all inputs and apply only the exact manually approved plan.
    Apply {
        #[command(flatten)]
        inputs: super::runtime_integrity::Options,
        #[arg(long, value_parser = super::parser::absolute_path)]
        plan: PathBuf,
        /// Plan SHA-256 shown by `plan`.
        #[arg(long)]
        approve_plan: String,
        /// Exact artifact SHA-256, explicitly accepting local key pinning risk.
        #[arg(long)]
        accept_user_pinned_sha256: String,
    },
    /// Inspect actual managed installation records and incomplete transactions.
    Status,
}

struct InputsVerified {
    evidence: verification::VerifiedManifest,
}
fn verify_inputs(o: &super::runtime_integrity::Options) -> Result<InputsVerified, String> {
    let bytes = verification::read_bounded(&o.manifest, verification::MAX_MANIFEST_BYTES)
        .map_err(|e| format!("manifest verification failed: {e}"))?;
    let signature = verification::read_bounded(&o.signature, 64)
        .map_err(|e| format!("signature verification failed: {e}"))?;
    let key: [u8; 32] = verification::read_bounded(&o.trust_key, 32)
        .map_err(|e| format!("pinned key unavailable: {e}"))?
        .try_into()
        .map_err(|_| "pinned key must contain exactly 32 bytes".to_owned())?;
    let anchor = verification::TrustAnchor {
        key_id: o.key_id.clone(),
        public_key: key,
        scope: verification::Scope {
            provider: o.provider.clone(),
            project: o.project.clone(),
            channel: o.channel.clone(),
            family: o.family.clone(),
            lineage: o.lineage.clone(),
        },
        provenance: verification::Provenance::UserPinned,
        revoked: false,
        valid_from: None,
        valid_until: None,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|t| i64::try_from(t.as_secs()).ok());
    let evidence = verification::verify_manifest(&bytes, &signature, &[anchor], now, None)
        .map_err(|e| format!("manifest verification failed: {e}"))?;
    let filename = o
        .artifact
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "artifact path has no valid filename".to_owned())?;
    verification::verify_artifact(verification::ArtifactInput {
        artifact_id: &o.artifact_id,
        filename,
        build: &o.build,
        file: verification::open_artifact(&o.artifact)
            .map_err(|e| format!("artifact unavailable: {e}"))?,
        metadata: &evidence,
    })
    .map_err(|e| format!("artifact verification failed: {e}"))?;
    Ok(InputsVerified { evidence })
}

fn write_plan(path: &std::path::Path, plan: &RuntimeInstallationPlan) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(plan).map_err(io::Error::other)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn read_plan(path: &std::path::Path) -> Result<RuntimeInstallationPlan, String> {
    let file = verification::open_artifact(path).map_err(|e| format!("plan unavailable: {e}"))?;
    let metadata = file.metadata().map_err(|_| "plan metadata unavailable")?;
    if metadata.len() > 64 * 1024 {
        return Err("plan exceeds 64 KiB".into());
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "plan read failed")?;
    serde_json::from_slice(&bytes)
        .map_err(|_| "plan schema invalid or contains unknown fields".into())
}

fn write_line(out: &mut impl Write, args: std::fmt::Arguments<'_>) -> Result<(), String> {
    out.write_fmt(args)
        .and_then(|()| out.write_all(b"\n"))
        .map_err(|_| "output failed".to_owned())
}

pub fn run(operation: Operation, out: &mut impl Write) -> io::Result<u8> {
    let outcome = match operation {
        Operation::Status => {
            let paths = match AppPaths::discover() {
                Ok(p) => p,
                Err(e) => {
                    writeln!(out, "Managed runtime status: {e}")?;
                    return Ok(9);
                }
            };
            match endfield_unix_runtime_install::status(&paths) {
                Ok(s) => {
                    writeln!(
                        out,
                        "Recorded installations: {}\nIncomplete staging: {}\nValidated staging needing fresh approval: {}\nAmbiguous entries: {}",
                        s.recorded_installations,
                        s.incomplete_staging,
                        s.validated_staging_requiring_approval,
                        s.ambiguous_entries
                    )?;
                    return Ok(0);
                }
                Err(e) => Err(format!("managed runtime status unavailable: {e}")),
            }
        }
        Operation::Plan { inputs, plan_out } => (|| {
            let verified = verify_inputs(&inputs)?;
            let paths = AppPaths::discover()
                .map_err(|_| "XDG user data directory cannot be resolved".to_owned())?;
            let plan =
                RuntimeInstallationPlan::new(&paths, &verified.evidence, &inputs.artifact_id, 1)
                    .map_err(|e| format!("installation is ineligible: {e}"))?;
            write_plan(&plan_out, &plan).map_err(|e| format!("could not create plan: {e}"))?;
            write_line(out, format_args!("Plan: {}", plan.identity()))?;
            write_line(
                out,
                format_args!("Runtime: {} {}", inputs.family, inputs.build),
            )?;
            write_line(
                out,
                format_args!("Artifact SHA-256: {}", plan.artifact_sha256()),
            )?;
            write_line(
                out,
                format_args!(
                    "Pinned key: {} ({})",
                    plan.key_id(),
                    plan.trust_key_sha256()
                ),
            )?;
            write_line(
                out,
                format_args!("Trust: user-pinned (limited; not publisher-authenticated)"),
            )?;
            write_line(
                out,
                format_args!(
                    "Destination: runtimes/{}/{} (under the P006-managed XDG data directory)",
                    inputs.family,
                    plan.installation_id()
                ),
            )?;
            write_line(
                out,
                format_args!(
                    "P021 policy: new managed location; explicit install approval required"
                ),
            )?;
            write_line(
                out,
                format_args!("Plan written with mode 0600. Installation performed: no"),
            )?;
            write_line(
                out,
                format_args!(
                    "Apply with --approve-plan {} --accept-user-pinned-sha256 {}",
                    plan.identity(),
                    plan.artifact_sha256()
                ),
            )?;
            Ok(())
        })(),
        Operation::Apply {
            inputs,
            plan: plan_path,
            approve_plan,
            accept_user_pinned_sha256,
        } => (|| {
            let serialized_plan = read_plan(&plan_path)?;
            let verified = verify_inputs(&inputs)?;
            let paths = AppPaths::discover()
                .map_err(|_| "XDG user data directory cannot be resolved".to_owned())?;
            let rebuilt = RuntimeInstallationPlan::new(
                &paths,
                &verified.evidence,
                &inputs.artifact_id,
                serialized_plan.generation(),
            )
            .map_err(|e| format!("installation is ineligible: {e}"))?;
            if serialized_plan.identity() != rebuilt.identity() {
                return Err(
                    "plan does not match freshly verified manifest, artifact or destination".into(),
                );
            }
            if rebuilt.trust_classification() != TrustClassification::UserPinnedLimitedTrust {
                return Err(
                    "CLI key input is user-pinned and cannot assert publisher authentication"
                        .into(),
                );
            }
            let approval = rebuilt
                .approve_user_pinned(&approve_plan, &accept_user_pinned_sha256)
                .map_err(|_| {
                    "approval must match both exact plan SHA-256 and artifact SHA-256".to_owned()
                })?;
            let record = install(
                &paths,
                &rebuilt,
                approval,
                &verified.evidence,
                &inputs.artifact,
            )
            .map_err(|e| format!("installation failed: {e}"))?;
            write_line(out, format_args!("Installation: published"))?;
            write_line(
                out,
                format_args!("Installation ID: {}", rebuilt.installation_id()),
            )?;
            write_line(
                out,
                format_args!("Trust: user-pinned (limited; not publisher-authenticated)"),
            )?;
            write_line(
                out,
                format_args!("Runtime execution: not performed\nGame execution: not performed"),
            )?;
            let _ = record;
            Ok(())
        })(),
    };
    match outcome {
        Ok(()) => Ok(0),
        Err(reason) => {
            writeln!(
                out,
                "Runtime installation: {reason}\nInstallation performed: no"
            )?;
            Ok(9)
        }
    }
}

pub fn audit_source(
    id: &str,
    options: &super::runtime_integrity::Options,
) -> Result<endfield_unix_runtime_audit::AuditReport, String> {
    let verified = verify_inputs(options)?.evidence;
    let paths = AppPaths::discover().map_err(|e| e.to_string())?;
    endfield_unix_runtime_install::audit_against_verified_artifact(
        &paths,
        id,
        &verified,
        &options.artifact,
    )
    .map_err(|e| e.to_string())
}
