use crate::{DrivenError, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::fs;
use std::path::{Path, PathBuf};

use super::{NextPassHandoff, ProofReceipt};

pub(crate) fn contained_state_dir(path: &Path) -> Result<PathBuf> {
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(DrivenError::Validation(
            "state dir cannot contain parent traversal".to_string(),
        ));
    }
    Ok(path.to_path_buf())
}

pub(crate) fn receipt_path(state_dir: &Path, receipt_id: &str) -> Result<PathBuf> {
    let receipt_file = format!("{}.json", safe_artifact_component(receipt_id)?);
    Ok(contained_state_dir(state_dir)?
        .join("receipts")
        .join(receipt_file))
}

pub(crate) fn handoff_path(state_dir: &Path, handoff_id: &str) -> Result<PathBuf> {
    let handoff_file = format!("{}.json", safe_artifact_component(handoff_id)?);
    Ok(contained_state_dir(state_dir)?
        .join("handoffs")
        .join(handoff_file))
}

pub(crate) fn ensure_artifact_path_in_state(
    state_dir: &Path,
    path: &Path,
    label: &str,
) -> Result<()> {
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(DrivenError::Validation(format!(
            "{} path cannot contain parent traversal",
            label
        )));
    }
    let state_dir = contained_state_dir(state_dir)?;
    if !path.starts_with(&state_dir) {
        return Err(DrivenError::Validation(format!(
            "{} path must stay inside the lane state directory",
            label
        )));
    }
    Ok(())
}

pub(crate) fn read_receipt_artifact(path: &Path) -> Result<ProofReceipt> {
    read_json_file(path, "proof receipt")
}

pub(crate) fn write_receipt_artifact(path: &Path, receipt: &ProofReceipt) -> Result<()> {
    write_json_atomic(path, receipt, "receipt JSON")
}

pub(crate) fn read_handoff_artifact(path: &Path) -> Result<NextPassHandoff> {
    read_json_file(path, "next-pass handoff")
}

pub(crate) fn write_handoff_artifact(path: &Path, handoff: &NextPassHandoff) -> Result<()> {
    handoff.validate()?;
    let mut persisted = handoff.redacted_for_persistence();
    persisted.seal_payload_digest()?;
    persisted.validate_persisted()?;
    write_json_atomic(path, &persisted, "handoff JSON")
}

pub(crate) fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let content = fs::read_to_string(path).map_err(DrivenError::Io)?;
    serde_json::from_str(&content)
        .map_err(|e| DrivenError::Parse(format!("failed to parse {}: {}", label, e)))
}

pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(DrivenError::Io)?;
    }
    let content = serde_json::to_string_pretty(value)
        .map(|mut rendered| {
            rendered.push('\n');
            rendered
        })
        .map_err(|e| DrivenError::Format(format!("failed to render {}: {}", label, e)))?;
    write_file_atomic(path, content.as_bytes())
}

pub(crate) fn write_file_atomic(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(DrivenError::Io)?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| DrivenError::Validation("state file path is invalid".to_string()))?;
    let temp_path = path.with_file_name(format!(".{}.{}.tmp", file_name, std::process::id()));
    fs::write(&temp_path, content).map_err(DrivenError::Io)?;
    if cfg!(windows) && path.exists() {
        fs::remove_file(path).map_err(DrivenError::Io)?;
    }
    fs::rename(&temp_path, path).map_err(DrivenError::Io)
}

fn safe_artifact_component(raw: &str) -> Result<String> {
    let value = raw.trim();
    if value.is_empty() || value == "." || value == ".." {
        return Err(DrivenError::Validation(
            "artifact id cannot be empty or relative".to_string(),
        ));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(DrivenError::Validation(
            "artifact id contains an unsafe path character".to_string(),
        ));
    }
    Ok(value.to_string())
}
