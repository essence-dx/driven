use crate::{DrivenError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const COMMAND_EVIDENCE_CAPTURED_BY: &str = "driven.strategy.command_evidence.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEvidence {
    pub cwd: String,
    pub stdout_digest: String,
    pub stderr_digest: String,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    #[serde(default)]
    pub stdout_truncated: bool,
    #[serde(default)]
    pub stderr_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_limit_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub started_unix_seconds: u64,
    pub finished_unix_seconds: u64,
    pub captured_by: String,
}

impl CommandEvidence {
    pub fn from_streams(
        cwd: &Path,
        stdout: &[u8],
        stderr: &[u8],
        started_unix_seconds: u64,
        finished_unix_seconds: u64,
    ) -> Result<Self> {
        let evidence = Self {
            cwd: normalize_path(cwd.display().to_string()),
            stdout_digest: digest_bytes(stdout),
            stderr_digest: digest_bytes(stderr),
            stdout_bytes: stdout.len() as u64,
            stderr_bytes: stderr.len() as u64,
            stdout_truncated: false,
            stderr_truncated: false,
            output_limit_bytes: None,
            duration_ms: None,
            started_unix_seconds,
            finished_unix_seconds,
            captured_by: COMMAND_EVIDENCE_CAPTURED_BY.to_string(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_captured_streams(
        cwd: &Path,
        stdout_digest: impl Into<String>,
        stderr_digest: impl Into<String>,
        stdout_bytes: u64,
        stderr_bytes: u64,
        stdout_truncated: bool,
        stderr_truncated: bool,
        output_limit_bytes: u64,
        duration_ms: u64,
        started_unix_seconds: u64,
        finished_unix_seconds: u64,
    ) -> Result<Self> {
        let evidence = Self {
            cwd: normalize_path(cwd.display().to_string()),
            stdout_digest: stdout_digest.into(),
            stderr_digest: stderr_digest.into(),
            stdout_bytes,
            stderr_bytes,
            stdout_truncated,
            stderr_truncated,
            output_limit_bytes: Some(output_limit_bytes),
            duration_ms: Some(duration_ms),
            started_unix_seconds,
            finished_unix_seconds,
            captured_by: COMMAND_EVIDENCE_CAPTURED_BY.to_string(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<()> {
        if self.cwd.trim().is_empty() {
            return Err(DrivenError::Validation(
                "command evidence cwd cannot be empty".to_string(),
            ));
        }
        if self.finished_unix_seconds < self.started_unix_seconds {
            return Err(DrivenError::Validation(
                "command evidence finish time cannot precede start time".to_string(),
            ));
        }
        if self.captured_by != COMMAND_EVIDENCE_CAPTURED_BY {
            return Err(DrivenError::Validation(format!(
                "command evidence captured_by must be {}",
                COMMAND_EVIDENCE_CAPTURED_BY
            )));
        }
        if (self.stdout_truncated || self.stderr_truncated) && self.output_limit_bytes.is_none() {
            return Err(DrivenError::Validation(
                "truncated command evidence requires an output limit".to_string(),
            ));
        }
        Ok(())
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn normalize_path(path: String) -> String {
    path.replace('\\', "/")
}
