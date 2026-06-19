//! DX lane/pass strategy contracts.
//!
//! This module models worker lane ownership, pass continuity, subagent
//! delegation, worktree metadata, proof receipts, and next-pass handoffs.

pub(crate) mod artifacts;
mod evidence;
mod handoff;
mod identity;
mod orchestration;
mod receipt;
mod redaction;
mod session;
mod state_lock;
mod worktree;

pub use evidence::{COMMAND_EVIDENCE_CAPTURED_BY, CommandEvidence};
pub use handoff::{NextPassHandoff, PassOutcome};
pub use identity::{
    ClaimId, ClaimStatus, ClaimToken, LaneClaim, LaneId, PassNumber, StateSessionId,
    SubagentDelegation, WorkerId, WorkerIdentity, WorkerKind, derive_claim_token,
};
pub use orchestration::{
    LanePassAssignment, LanePassAssignmentStatus, LanePassConfig, LanePassStatePaths, LanePassStore,
};
pub use receipt::{
    CommandProof, CommandStatus, FileProof, OutcomeProof, ProofReceipt, ReceiptFormat,
    VerificationClass,
};
pub use worktree::{
    GitCheckoutKind, WorktreeCreationDecision, WorktreeIdentity, WorktreeIsolationMode,
    WorktreeIsolationPlan, WorktreeMetadata, detect_worktree_metadata, plan_worktree_isolation,
};

pub const LANE_CLAIM_SCHEMA: &str = "driven.lane_claim.v1";
pub const LANE_HANDOFF_SCHEMA: &str = "driven.lane_handoff.v1";
pub const PROOF_RECEIPT_SCHEMA: &str = "driven.proof_receipt.v1";
pub const MAX_LANES: u8 = 30;
