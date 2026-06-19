use crate::{DrivenError, Result};
use serde::{Deserialize, Serialize};

use super::redaction::redact_secrets;
use super::{
    ClaimToken, LANE_HANDOFF_SCHEMA, LaneClaim, LaneId, OutcomeProof, PassNumber, ProofReceipt,
    WorkerId, WorktreeIdentity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassOutcome {
    Completed,
    Partial,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextPassHandoff {
    pub schema: String,
    pub lane: LaneId,
    pub completed_pass: PassNumber,
    pub next_pass: PassNumber,
    pub worker_id: WorkerId,
    pub from_claim: ClaimToken,
    pub receipt_id: String,
    pub outcome: PassOutcome,
    pub summary: String,
    pub next_action: String,
    pub blockers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_identity: Option<WorktreeIdentity>,
}

impl LaneClaim {
    pub fn next_pass_handoff(
        &self,
        receipt: ProofReceipt,
        next_action: impl Into<String>,
    ) -> Result<NextPassHandoff> {
        self.validate()?;
        receipt.validate()?;
        if receipt.redacted {
            return Err(DrivenError::Validation(
                "next-pass handoff requires an unredacted proof receipt".to_string(),
            ));
        }
        if receipt.claim.lane != self.lane
            || receipt.claim.pass != self.pass
            || receipt.claim.worker_id != self.worker_id
            || receipt.claim.token != self.token
        {
            return Err(DrivenError::Validation(
                "receipt claim does not match lane claim".to_string(),
            ));
        }

        let next_action = next_action.into();
        if next_action.trim().is_empty() {
            return Err(DrivenError::Validation(
                "next-pass handoff requires a next action".to_string(),
            ));
        }

        let blockers = receipt.blockers();
        let outcome = if !blockers.is_empty() {
            PassOutcome::Blocked
        } else if receipt
            .outcomes
            .iter()
            .any(|outcome| matches!(outcome, OutcomeProof::Partial { .. }))
        {
            PassOutcome::Partial
        } else {
            PassOutcome::Completed
        };

        Ok(NextPassHandoff {
            schema: LANE_HANDOFF_SCHEMA.to_string(),
            lane: self.lane,
            completed_pass: self.pass,
            next_pass: self.pass.next()?,
            worker_id: self.worker_id.clone(),
            from_claim: self.token.clone(),
            receipt_id: receipt.receipt_id()?,
            outcome,
            summary: receipt.summary().to_string(),
            next_action,
            blockers,
            payload_digest: None,
            worktree_identity: None,
        })
    }
}

impl NextPassHandoff {
    pub fn validate(&self) -> Result<()> {
        if self.schema != LANE_HANDOFF_SCHEMA {
            return Err(DrivenError::Validation(format!(
                "handoff schema must be {}",
                LANE_HANDOFF_SCHEMA
            )));
        }
        if self.receipt_id.trim().is_empty() {
            return Err(DrivenError::Validation(
                "handoff receipt id cannot be empty".to_string(),
            ));
        }
        if self.summary.trim().is_empty() {
            return Err(DrivenError::Validation(
                "handoff summary cannot be empty".to_string(),
            ));
        }
        if self.next_action.trim().is_empty() {
            return Err(DrivenError::Validation(
                "handoff next action cannot be empty".to_string(),
            ));
        }
        if self.outcome == PassOutcome::Blocked && self.blockers.is_empty() {
            return Err(DrivenError::Validation(
                "blocked handoff requires at least one blocker".to_string(),
            ));
        }
        if self.outcome == PassOutcome::Completed && !self.blockers.is_empty() {
            return Err(DrivenError::Validation(
                "completed handoff cannot include blockers".to_string(),
            ));
        }
        if self.next_pass != self.completed_pass.next()? {
            return Err(DrivenError::Validation(
                "handoff next pass must increment exactly once".to_string(),
            ));
        }
        if let Some(payload_digest) = &self.payload_digest
            && payload_digest != &self.computed_payload_digest()?
        {
            return Err(DrivenError::Validation(
                "handoff payload digest does not match payload".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_persisted(&self) -> Result<()> {
        match &self.payload_digest {
            Some(payload_digest) if !payload_digest.trim().is_empty() => {}
            _ => {
                return Err(DrivenError::Validation(
                    "stored handoff payload digest is required".to_string(),
                ));
            }
        }
        if self.worktree_identity.is_none() {
            return Err(DrivenError::Validation(
                "stored handoff worktree identity is required".to_string(),
            ));
        }
        self.validate()
    }

    pub(crate) fn redacted_for_persistence(&self) -> Self {
        let mut handoff = self.clone();
        handoff.payload_digest = None;
        handoff.summary = redact_secrets(&handoff.summary);
        handoff.next_action = redact_secrets(&handoff.next_action);
        for blocker in &mut handoff.blockers {
            *blocker = redact_secrets(blocker);
        }
        handoff
    }

    pub(crate) fn seal_payload_digest(&mut self) -> Result<()> {
        self.payload_digest = Some(self.computed_payload_digest()?);
        Ok(())
    }

    fn computed_payload_digest(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.payload_digest = None;
        let payload = serde_json::to_vec(&canonical)
            .map_err(|e| DrivenError::Format(format!("failed to render handoff JSON: {}", e)))?;
        Ok(blake3::hash(&payload).to_hex().to_string())
    }

    pub fn validate_against(&self, claim: &LaneClaim) -> Result<()> {
        self.validate()?;
        claim.validate()?;
        if self.lane != claim.lane {
            return Err(DrivenError::Validation(
                "handoff lane does not match claim lane".to_string(),
            ));
        }
        if self.completed_pass != claim.pass {
            return Err(DrivenError::Validation(
                "handoff completed pass does not match claim pass".to_string(),
            ));
        }
        if self.next_pass != claim.pass.next()? {
            return Err(DrivenError::Validation(
                "handoff next pass must increment exactly once".to_string(),
            ));
        }
        if self.worker_id != claim.worker_id {
            return Err(DrivenError::Validation(
                "handoff worker does not match claim worker".to_string(),
            ));
        }
        if self.from_claim != claim.token {
            return Err(DrivenError::Validation(
                "handoff claim token does not match claim".to_string(),
            ));
        }
        Ok(())
    }
}
