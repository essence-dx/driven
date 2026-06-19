use crate::{DrivenError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

use super::{LANE_CLAIM_SCHEMA, MAX_LANES};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LaneId(u8);

impl LaneId {
    pub fn new(value: u8) -> Result<Self> {
        if (1..=MAX_LANES).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DrivenError::Validation(format!(
                "lane must be between 1 and {}, got {}",
                MAX_LANES, value
            )))
        }
    }

    pub fn value(self) -> u8 {
        self.0
    }
}

impl fmt::Display for LaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PassNumber(u32);

impl PassNumber {
    pub fn first() -> Self {
        Self(1)
    }

    pub fn new(value: u32) -> Result<Self> {
        if value == 0 {
            Err(DrivenError::Validation(
                "pass must be at least 1".to_string(),
            ))
        } else {
            Ok(Self(value))
        }
    }

    pub fn next(self) -> Result<Self> {
        self.0
            .checked_add(1)
            .ok_or_else(|| DrivenError::Validation("pass overflow".to_string()))
            .and_then(Self::new)
    }

    pub fn value(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PassNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WorkerId(String);

impl WorkerId {
    pub fn new(raw: impl AsRef<str>) -> Result<Self> {
        let normalized = raw.as_ref().trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(DrivenError::Validation(
                "worker id cannot be empty".to_string(),
            ));
        }

        let valid = normalized.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        });
        if !valid {
            return Err(DrivenError::Validation(format!(
                "worker id contains unsupported characters: {}",
                raw.as_ref()
            )));
        }

        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate_canonical(&self, label: &str) -> Result<()> {
        let canonical = Self::new(&self.0)?;
        if &canonical != self {
            return Err(DrivenError::Validation(format!(
                "{} worker id must be normalized",
                label
            )));
        }
        Ok(())
    }
}

impl fmt::Display for WorkerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimToken(String);

impl ClaimToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClaimToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ClaimId(String);

impl ClaimId {
    pub fn new(raw: impl AsRef<str>) -> Result<Self> {
        let normalized = raw.as_ref().trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(DrivenError::Validation(
                "claim id cannot be empty".to_string(),
            ));
        }
        let valid = normalized.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.' | b':')
        });
        if !valid {
            return Err(DrivenError::Validation(format!(
                "claim id contains unsupported characters: {}",
                raw.as_ref()
            )));
        }
        Ok(Self(normalized))
    }

    pub fn legacy() -> Self {
        Self("legacy".to_string())
    }

    pub fn from_sequence(sequence: u64) -> Self {
        Self(format!("claim-{sequence:016}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_legacy(&self) -> bool {
        self.0 == "legacy"
    }

    pub(crate) fn sequence(&self) -> Option<u64> {
        self.0.strip_prefix("claim-")?.parse().ok()
    }
}

impl Default for ClaimId {
    fn default() -> Self {
        Self::legacy()
    }
}

impl fmt::Display for ClaimId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StateSessionId(String);

impl StateSessionId {
    pub fn new(raw: impl AsRef<str>) -> Result<Self> {
        let normalized = raw.as_ref().trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(DrivenError::Validation(
                "state session id cannot be empty".to_string(),
            ));
        }
        let valid = normalized.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.' | b':')
        });
        if !valid {
            return Err(DrivenError::Validation(format!(
                "state session id contains unsupported characters: {}",
                raw.as_ref()
            )));
        }
        Ok(Self(normalized))
    }

    pub fn generate() -> Self {
        Self(format!("state-{}", uuid::Uuid::new_v4().simple()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StateSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    Human,
    CodexSubagent,
    Automation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerIdentity {
    pub id: WorkerId,
    pub display_name: String,
    pub kind: WorkerKind,
}

impl WorkerIdentity {
    pub fn new(
        id: impl AsRef<str>,
        display_name: impl Into<String>,
        kind: WorkerKind,
    ) -> Result<Self> {
        let display_name = display_name.into();
        if display_name.trim().is_empty() {
            return Err(DrivenError::Validation(
                "worker display name cannot be empty".to_string(),
            ));
        }

        Ok(Self {
            id: WorkerId::new(id)?,
            display_name,
            kind,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Claimed,
    Released,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentDelegation {
    pub worker_id: WorkerId,
    pub lane: LaneId,
    pub pass: PassNumber,
    pub task: String,
}

impl SubagentDelegation {
    pub fn new(
        worker_id: WorkerId,
        lane: LaneId,
        pass: PassNumber,
        task: impl Into<String>,
    ) -> Self {
        Self {
            worker_id,
            lane,
            pass,
            task: task.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.worker_id.validate_canonical("subagent")?;
        if self.task.trim().is_empty() {
            return Err(DrivenError::Validation(
                "subagent task cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneClaim {
    pub schema: String,
    pub lane: LaneId,
    pub pass: PassNumber,
    pub worker_id: WorkerId,
    pub worker: WorkerIdentity,
    pub scope: String,
    #[serde(default, skip_serializing_if = "ClaimId::is_legacy")]
    pub claim_id: ClaimId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_session_id: Option<StateSessionId>,
    pub status: ClaimStatus,
    pub token: ClaimToken,
    pub subagents: Vec<SubagentDelegation>,
}

impl LaneClaim {
    pub fn new(
        lane: LaneId,
        pass: PassNumber,
        worker_id: WorkerId,
        scope: impl Into<String>,
    ) -> Self {
        let scope = scope.into();
        let worker = WorkerIdentity {
            display_name: worker_id.as_str().to_string(),
            id: worker_id.clone(),
            kind: WorkerKind::Automation,
        };

        Self {
            schema: LANE_CLAIM_SCHEMA.to_string(),
            token: derive_claim_token(lane, pass, &worker_id, &scope),
            lane,
            pass,
            worker_id,
            worker,
            scope,
            claim_id: ClaimId::legacy(),
            state_session_id: None,
            status: ClaimStatus::Claimed,
            subagents: Vec::new(),
        }
    }

    pub fn with_worker_identity(mut self, worker: WorkerIdentity) -> Self {
        self.worker_id = worker.id.clone();
        self.worker = worker;
        self.refresh_token();
        self
    }

    pub fn with_claim_id(mut self, claim_id: ClaimId) -> Self {
        self.claim_id = claim_id;
        self.refresh_token();
        self
    }

    pub fn with_state_session_id(mut self, state_session_id: StateSessionId) -> Self {
        self.state_session_id = Some(state_session_id);
        self.refresh_token();
        self
    }

    pub fn with_subagent(mut self, subagent: SubagentDelegation) -> Self {
        self.subagents.push(subagent);
        self
    }

    pub fn is_owner(&self, worker_id: &WorkerId) -> bool {
        &self.worker_id == worker_id
    }

    pub(crate) fn refresh_token(&mut self) {
        self.token = derive_claim_token_for_id(
            self.lane,
            self.pass,
            &self.worker_id,
            &self.scope,
            &self.claim_id,
            self.state_session_id.as_ref(),
        );
    }

    pub fn validate(&self) -> Result<()> {
        LaneId::new(self.lane.value())?;
        PassNumber::new(self.pass.value())?;
        self.worker_id.validate_canonical("claim")?;
        self.worker.id.validate_canonical("worker identity")?;
        if ClaimId::new(self.claim_id.as_str())? != self.claim_id {
            return Err(DrivenError::Validation(
                "claim id must be normalized".to_string(),
            ));
        }
        if let Some(state_session_id) = &self.state_session_id
            && StateSessionId::new(state_session_id.as_str())? != *state_session_id
        {
            return Err(DrivenError::Validation(
                "state session id must be normalized".to_string(),
            ));
        }
        if self.schema != LANE_CLAIM_SCHEMA {
            return Err(DrivenError::Validation(format!(
                "lane claim schema must be {}",
                LANE_CLAIM_SCHEMA
            )));
        }
        if self.scope.trim().is_empty() {
            return Err(DrivenError::Validation(
                "lane scope cannot be empty".to_string(),
            ));
        }
        if self.worker_id != self.worker.id {
            return Err(DrivenError::Validation(
                "claim worker id and worker identity must match".to_string(),
            ));
        }
        if self.worker.display_name.trim().is_empty() {
            return Err(DrivenError::Validation(
                "worker display name cannot be empty".to_string(),
            ));
        }
        let expected_token = derive_claim_token_for_id(
            self.lane,
            self.pass,
            &self.worker_id,
            &self.scope,
            &self.claim_id,
            self.state_session_id.as_ref(),
        );
        if self.token != expected_token {
            return Err(DrivenError::Validation(
                "claim token does not match lane/pass/worker/scope/claim id/state session"
                    .to_string(),
            ));
        }
        for subagent in &self.subagents {
            subagent.validate()?;
            if subagent.lane != self.lane || subagent.pass != self.pass {
                return Err(DrivenError::Validation(
                    "subagent lane/pass must match parent claim lane/pass".to_string(),
                ));
            }
        }
        Ok(())
    }
}

pub fn derive_claim_token(
    lane: LaneId,
    pass: PassNumber,
    worker_id: &WorkerId,
    scope: &str,
) -> ClaimToken {
    derive_claim_token_for_id(lane, pass, worker_id, scope, &ClaimId::legacy(), None)
}

fn derive_claim_token_for_id(
    lane: LaneId,
    pass: PassNumber,
    worker_id: &WorkerId,
    scope: &str,
    claim_id: &ClaimId,
    state_session_id: Option<&StateSessionId>,
) -> ClaimToken {
    let canonical = if let Some(state_session_id) = state_session_id {
        format!(
            "driven.lane_claim.v3\nlane={}\npass={}\nworker={}\nscope={}\nclaim_id={}\nstate_session_id={}\n",
            lane.value(),
            pass.value(),
            worker_id.as_str(),
            scope.trim(),
            claim_id.as_str(),
            state_session_id.as_str()
        )
    } else if claim_id.is_legacy() {
        format!(
            "driven.lane_claim.v1\nlane={}\npass={}\nworker={}\nscope={}\n",
            lane.value(),
            pass.value(),
            worker_id.as_str(),
            scope.trim()
        )
    } else {
        format!(
            "driven.lane_claim.v2\nlane={}\npass={}\nworker={}\nscope={}\nclaim_id={}\n",
            lane.value(),
            pass.value(),
            worker_id.as_str(),
            scope.trim(),
            claim_id.as_str()
        )
    };
    ClaimToken(blake3::hash(canonical.as_bytes()).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_id_accepts_only_one_through_thirty() {
        assert!(LaneId::new(1).is_ok());
        assert!(LaneId::new(30).is_ok());
        assert!(LaneId::new(0).is_err());
        assert!(LaneId::new(31).is_err());
    }

    #[test]
    fn worker_id_normalizes_and_rejects_invalid_input() {
        assert_eq!(
            WorkerId::new(" Worker.One ").unwrap().as_str(),
            "worker.one"
        );
        assert!(WorkerId::new("worker one").is_err());
    }

    #[test]
    fn claim_token_is_deterministic() {
        let lane = LaneId::new(2).unwrap();
        let pass = PassNumber::new(3).unwrap();
        let worker = WorkerId::new("worker-a").unwrap();
        assert_eq!(
            derive_claim_token(lane, pass, &worker, "scope"),
            derive_claim_token(lane, pass, &worker, "scope")
        );
    }

    #[test]
    fn claim_validation_rejects_tampered_schema_and_token() {
        let mut claim = LaneClaim::new(
            LaneId::new(1).unwrap(),
            PassNumber::first(),
            WorkerId::new("worker-a").unwrap(),
            "scope",
        );

        claim.schema = "wrong".to_string();
        assert!(claim.validate().is_err());

        claim.schema = LANE_CLAIM_SCHEMA.to_string();
        claim.scope = "other-scope".to_string();
        assert!(claim.validate().is_err());
    }
}
