use crate::{DrivenError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::redaction::redact_secrets;
use super::{CommandEvidence, LaneClaim, PROOF_RECEIPT_SCHEMA, WorktreeIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptFormat {
    Json,
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationClass {
    Small,
    Targeted,
    Heavy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum CommandStatus {
    Passed { exit_code: i32 },
    Failed { exit_code: i32 },
    Skipped { reason: String },
    Blocked { reason: String },
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandProof {
    pub command: String,
    pub class: VerificationClass,
    pub status: CommandStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<CommandEvidence>,
}

impl CommandProof {
    pub fn new(
        command: impl Into<String>,
        class: VerificationClass,
        status: CommandStatus,
    ) -> Self {
        Self {
            command: command.into(),
            class,
            status,
            evidence: None,
        }
    }

    pub fn passed(command: impl Into<String>, class: VerificationClass) -> Self {
        Self::new(command, class, CommandStatus::Passed { exit_code: 0 })
    }

    pub fn observed(
        command: impl Into<String>,
        class: VerificationClass,
        exit_code: i32,
        cwd: &Path,
        stdout: &[u8],
        stderr: &[u8],
        started_unix_seconds: u64,
        finished_unix_seconds: u64,
    ) -> Result<Self> {
        let status = if exit_code == 0 {
            CommandStatus::Passed { exit_code }
        } else {
            CommandStatus::Failed { exit_code }
        };
        let proof = Self {
            command: command.into(),
            class,
            status,
            evidence: Some(CommandEvidence::from_streams(
                cwd,
                stdout,
                stderr,
                started_unix_seconds,
                finished_unix_seconds,
            )?),
        };
        proof.validate()?;
        Ok(proof)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn observed_captured(
        command: impl Into<String>,
        class: VerificationClass,
        exit_code: i32,
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
        let status = if exit_code == 0 {
            CommandStatus::Passed { exit_code }
        } else {
            CommandStatus::Failed { exit_code }
        };
        let proof = Self {
            command: command.into(),
            class,
            status,
            evidence: Some(CommandEvidence::from_captured_streams(
                cwd,
                stdout_digest,
                stderr_digest,
                stdout_bytes,
                stderr_bytes,
                stdout_truncated,
                stderr_truncated,
                output_limit_bytes,
                duration_ms,
                started_unix_seconds,
                finished_unix_seconds,
            )?),
        };
        proof.validate()?;
        Ok(proof)
    }

    fn validate(&self) -> Result<()> {
        if self.command.trim().is_empty() {
            return Err(DrivenError::Validation(
                "command proof command cannot be empty".to_string(),
            ));
        }
        if let Some(evidence) = &self.evidence {
            if !matches!(
                self.status,
                CommandStatus::Passed { .. } | CommandStatus::Failed { .. }
            ) {
                return Err(DrivenError::Validation(
                    "command evidence can only be attached to passed or failed commands"
                        .to_string(),
                ));
            }
            evidence.validate()?;
        }
        self.status.validate()
    }

    fn is_successful_small(&self) -> bool {
        self.class == VerificationClass::Small
            && matches!(self.status, CommandStatus::Passed { exit_code: 0 })
    }

    fn blocker(&self) -> Option<String> {
        match &self.status {
            CommandStatus::Blocked { reason } => Some(format!("command blocked: {}", reason)),
            _ => None,
        }
    }
}

impl CommandStatus {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Passed { exit_code } if *exit_code == 0 => Ok(()),
            Self::Passed { .. } => Err(DrivenError::Validation(
                "passed command proof must use exit code 0".to_string(),
            )),
            Self::Failed { exit_code } if *exit_code != 0 => Ok(()),
            Self::Failed { .. } => Err(DrivenError::Validation(
                "failed command proof must use a non-zero exit code".to_string(),
            )),
            Self::Skipped { reason } | Self::Blocked { reason } => {
                if reason.trim().is_empty() {
                    Err(DrivenError::Validation(
                        "skipped or blocked command proof requires a reason".to_string(),
                    ))
                } else {
                    Ok(())
                }
            }
            Self::NotRun => Ok(()),
        }
    }

    fn prevents_verified_outcome(&self) -> bool {
        matches!(
            self,
            Self::Failed { .. } | Self::Skipped { .. } | Self::Blocked { .. } | Self::NotRun
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileProof {
    pub path: String,
    pub purpose: String,
}

impl FileProof {
    pub fn new(path: impl Into<String>, purpose: impl Into<String>) -> Self {
        Self {
            path: normalize_path(path.into()),
            purpose: purpose.into(),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.path.trim().is_empty() {
            return Err(DrivenError::Validation(
                "file proof path cannot be empty".to_string(),
            ));
        }
        if self.purpose.trim().is_empty() {
            return Err(DrivenError::Validation(
                "file proof purpose cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum OutcomeProof {
    Verified { summary: String },
    Partial { summary: String },
    Blocked { reason: String },
}

impl OutcomeProof {
    pub fn verified(summary: impl Into<String>) -> Self {
        Self::Verified {
            summary: summary.into(),
        }
    }

    pub fn partial(summary: impl Into<String>) -> Self {
        Self::Partial {
            summary: summary.into(),
        }
    }

    pub fn blocked(reason: impl Into<String>) -> Self {
        Self::Blocked {
            reason: reason.into(),
        }
    }

    fn validate(&self) -> Result<()> {
        let text = match self {
            Self::Verified { summary } | Self::Partial { summary } => summary,
            Self::Blocked { reason } => reason,
        };
        if text.trim().is_empty() {
            return Err(DrivenError::Validation(
                "outcome proof text cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    fn blocker(&self) -> Option<String> {
        match self {
            Self::Blocked { reason } => Some(reason.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofReceipt {
    pub schema: String,
    pub receipt_id: String,
    #[serde(default)]
    pub redacted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_payload_digest: Option<String>,
    pub claim: LaneClaim,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_identity: Option<WorktreeIdentity>,
    pub summary: String,
    pub commands: Vec<CommandProof>,
    pub files: Vec<FileProof>,
    pub outcomes: Vec<OutcomeProof>,
}

impl ProofReceipt {
    pub fn new(claim: LaneClaim, summary: impl Into<String>) -> Self {
        Self {
            schema: PROOF_RECEIPT_SCHEMA.to_string(),
            receipt_id: String::new(),
            redacted: false,
            redacted_payload_digest: None,
            claim,
            worktree_identity: None,
            summary: summary.into(),
            commands: Vec::new(),
            files: Vec::new(),
            outcomes: Vec::new(),
        }
    }

    pub fn with_command(mut self, command: CommandProof) -> Self {
        self.commands.push(command);
        self
    }

    pub fn with_file(mut self, file: FileProof) -> Self {
        self.files.push(file);
        self
    }

    pub fn with_outcome(mut self, outcome: OutcomeProof) -> Self {
        self.outcomes.push(outcome);
        self
    }

    pub fn with_worktree_identity(mut self, identity: WorktreeIdentity) -> Self {
        self.worktree_identity = Some(identity);
        self
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn blockers(&self) -> Vec<String> {
        let mut blockers: Vec<String> = self
            .commands
            .iter()
            .filter_map(CommandProof::blocker)
            .collect();
        blockers.extend(self.outcomes.iter().filter_map(OutcomeProof::blocker));
        blockers
    }

    pub fn validate(&self) -> Result<()> {
        self.claim.validate()?;
        if self.schema != PROOF_RECEIPT_SCHEMA {
            return Err(DrivenError::Validation(format!(
                "proof receipt schema must be {}",
                PROOF_RECEIPT_SCHEMA
            )));
        }
        if self.summary.trim().is_empty() {
            return Err(DrivenError::Validation(
                "receipt summary cannot be empty".to_string(),
            ));
        }
        if self.redacted {
            if self.redacted_payload_digest.as_deref() != Some(&self.rendered_payload_digest()?) {
                return Err(DrivenError::Validation(
                    "redacted receipt digest does not match rendered payload".to_string(),
                ));
            }
        } else if !self.receipt_id.trim().is_empty()
            && self.receipt_id != self.computed_receipt_id()?
        {
            return Err(DrivenError::Validation(
                "receipt id does not match canonical receipt payload".to_string(),
            ));
        }
        if self.outcomes.is_empty() {
            return Err(DrivenError::Validation(
                "receipt requires at least one outcome proof".to_string(),
            ));
        }

        for command in &self.commands {
            command.validate()?;
        }
        for file in &self.files {
            file.validate()?;
        }
        for outcome in &self.outcomes {
            outcome.validate()?;
        }

        let mut has_passed_small = false;
        for command in &self.commands {
            match command.class {
                VerificationClass::Small if command.is_successful_small() => {
                    has_passed_small = true;
                }
                VerificationClass::Targeted | VerificationClass::Heavy if !has_passed_small => {
                    return Err(DrivenError::Validation(
                        "targeted or heavy verification requires a passed small command proof first"
                            .to_string(),
                    ));
                }
                _ => {}
            }
        }

        let has_verified_outcome = self
            .outcomes
            .iter()
            .any(|outcome| matches!(outcome, OutcomeProof::Verified { .. }));
        if has_verified_outcome
            && !self
                .commands
                .iter()
                .any(|command| matches!(command.status, CommandStatus::Passed { exit_code: 0 }))
        {
            return Err(DrivenError::Validation(
                "verified receipt requires at least one passed command proof".to_string(),
            ));
        }
        if has_verified_outcome
            && self
                .commands
                .iter()
                .any(|command| command.status.prevents_verified_outcome())
        {
            return Err(DrivenError::Validation(
                "verified receipt cannot include failed, blocked, or not-run commands".to_string(),
            ));
        }

        if self
            .commands
            .iter()
            .any(|command| matches!(command.status, CommandStatus::Blocked { .. }))
            && !self
                .outcomes
                .iter()
                .any(|outcome| matches!(outcome, OutcomeProof::Blocked { .. }))
        {
            return Err(DrivenError::Validation(
                "blocked command proof requires a blocked outcome".to_string(),
            ));
        }

        Ok(())
    }

    pub fn is_verified(&self) -> bool {
        self.outcomes
            .iter()
            .any(|outcome| matches!(outcome, OutcomeProof::Verified { .. }))
    }

    pub(crate) fn validate_completion_readiness(&self) -> Result<()> {
        self.validate()?;
        if self.redacted {
            return Err(DrivenError::Validation(
                "completion requires an unredacted canonical proof receipt".to_string(),
            ));
        }
        if self.receipt_id.trim().is_empty() {
            return Err(DrivenError::Validation(
                "completion requires a canonical receipt id".to_string(),
            ));
        }
        if !self.is_verified() {
            return Err(DrivenError::Validation(
                "completion requires a verified proof receipt".to_string(),
            ));
        }
        if self
            .outcomes
            .iter()
            .any(|outcome| !matches!(outcome, OutcomeProof::Verified { .. }))
        {
            return Err(DrivenError::Validation(
                "completion receipt cannot include partial or blocked outcomes".to_string(),
            ));
        }
        if self
            .commands
            .iter()
            .any(|command| command.evidence.is_none())
        {
            return Err(DrivenError::Validation(
                "completion command proofs require driven-captured evidence".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_completion_claim_subject(&self, claim: &LaneClaim) -> Result<()> {
        if self.claim.lane != claim.lane
            || self.claim.pass != claim.pass
            || self.claim.worker_id != claim.worker_id
        {
            return Err(DrivenError::Validation(
                "receipt claim does not match active lane claim".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_completion_claim_match(&self, claim: &LaneClaim) -> Result<()> {
        self.validate_completion_claim_subject(claim)?;
        if self.claim.token != claim.token {
            return Err(DrivenError::Validation(
                "receipt claim does not match active lane claim".to_string(),
            ));
        }
        Ok(())
    }

    pub fn validate_for_completion_claim(&self, claim: &LaneClaim) -> Result<()> {
        self.validate_completion_readiness()?;
        self.validate_completion_claim_match(claim)
    }

    pub fn validate_worktree_identity(&self, expected: &WorktreeIdentity) -> Result<()> {
        self.validate()?;
        let Some(actual) = &self.worktree_identity else {
            return Err(DrivenError::Validation(
                "receipt worktree identity is required".to_string(),
            ));
        };
        if actual != expected {
            return Err(DrivenError::Validation(
                "receipt worktree identity does not match active lane claim".to_string(),
            ));
        }
        Ok(())
    }

    pub fn receipt_id(&self) -> Result<String> {
        self.validate()?;
        self.computed_receipt_id()
    }

    fn computed_receipt_id(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.receipt_id.clear();
        canonical.redacted = false;
        canonical.redacted_payload_digest = None;
        let bytes = serde_json::to_vec(&canonical)
            .map_err(|e| DrivenError::Format(format!("failed to render receipt JSON: {}", e)))?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }

    fn rendered_payload_digest(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.receipt_id.clear();
        canonical.redacted_payload_digest = None;
        let bytes = serde_json::to_vec(&canonical)
            .map_err(|e| DrivenError::Format(format!("failed to render receipt JSON: {}", e)))?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }

    pub fn render(&self, format: ReceiptFormat) -> Result<String> {
        self.validate()?;
        let canonical_id = self.receipt_id()?;
        let mut receipt = self.redacted_for_render();
        receipt.receipt_id = canonical_id;
        receipt.redacted = true;
        receipt.redacted_payload_digest = Some(receipt.rendered_payload_digest()?);

        match format {
            ReceiptFormat::Json => serde_json::to_string_pretty(&receipt)
                .map(|mut json| {
                    json.push('\n');
                    json
                })
                .map_err(|e| DrivenError::Format(format!("failed to render receipt JSON: {}", e))),
            ReceiptFormat::Markdown => Ok(receipt.render_markdown()),
        }
    }

    fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# DX Proof Receipt: {}\n\n", self.receipt_id));
        out.push_str(&format!("Status: {}\n", receipt_status_label(self)));
        out.push_str(&format!(
            "Scope: lane {} / pass {} / worker {}\n\n",
            self.claim.lane, self.claim.pass, self.claim.worker_id
        ));

        if let Some(identity) = &self.worktree_identity {
            out.push_str("## Worktree\n");
            out.push_str(&format!("- Kind: {:?}\n", identity.kind));
            out.push_str(&format!(
                "- Root: {}\n",
                escape_markdown_text(&identity.input_root.display().to_string())
            ));
            if let Some(branch) = &identity.branch {
                out.push_str(&format!("- Branch: {}\n", escape_markdown_text(branch)));
            }
            if let Some(remote) = &identity.remote {
                out.push_str(&format!("- Remote: {}\n", escape_markdown_text(remote)));
            }
            out.push('\n');
        }

        out.push_str("## Summary\n");
        out.push_str(&format!("- {}\n\n", escape_markdown_text(&self.summary)));

        out.push_str("## Commands\n");
        out.push_str("| # | Class | Status | Evidence | Command |\n");
        out.push_str("|---|---|---|---|---|\n");
        for (index, command) in self.commands.iter().enumerate() {
            out.push_str(&format!(
                "| {} | {:?} | {} | {} | {} |\n",
                index + 1,
                command.class,
                command_status_label(&command.status),
                command_evidence_label(command.evidence.as_ref()),
                escape_table_cell(&command.command)
            ));
        }
        if self.commands.is_empty() {
            out.push_str("| - | - | not_run | - | - |\n");
        }
        out.push('\n');

        out.push_str("## Files\n");
        out.push_str("| Path | Purpose |\n");
        out.push_str("|---|---|\n");
        for file in &self.files {
            out.push_str(&format!(
                "| {} | {} |\n",
                escape_table_cell(&file.path),
                escape_table_cell(&file.purpose)
            ));
        }
        if self.files.is_empty() {
            out.push_str("| - | - |\n");
        }
        out.push('\n');

        out.push_str("## Outcomes\n");
        for outcome in &self.outcomes {
            out.push_str(&format!("- {}\n", outcome_label(outcome)));
        }
        if self.outcomes.is_empty() {
            out.push_str("- No outcome proof recorded.\n");
        }
        out.push('\n');

        out.push_str("## Digest\n");
        out.push_str("- Algorithm: blake3\n");
        out.push_str("- Canonicalization: driven_receipt_v1\n");
        if self.redacted {
            out.push_str(&format!("- Canonical receipt: `{}`\n", self.receipt_id));
            if let Some(redacted_payload_digest) = &self.redacted_payload_digest {
                out.push_str(&format!(
                    "- Redacted payload: `{}`\n",
                    redacted_payload_digest
                ));
            }
        } else {
            out.push_str(&format!("- Value: `{}`\n", self.receipt_id));
        }
        out
    }

    fn redacted_for_render(&self) -> Self {
        let mut receipt = self.clone();
        receipt.receipt_id.clear();
        receipt.redacted = false;
        receipt.redacted_payload_digest = None;
        receipt.summary = redact_secrets(&receipt.summary);
        receipt.claim.scope = redact_secrets(&receipt.claim.scope);
        receipt.claim.refresh_token();
        if let Some(identity) = &mut receipt.worktree_identity {
            identity.input_root = redacted_path(&identity.input_root);
            identity.worktree_root = identity
                .worktree_root
                .as_ref()
                .map(|path| redacted_path(path));
            identity.git_dir = identity.git_dir.as_ref().map(|path| redacted_path(path));
            identity.common_dir = identity.common_dir.as_ref().map(|path| redacted_path(path));
            identity.superproject_root = identity
                .superproject_root
                .as_ref()
                .map(|path| redacted_path(path));
            identity.branch = identity
                .branch
                .as_ref()
                .map(|branch| redact_secrets(branch));
            identity.remote = identity
                .remote
                .as_ref()
                .map(|remote| redact_secrets(remote));
        }

        for subagent in &mut receipt.claim.subagents {
            subagent.task = redact_secrets(&subagent.task);
        }
        for command in &mut receipt.commands {
            command.command = redact_secrets(&command.command);
            if let Some(evidence) = &mut command.evidence {
                evidence.cwd = redact_secrets(&evidence.cwd);
            }
            match &mut command.status {
                CommandStatus::Skipped { reason } | CommandStatus::Blocked { reason } => {
                    *reason = redact_secrets(reason);
                }
                _ => {}
            }
        }
        for file in &mut receipt.files {
            file.path = redact_secrets(&file.path);
            file.purpose = redact_secrets(&file.purpose);
        }
        for outcome in &mut receipt.outcomes {
            match outcome {
                OutcomeProof::Verified { summary } | OutcomeProof::Partial { summary } => {
                    *summary = redact_secrets(summary);
                }
                OutcomeProof::Blocked { reason } => {
                    *reason = redact_secrets(reason);
                }
            }
        }
        receipt
    }
}

fn normalize_path(path: String) -> String {
    path.replace('\\', "/")
}

fn redacted_path(path: &Path) -> std::path::PathBuf {
    Path::new(&redact_secrets(&path.display().to_string())).to_path_buf()
}

fn escape_table_cell(value: &str) -> String {
    escape_markdown_text(value)
}

pub(crate) fn escape_markdown_text(value: &str) -> String {
    value
        .replace('\r', "")
        .replace('|', "\\|")
        .replace('\n', "<br>")
}

fn command_status_label(status: &CommandStatus) -> String {
    match status {
        CommandStatus::Passed { exit_code } => format!("passed({})", exit_code),
        CommandStatus::Failed { exit_code } => format!("failed({})", exit_code),
        CommandStatus::Skipped { reason } => format!("skipped: {}", escape_markdown_text(reason)),
        CommandStatus::Blocked { reason } => format!("blocked: {}", escape_markdown_text(reason)),
        CommandStatus::NotRun => "not_run".to_string(),
    }
}

fn command_evidence_label(evidence: Option<&CommandEvidence>) -> String {
    match evidence {
        Some(evidence) => format!(
            "cwd: {}; stdout: {}; stderr: {}",
            escape_markdown_text(&evidence.cwd),
            evidence.stdout_digest,
            evidence.stderr_digest
        ),
        None => "-".to_string(),
    }
}

fn outcome_label(outcome: &OutcomeProof) -> String {
    match outcome {
        OutcomeProof::Verified { summary } => {
            format!("verified: {}", escape_markdown_text(summary))
        }
        OutcomeProof::Partial { summary } => format!("partial: {}", escape_markdown_text(summary)),
        OutcomeProof::Blocked { reason } => format!("blocked: {}", escape_markdown_text(reason)),
    }
}

fn receipt_status_label(receipt: &ProofReceipt) -> &'static str {
    if !receipt.blockers().is_empty() {
        "blocked"
    } else if receipt
        .outcomes
        .iter()
        .any(|outcome| matches!(outcome, OutcomeProof::Partial { .. }))
        || receipt.commands.iter().any(|command| {
            matches!(
                command.status,
                CommandStatus::Failed { .. }
                    | CommandStatus::Skipped { .. }
                    | CommandStatus::NotRun
            )
        })
    {
        "partial"
    } else {
        "verified"
    }
}
