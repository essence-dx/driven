use crate::{DrivenError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use super::artifacts::{
    contained_state_dir, ensure_artifact_path_in_state, handoff_path as build_handoff_path,
    read_handoff_artifact, read_receipt_artifact, receipt_path as build_receipt_path,
    write_file_atomic, write_handoff_artifact, write_receipt_artifact,
};
use super::session::{read_or_create_session, validate_claim_session, validate_receipt_session};
use super::state_lock::StateLock;
use super::worktree::normalize_worktree_root;
use super::{
    ClaimId, ClaimStatus, LaneClaim, LaneId, PassNumber, PassOutcome, ProofReceipt, WorkerId,
    WorktreeIsolationPlan, WorktreeMetadata, detect_worktree_metadata,
};

const COUNTER_FILE: &str = "lane-counter.json";
const CLAIMS_FILE: &str = "lane-claims.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanePassConfig {
    pub state_dir: PathBuf,
    pub scope: String,
    pub max_lanes: u8,
    pub max_passes: u32,
    pub cycle_lanes: bool,
    pub project_root: Option<PathBuf>,
    pub handoff_required_for_next: bool,
}

impl LanePassConfig {
    pub fn new(state_dir: impl Into<PathBuf>, scope: impl Into<String>) -> Self {
        Self {
            state_dir: state_dir.into(),
            scope: scope.into(),
            max_lanes: super::MAX_LANES,
            max_passes: 3,
            cycle_lanes: false,
            project_root: None,
            handoff_required_for_next: false,
        }
    }

    pub fn with_max_lanes(mut self, max_lanes: u8) -> Self {
        self.max_lanes = max_lanes;
        self
    }

    pub fn with_max_passes(mut self, max_passes: u32) -> Self {
        self.max_passes = max_passes;
        self
    }

    pub fn with_lane_cycling(mut self, cycle_lanes: bool) -> Self {
        self.cycle_lanes = cycle_lanes;
        self
    }

    pub fn with_project_root(mut self, project_root: impl Into<PathBuf>) -> Self {
        let project_root = project_root.into();
        self.project_root = Some(normalize_worktree_root(&project_root));
        self
    }

    pub fn with_handoff_required_for_next(mut self, required: bool) -> Self {
        self.handoff_required_for_next = required;
        self
    }

    fn validate(&self) -> Result<()> {
        if self.scope.trim().is_empty() {
            return Err(DrivenError::Validation(
                "lane/pass scope cannot be empty".to_string(),
            ));
        }
        if self.max_lanes == 0 || self.max_lanes > super::MAX_LANES {
            return Err(DrivenError::Validation(format!(
                "max lanes must be between 1 and {}",
                super::MAX_LANES
            )));
        }
        if self.max_passes == 0 {
            return Err(DrivenError::Validation(
                "max passes must be at least 1".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanePassStatePaths {
    pub state_dir: PathBuf,
    pub counter_path: PathBuf,
    pub claims_path: PathBuf,
    pub worker_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanePassAssignmentStatus {
    Peeked,
    Claimed,
    Advanced,
    Completed,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanePassAssignment {
    pub status: LanePassAssignmentStatus,
    pub scope: String,
    pub lane: LaneId,
    pub pass: PassNumber,
    pub worker_id: WorkerId,
    pub claim: LaneClaim,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_path: Option<PathBuf>,
    pub paths: LanePassStatePaths,
    pub worktree: WorktreeMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_plan: Option<WorktreeIsolationPlan>,
}

impl LanePassAssignment {
    pub fn validate(&self) -> Result<()> {
        self.claim.validate()?;
        if self.scope.trim().is_empty() {
            return Err(DrivenError::Validation(
                "lane/pass assignment scope cannot be empty".to_string(),
            ));
        }
        if self.claim.scope != self.scope {
            return Err(DrivenError::Validation(
                "assignment scope does not match claim scope".to_string(),
            ));
        }
        if self.claim.lane != self.lane {
            return Err(DrivenError::Validation(
                "assignment lane does not match claim lane".to_string(),
            ));
        }
        if self.claim.pass != self.pass {
            return Err(DrivenError::Validation(
                "assignment pass does not match claim pass".to_string(),
            ));
        }
        if self.claim.worker_id != self.worker_id {
            return Err(DrivenError::Validation(
                "assignment worker does not match claim worker".to_string(),
            ));
        }
        let expected_claim_status = match self.status {
            LanePassAssignmentStatus::Peeked
            | LanePassAssignmentStatus::Claimed
            | LanePassAssignmentStatus::Advanced => ClaimStatus::Claimed,
            LanePassAssignmentStatus::Completed => ClaimStatus::Completed,
            LanePassAssignmentStatus::Released => ClaimStatus::Released,
        };
        if self.claim.status != expected_claim_status {
            return Err(DrivenError::Validation(
                "assignment status does not match claim status".to_string(),
            ));
        }
        if let Some(receipt_id) = &self.receipt_id
            && receipt_id.trim().is_empty()
        {
            return Err(DrivenError::Validation(
                "assignment receipt id cannot be empty".to_string(),
            ));
        }
        if let Some(receipt_path) = &self.receipt_path
            && receipt_path.as_os_str().is_empty()
        {
            return Err(DrivenError::Validation(
                "assignment receipt path cannot be empty".to_string(),
            ));
        }
        if let Some(handoff_id) = &self.handoff_id
            && handoff_id.trim().is_empty()
        {
            return Err(DrivenError::Validation(
                "assignment handoff id cannot be empty".to_string(),
            ));
        }
        if let Some(handoff_path) = &self.handoff_path
            && handoff_path.as_os_str().is_empty()
        {
            return Err(DrivenError::Validation(
                "assignment handoff path cannot be empty".to_string(),
            ));
        }
        if self.receipt_id.is_some() != self.receipt_path.is_some() {
            return Err(DrivenError::Validation(
                "assignment receipt id and path must be recorded together".to_string(),
            ));
        }
        if self.handoff_id.is_some() != self.handoff_path.is_some() {
            return Err(DrivenError::Validation(
                "assignment handoff id and path must be recorded together".to_string(),
            ));
        }
        if let (Some(receipt_id), Some(handoff_id)) = (&self.receipt_id, &self.handoff_id)
            && receipt_id != handoff_id
        {
            return Err(DrivenError::Validation(
                "assignment receipt and handoff ids must match".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct LanePassStore {
    config: LanePassConfig,
}

impl LanePassStore {
    pub fn new(config: LanePassConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn peek_next_claim(&self) -> Result<LanePassAssignment> {
        let lane = self.peek_next_lane()?;
        let worker_id = WorkerId::new("unclaimed")?;
        let execution_root = self.execution_root();
        let worktree = detect_worktree_metadata(&execution_root);
        let worktree_plan = WorktreeIsolationPlan::from_metadata(worktree.clone());
        let claim = LaneClaim::new(
            lane,
            PassNumber::first(),
            worker_id.clone(),
            self.config.scope.clone(),
        );

        Ok(LanePassAssignment {
            status: LanePassAssignmentStatus::Peeked,
            scope: self.config.scope.clone(),
            lane,
            pass: PassNumber::first(),
            worker_id,
            claim,
            receipt_id: None,
            receipt_path: None,
            handoff_id: None,
            handoff_path: None,
            paths: self.paths_for_worker("unclaimed")?,
            worktree,
            worktree_plan: Some(worktree_plan),
        })
    }

    pub fn claim(&self, worker_id: WorkerId) -> Result<LanePassAssignment> {
        let state_dir = contained_state_dir(&self.config.state_dir)?;
        fs::create_dir_all(&state_dir).map_err(DrivenError::Io)?;
        let _lock = StateLock::acquire(&state_dir)?;

        if let Some(existing) = self.read_worker_assignment(&worker_id)?
            && existing.claim.status == ClaimStatus::Claimed
        {
            self.validate_current_worktree_identity(&existing)?;
            return Ok(existing);
        }

        let lane = self.peek_next_lane()?;
        let pass = PassNumber::first();
        let claim_id = self.next_claim_id()?;
        let assignment = self.build_assignment(
            LanePassAssignmentStatus::Claimed,
            lane,
            pass,
            worker_id,
            ClaimStatus::Claimed,
            claim_id,
        )?;
        self.write_assignment(&assignment)?;
        Ok(assignment)
    }

    pub fn worker_assignment(&self, worker_id: &WorkerId) -> Result<Option<LanePassAssignment>> {
        let state_dir = contained_state_dir(&self.config.state_dir)?;
        if !state_dir.exists() {
            return Ok(None);
        }
        let _lock = StateLock::acquire(&state_dir)?;
        self.read_worker_assignment(worker_id)
    }

    pub fn next_pass(&self, worker_id: &WorkerId) -> Result<LanePassAssignment> {
        if self.config.handoff_required_for_next {
            return Err(DrivenError::Validation(
                "durable handoff is required for next-pass advancement; use next_pass_with_handoff"
                    .to_string(),
            ));
        }
        let state_dir = contained_state_dir(&self.config.state_dir)?;
        fs::create_dir_all(&state_dir).map_err(DrivenError::Io)?;
        let _lock = StateLock::acquire(&state_dir)?;

        let current = self.read_worker_assignment(worker_id)?.ok_or_else(|| {
            DrivenError::Validation(format!("worker {} has no lane claim", worker_id))
        })?;
        if current.claim.status != ClaimStatus::Claimed {
            return Err(DrivenError::Validation(format!(
                "worker {} lane claim is not active",
                worker_id
            )));
        }
        self.validate_current_worktree_identity(&current)?;
        let next_pass = current.pass.next()?;
        if next_pass.value() > self.config.max_passes {
            return Err(DrivenError::Validation(format!(
                "worker {} reached max passes ({})",
                worker_id, self.config.max_passes
            )));
        }

        let claim_id = self.next_claim_id()?;
        let assignment = self.build_assignment(
            LanePassAssignmentStatus::Advanced,
            current.lane,
            next_pass,
            worker_id.clone(),
            ClaimStatus::Claimed,
            claim_id,
        )?;
        self.write_assignment(&assignment)?;
        Ok(assignment)
    }

    pub fn next_pass_with_handoff(
        &self,
        worker_id: &WorkerId,
        receipt: &ProofReceipt,
        next_action: &str,
    ) -> Result<LanePassAssignment> {
        let state_dir = contained_state_dir(&self.config.state_dir)?;
        fs::create_dir_all(&state_dir).map_err(DrivenError::Io)?;
        let _lock = StateLock::acquire(&state_dir)?;

        let current = self.read_worker_assignment(worker_id)?.ok_or_else(|| {
            DrivenError::Validation(format!("worker {} has no lane claim", worker_id))
        })?;
        if current.claim.status != ClaimStatus::Claimed {
            return Err(DrivenError::Validation(format!(
                "worker {} lane claim is not active",
                worker_id
            )));
        }
        self.validate_current_worktree_identity(&current)?;

        let mut receipt = receipt.clone();
        if receipt.redacted {
            return Err(DrivenError::Validation(
                "next-pass handoff requires an unredacted proof receipt".to_string(),
            ));
        }
        if receipt.receipt_id.trim().is_empty() {
            receipt.receipt_id = receipt.receipt_id()?;
        }
        receipt.validate()?;
        receipt.validate_worktree_identity(&current.worktree.identity())?;
        validate_receipt_session(&receipt.claim, &current.claim)?;
        let mut handoff = current
            .claim
            .next_pass_handoff(receipt.clone(), next_action.to_string())?;
        handoff.worktree_identity = Some(current.worktree.identity());
        handoff.validate_against(&current.claim)?;
        if handoff.outcome == PassOutcome::Blocked {
            return Err(DrivenError::Validation(
                "blocked handoff cannot advance the next pass".to_string(),
            ));
        }
        if handoff.next_pass.value() > self.config.max_passes {
            return Err(DrivenError::Validation(format!(
                "worker {} reached max passes ({})",
                worker_id, self.config.max_passes
            )));
        }

        let claim_id = self.next_claim_id()?;
        let mut assignment = self.build_assignment(
            LanePassAssignmentStatus::Advanced,
            current.lane,
            handoff.next_pass,
            worker_id.clone(),
            ClaimStatus::Claimed,
            claim_id,
        )?;
        let receipt_path = build_receipt_path(&self.config.state_dir, &receipt.receipt_id)?;
        let handoff_path = build_handoff_path(&self.config.state_dir, &handoff.receipt_id)?;
        assignment.receipt_id = Some(receipt.receipt_id.clone());
        assignment.receipt_path = Some(receipt_path.clone());
        assignment.handoff_id = Some(handoff.receipt_id.clone());
        assignment.handoff_path = Some(handoff_path.clone());
        write_receipt_artifact(&receipt_path, &receipt)?;
        write_handoff_artifact(&handoff_path, &handoff)?;
        self.write_assignment(&assignment)?;
        Ok(assignment)
    }

    pub fn complete_pass(&self, worker_id: &WorkerId) -> Result<LanePassAssignment> {
        let _ = worker_id;
        Err(DrivenError::Validation(
            "completion requires a canonical proof receipt; use complete_pass_with_receipt"
                .to_string(),
        ))
    }

    pub fn complete_pass_with_receipt(
        &self,
        worker_id: &WorkerId,
        receipt: &ProofReceipt,
    ) -> Result<LanePassAssignment> {
        let state_dir = contained_state_dir(&self.config.state_dir)?;
        if !state_dir.exists() {
            return Err(DrivenError::Validation(format!(
                "worker {} has no lane claim",
                worker_id
            )));
        }
        fs::create_dir_all(&state_dir).map_err(DrivenError::Io)?;
        let _lock = StateLock::acquire(&state_dir)?;

        let current = self.read_worker_assignment(worker_id)?.ok_or_else(|| {
            DrivenError::Validation(format!("worker {} has no lane claim", worker_id))
        })?;
        if current.claim.status != ClaimStatus::Claimed {
            return Err(DrivenError::Validation(format!(
                "worker {} lane claim is not active",
                worker_id
            )));
        }
        self.validate_current_worktree_identity(&current)?;
        receipt.validate_completion_readiness()?;
        receipt.validate_completion_claim_subject(&current.claim)?;
        receipt.validate_worktree_identity(&current.worktree.identity())?;
        validate_receipt_session(&receipt.claim, &current.claim)?;
        receipt.validate_completion_claim_match(&current.claim)?;

        let mut assignment = self.build_assignment(
            LanePassAssignmentStatus::Completed,
            current.lane,
            current.pass,
            worker_id.clone(),
            ClaimStatus::Completed,
            current.claim.claim_id.clone(),
        )?;
        let receipt_path = build_receipt_path(&self.config.state_dir, &receipt.receipt_id)?;
        assignment.receipt_id = Some(receipt.receipt_id.clone());
        assignment.receipt_path = Some(receipt_path.clone());
        write_receipt_artifact(&receipt_path, receipt)?;
        self.write_assignment(&assignment)?;
        Ok(assignment)
    }

    pub fn release_lane(&self, worker_id: &WorkerId) -> Result<LanePassAssignment> {
        self.update_claim_status(
            worker_id,
            LanePassAssignmentStatus::Released,
            ClaimStatus::Released,
        )
    }

    fn peek_next_lane(&self) -> Result<LaneId> {
        let counter = self.read_counter()?.max(self.highest_recorded_lane()?);
        let next = counter.saturating_add(1);
        if next > self.config.max_lanes {
            if self.config.cycle_lanes {
                self.first_available_lane()
            } else {
                Err(DrivenError::Validation(format!(
                    "max lanes ({}) reached",
                    self.config.max_lanes
                )))
            }
        } else {
            LaneId::new(next)
        }
    }

    fn build_assignment(
        &self,
        status: LanePassAssignmentStatus,
        lane: LaneId,
        pass: PassNumber,
        worker_id: WorkerId,
        claim_status: ClaimStatus,
        claim_id: ClaimId,
    ) -> Result<LanePassAssignment> {
        let session = read_or_create_session(&self.config.state_dir, &self.config.scope)?;
        let mut claim = LaneClaim::new(lane, pass, worker_id.clone(), self.config.scope.clone())
            .with_claim_id(claim_id)
            .with_state_session_id(session.state_session_id);
        claim.status = claim_status;
        let paths = self.paths_for_worker(worker_id.as_str())?;
        let execution_root = self.execution_root();
        let worktree = detect_worktree_metadata(&execution_root);
        let worktree_plan = Some(WorktreeIsolationPlan::from_metadata(worktree.clone()));

        Ok(LanePassAssignment {
            status,
            scope: self.config.scope.clone(),
            lane,
            pass,
            worker_id,
            claim,
            receipt_id: None,
            receipt_path: None,
            handoff_id: None,
            handoff_path: None,
            paths,
            worktree,
            worktree_plan,
        })
    }

    fn next_claim_id(&self) -> Result<ClaimId> {
        let paths = self.paths_for_worker("claim-sequence")?;
        let next = self
            .read_claims(&paths.claims_path)?
            .iter()
            .filter_map(|assignment| assignment.claim.claim_id.sequence())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| DrivenError::Validation("claim id sequence overflow".to_string()))?;
        Ok(ClaimId::from_sequence(next))
    }

    fn paths_for_worker(&self, worker_id: &str) -> Result<LanePassStatePaths> {
        let state_dir = contained_state_dir(&self.config.state_dir)?;
        let worker_file = format!("worker-{}.json", safe_component(worker_id)?);
        Ok(LanePassStatePaths {
            counter_path: state_dir.join(COUNTER_FILE),
            claims_path: state_dir.join(CLAIMS_FILE),
            worker_path: state_dir.join("workers").join(worker_file),
            state_dir,
        })
    }

    fn read_worker_assignment(&self, worker_id: &WorkerId) -> Result<Option<LanePassAssignment>> {
        let path = self.paths_for_worker(worker_id.as_str())?.worker_path;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).map_err(DrivenError::Io)?;
        let assignment: LanePassAssignment = serde_json::from_str(&content)
            .map_err(|e| DrivenError::Parse(format!("failed to parse worker claim: {}", e)))?;
        assignment.validate()?;
        if &assignment.worker_id != worker_id {
            return Err(DrivenError::Validation(format!(
                "worker state file belongs to {}, not {}",
                assignment.worker_id, worker_id
            )));
        }
        if assignment.scope != self.config.scope {
            return Err(DrivenError::Validation(format!(
                "worker {} lane claim scope {} does not match store scope {}",
                worker_id, assignment.scope, self.config.scope
            )));
        }
        let session = read_or_create_session(&self.config.state_dir, &self.config.scope)?;
        validate_claim_session(&assignment.claim, &session, "assignment")?;
        self.validate_assignment_artifacts(&assignment)?;
        Ok(Some(assignment))
    }

    fn update_claim_status(
        &self,
        worker_id: &WorkerId,
        assignment_status: LanePassAssignmentStatus,
        claim_status: ClaimStatus,
    ) -> Result<LanePassAssignment> {
        let state_dir = contained_state_dir(&self.config.state_dir)?;
        fs::create_dir_all(&state_dir).map_err(DrivenError::Io)?;
        let _lock = StateLock::acquire(&state_dir)?;

        let current = self.read_worker_assignment(worker_id)?.ok_or_else(|| {
            DrivenError::Validation(format!("worker {} has no lane claim", worker_id))
        })?;
        if current.claim.status != ClaimStatus::Claimed {
            return Err(DrivenError::Validation(format!(
                "worker {} lane claim is not active",
                worker_id
            )));
        }
        self.validate_current_worktree_identity(&current)?;

        let assignment = self.build_assignment(
            assignment_status,
            current.lane,
            current.pass,
            worker_id.clone(),
            claim_status,
            current.claim.claim_id.clone(),
        )?;
        self.write_assignment(&assignment)?;
        Ok(assignment)
    }

    fn write_assignment(&self, assignment: &LanePassAssignment) -> Result<()> {
        let paths = &assignment.paths;
        fs::create_dir_all(&paths.state_dir).map_err(DrivenError::Io)?;
        if let Some(parent) = paths.worker_path.parent() {
            fs::create_dir_all(parent).map_err(DrivenError::Io)?;
        }

        let worker_content = serde_json::to_string_pretty(assignment)
            .map(|mut value| {
                value.push('\n');
                value
            })
            .map_err(|e| DrivenError::Format(format!("failed to render lane claim: {}", e)))?;

        let mut claims = self.read_claims(&paths.claims_path)?;
        claims.push(assignment.clone());
        let claims_content = serde_json::to_string_pretty(&claims)
            .map(|mut value| {
                value.push('\n');
                value
            })
            .map_err(|e| DrivenError::Format(format!("failed to render claims: {}", e)))?;
        let counter_content = serde_json::to_string_pretty(&LaneCounter {
            last_lane: assignment.lane.value(),
        })
        .map(|mut value| {
            value.push('\n');
            value
        })
        .map_err(|e| DrivenError::Format(format!("failed to render lane counter: {}", e)))?;

        write_file_atomic(&paths.claims_path, claims_content.as_bytes())?;
        write_file_atomic(&paths.worker_path, worker_content.as_bytes())?;
        write_file_atomic(&paths.counter_path, counter_content.as_bytes())
    }

    fn validate_assignment_artifacts(&self, assignment: &LanePassAssignment) -> Result<()> {
        if let (Some(receipt_id), Some(receipt_path)) =
            (&assignment.receipt_id, &assignment.receipt_path)
        {
            ensure_artifact_path_in_state(&self.config.state_dir, receipt_path, "receipt")?;
            let canonical_receipt_path = build_receipt_path(&self.config.state_dir, receipt_id)?;
            if receipt_path != &canonical_receipt_path {
                return Err(DrivenError::Validation(
                    "assignment receipt path must match canonical receipt path".to_string(),
                ));
            }
            let stored_receipt = read_receipt_artifact(receipt_path)?;
            stored_receipt.validate()?;
            if stored_receipt.receipt_id != *receipt_id {
                return Err(DrivenError::Validation(
                    "stored receipt id does not match assignment receipt id".to_string(),
                ));
            }
            let expected_receipt_pass = if assignment.handoff_id.is_some() {
                assignment
                    .pass
                    .value()
                    .checked_sub(1)
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        DrivenError::Validation(
                            "handoff receipt cannot belong to pass zero".to_string(),
                        )
                    })
                    .and_then(PassNumber::new)?
            } else {
                assignment.pass
            };
            if stored_receipt.claim.lane != assignment.lane
                || stored_receipt.claim.pass != expected_receipt_pass
                || stored_receipt.claim.worker_id != assignment.worker_id
                || stored_receipt.claim.scope != assignment.scope
            {
                return Err(DrivenError::Validation(
                    "stored receipt claim does not match assignment claim".to_string(),
                ));
            }
            validate_receipt_session(&stored_receipt.claim, &assignment.claim)?;
            stored_receipt.validate_worktree_identity(&assignment.worktree.identity())?;
            if assignment.handoff_id.is_none()
                && (stored_receipt.claim.claim_id != assignment.claim.claim_id
                    || stored_receipt.claim.token != assignment.claim.token)
            {
                return Err(DrivenError::Validation(
                    "stored receipt claim does not match assignment claim".to_string(),
                ));
            }
        }

        if let (Some(handoff_id), Some(handoff_path)) =
            (&assignment.handoff_id, &assignment.handoff_path)
        {
            ensure_artifact_path_in_state(&self.config.state_dir, handoff_path, "handoff")?;
            let canonical_handoff_path = build_handoff_path(&self.config.state_dir, handoff_id)?;
            if handoff_path != &canonical_handoff_path {
                return Err(DrivenError::Validation(
                    "assignment handoff path must match canonical handoff path".to_string(),
                ));
            }
            let stored_handoff = read_handoff_artifact(handoff_path)?;
            if stored_handoff.worktree_identity.is_none() {
                return Err(DrivenError::Validation(
                    "stored handoff worktree identity is required".to_string(),
                ));
            }
            if stored_handoff.receipt_id != *handoff_id {
                return Err(DrivenError::Validation(
                    "stored handoff receipt id does not match assignment handoff id".to_string(),
                ));
            }
            if let Some(receipt_id) = &assignment.receipt_id
                && stored_handoff.receipt_id != *receipt_id
            {
                return Err(DrivenError::Validation(
                    "stored handoff receipt id does not match assignment receipt id".to_string(),
                ));
            }
            if stored_handoff.lane != assignment.lane
                || stored_handoff.worker_id != assignment.worker_id
                || stored_handoff.next_pass != assignment.pass
            {
                return Err(DrivenError::Validation(
                    "stored handoff does not match assignment lane/pass/worker".to_string(),
                ));
            }
            let receipt_path = assignment.receipt_path.as_ref().ok_or_else(|| {
                DrivenError::Validation(
                    "stored handoff requires a receipt artifact for claim validation".to_string(),
                )
            })?;
            let stored_receipt = read_receipt_artifact(receipt_path)?;
            if stored_handoff.from_claim != stored_receipt.claim.token {
                return Err(DrivenError::Validation(
                    "stored handoff claim token does not match assignment claim".to_string(),
                ));
            }
            let expected_handoff = stored_receipt
                .claim
                .next_pass_handoff(stored_receipt.clone(), stored_handoff.next_action.clone())?
                .redacted_for_persistence();
            if stored_handoff.summary != expected_handoff.summary {
                return Err(DrivenError::Validation(
                    "stored handoff summary does not match receipt summary".to_string(),
                ));
            }
            if stored_handoff.outcome != expected_handoff.outcome {
                return Err(DrivenError::Validation(
                    "stored handoff outcome does not match receipt outcome".to_string(),
                ));
            }
            if stored_handoff.blockers != expected_handoff.blockers {
                return Err(DrivenError::Validation(
                    "stored handoff blockers do not match receipt blockers".to_string(),
                ));
            }
            let identity = stored_handoff.worktree_identity.as_ref().ok_or_else(|| {
                DrivenError::Validation("stored handoff worktree identity is required".to_string())
            })?;
            if identity != &assignment.worktree.identity() {
                return Err(DrivenError::Validation(
                    "stored handoff worktree identity does not match assignment worktree"
                        .to_string(),
                ));
            }
            stored_handoff.validate_persisted()?;
        }

        Ok(())
    }

    pub(crate) fn validate_current_worktree_identity(
        &self,
        assignment: &LanePassAssignment,
    ) -> Result<()> {
        let current = detect_worktree_metadata(&self.execution_root());
        if !assignment.worktree.has_same_identity(&current) {
            return Err(DrivenError::Validation(
                "worktree identity changed since the active lane claim".to_string(),
            ));
        }
        Ok(())
    }

    fn read_claims(&self, path: &Path) -> Result<Vec<LanePassAssignment>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(path).map_err(DrivenError::Io)?;
        let claims: Vec<LanePassAssignment> = serde_json::from_str(&content)
            .map_err(|e| DrivenError::Parse(format!("failed to parse claims: {}", e)))?;
        for claim in &claims {
            claim.validate()?;
        }
        Ok(claims)
    }

    fn read_counter(&self) -> Result<u8> {
        let path = contained_state_dir(&self.config.state_dir)?.join(COUNTER_FILE);
        if !path.exists() {
            return Ok(0);
        }
        let content = fs::read_to_string(&path).map_err(DrivenError::Io)?;
        serde_json::from_str::<LaneCounter>(&content)
            .map(|counter| counter.last_lane)
            .map_err(|e| DrivenError::Parse(format!("failed to parse lane counter: {}", e)))
    }

    fn highest_recorded_lane(&self) -> Result<u8> {
        let paths = self.paths_for_worker("scan")?;
        Ok(self
            .read_claims(&paths.claims_path)?
            .iter()
            .map(|assignment| assignment.lane.value())
            .max()
            .unwrap_or(0))
    }

    fn first_available_lane(&self) -> Result<LaneId> {
        let paths = self.paths_for_worker("scan")?;
        let claims = self.read_claims(&paths.claims_path)?;
        let current_worktree = detect_worktree_metadata(&self.execution_root());
        for lane in 1..=self.config.max_lanes {
            let latest = claims
                .iter()
                .rev()
                .find(|assignment| assignment.lane.value() == lane);
            match latest {
                None => return LaneId::new(lane),
                Some(assignment) if assignment.claim.status == ClaimStatus::Claimed => {}
                Some(assignment) if assignment.worktree.has_same_identity(&current_worktree) => {
                    return LaneId::new(lane);
                }
                Some(_) => {}
            }
        }
        Err(DrivenError::Validation(format!(
            "max lanes ({}) reached and no released lane is available for the current worktree",
            self.config.max_lanes
        )))
    }

    fn execution_root(&self) -> PathBuf {
        if let Some(project_root) = &self.config.project_root {
            return project_root.clone();
        }
        if self.config.state_dir.is_relative() {
            return PathBuf::from(".");
        }
        self.config
            .state_dir
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct LaneCounter {
    last_lane: u8,
}

fn safe_component(raw: &str) -> Result<String> {
    let worker = WorkerId::new(raw)?;
    Ok(worker.as_str().to_string())
}
