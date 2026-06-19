use crate::{DrivenError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use super::artifacts::{contained_state_dir, read_json_file, write_json_atomic};
use super::{LaneClaim, StateSessionId};

const STATE_SESSION_FILE: &str = "state-session.json";
const STATE_SESSION_SCHEMA: &str = "driven.lane_pass.state_session.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LanePassSessionManifest {
    pub schema: String,
    pub state_session_id: StateSessionId,
    pub scope: String,
}

impl LanePassSessionManifest {
    fn new(scope: &str) -> Self {
        Self {
            schema: STATE_SESSION_SCHEMA.to_string(),
            state_session_id: StateSessionId::generate(),
            scope: scope.trim().to_string(),
        }
    }

    fn validate(&self, scope: &str) -> Result<()> {
        if self.schema != STATE_SESSION_SCHEMA {
            return Err(DrivenError::Validation(format!(
                "state session schema must be {}",
                STATE_SESSION_SCHEMA
            )));
        }
        if StateSessionId::new(self.state_session_id.as_str())? != self.state_session_id {
            return Err(DrivenError::Validation(
                "state session id must be normalized".to_string(),
            ));
        }
        if self.scope.trim().is_empty() {
            return Err(DrivenError::Validation(
                "state session scope cannot be empty".to_string(),
            ));
        }
        if self.scope != scope.trim() {
            return Err(DrivenError::Validation(
                "state session scope does not match store scope".to_string(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn read_or_create_session(
    state_dir: &Path,
    scope: &str,
) -> Result<LanePassSessionManifest> {
    let path = session_path(state_dir)?;
    if path.exists() {
        let manifest: LanePassSessionManifest = read_json_file(&path, "state session")?;
        manifest.validate(scope)?;
        return Ok(manifest);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(DrivenError::Io)?;
    }
    let manifest = LanePassSessionManifest::new(scope);
    manifest.validate(scope)?;
    write_json_atomic(&path, &manifest, "state session")?;
    Ok(manifest)
}

pub(crate) fn validate_claim_session(
    claim: &LaneClaim,
    manifest: &LanePassSessionManifest,
    label: &str,
) -> Result<()> {
    if let Some(state_session_id) = &claim.state_session_id
        && state_session_id != &manifest.state_session_id
    {
        return Err(DrivenError::Validation(format!(
            "{} state identity does not match lane/pass state",
            label
        )));
    }
    Ok(())
}

pub(crate) fn validate_receipt_session(
    receipt_claim: &LaneClaim,
    active_claim: &LaneClaim,
) -> Result<()> {
    match (
        &receipt_claim.state_session_id,
        &active_claim.state_session_id,
    ) {
        (Some(receipt), Some(active)) if receipt == active => Ok(()),
        (Some(_), Some(_)) => Err(DrivenError::Validation(
            "receipt state identity does not match active lane claim".to_string(),
        )),
        (None, Some(_)) => Err(DrivenError::Validation(
            "receipt claim state identity is required".to_string(),
        )),
        (Some(_), None) => Err(DrivenError::Validation(
            "active lane claim state identity is missing".to_string(),
        )),
        (None, None) => Ok(()),
    }
}

fn session_path(state_dir: &Path) -> Result<PathBuf> {
    Ok(contained_state_dir(state_dir)?.join(STATE_SESSION_FILE))
}
