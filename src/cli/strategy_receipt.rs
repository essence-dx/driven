use crate::Result;
use crate::cli::command_capture::{
    CapturedCommandStatus, CommandExecutionPolicy, run_command_with_policy,
};
use crate::cli::strategy::{
    StrategyCommand, StrategyStateOptions, command_display, enforce_strict_isolation,
    render_receipt, state_config, unix_seconds_now,
};
use crate::strategy::artifacts::write_receipt_artifact;
use crate::strategy::{
    ClaimStatus, CommandProof, CommandStatus, LanePassStore, OutcomeProof, VerificationClass,
    WorkerId,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrategyReceiptExecutionOptions {
    pub timeout_ms: u64,
    pub max_output_bytes: u64,
}

impl Default for StrategyReceiptExecutionOptions {
    fn default() -> Self {
        Self {
            timeout_ms: 300_000,
            max_output_bytes: 1_048_576,
        }
    }
}

impl StrategyReceiptExecutionOptions {
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn with_max_output_bytes(mut self, max_output_bytes: u64) -> Self {
        self.max_output_bytes = max_output_bytes;
        self
    }

    fn validate(&self) -> Result<()> {
        if self.timeout_ms == 0 {
            return Err(crate::DrivenError::Validation(
                "receipt command timeout must be at least 1 ms".to_string(),
            ));
        }
        if self.max_output_bytes == 0 {
            return Err(crate::DrivenError::Validation(
                "receipt command output limit must be at least 1 byte".to_string(),
            ));
        }
        Ok(())
    }
}

impl StrategyCommand {
    pub fn receipt_state_with_options(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        summary: &str,
        class: VerificationClass,
        program: &str,
        args: &[String],
        receipt_path: &Path,
        json: bool,
        options: StrategyStateOptions,
    ) -> Result<String> {
        Self::receipt_state_with_execution_options(
            state_dir,
            scope,
            max_lanes,
            max_passes,
            worker_id,
            summary,
            class,
            program,
            args,
            receipt_path,
            json,
            StrategyReceiptExecutionOptions::default(),
            options,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn receipt_state_with_execution_options(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        summary: &str,
        class: VerificationClass,
        program: &str,
        args: &[String],
        receipt_path: &Path,
        json: bool,
        execution: StrategyReceiptExecutionOptions,
        options: StrategyStateOptions,
    ) -> Result<String> {
        execution.validate()?;
        let config = state_config(state_dir, scope, &options)
            .with_max_lanes(max_lanes)
            .with_max_passes(max_passes);
        enforce_strict_isolation(&config, &options)?;
        let cwd = config
            .project_root
            .clone()
            .unwrap_or(std::env::current_dir().map_err(crate::DrivenError::Io)?);
        let store = LanePassStore::new(config)?;
        let worker = WorkerId::new(worker_id)?;
        let assignment = store.worker_assignment(&worker)?.ok_or_else(|| {
            crate::DrivenError::Validation(format!("worker {} has no lane claim", worker))
        })?;
        if assignment.claim.status != ClaimStatus::Claimed {
            return Err(crate::DrivenError::Validation(format!(
                "worker {} lane claim is not active",
                worker
            )));
        }
        store.validate_current_worktree_identity(&assignment)?;

        let command_text = command_display(program, args);
        let started_unix_seconds = unix_seconds_now()?;
        let captured = run_command_with_policy(
            program,
            args,
            &cwd,
            &CommandExecutionPolicy {
                timeout_ms: execution.timeout_ms,
                max_output_bytes: execution.max_output_bytes,
            },
            started_unix_seconds,
        )?;

        let (command, mut outcome) = match captured.status {
            CapturedCommandStatus::Exited { exit_code } => {
                let command = CommandProof::observed_captured(
                    command_text.clone(),
                    class,
                    exit_code,
                    &cwd,
                    captured.stdout.digest,
                    captured.stderr.digest,
                    captured.stdout.bytes,
                    captured.stderr.bytes,
                    captured.stdout.truncated,
                    captured.stderr.truncated,
                    execution.max_output_bytes,
                    captured.duration_ms,
                    captured.started_unix_seconds,
                    captured.finished_unix_seconds,
                )?;
                let outcome = if exit_code == 0 {
                    OutcomeProof::verified(format!(
                        "{} passed with exit code {}",
                        command_text, exit_code
                    ))
                } else {
                    OutcomeProof::partial(format!(
                        "{} failed with exit code {}",
                        command_text, exit_code
                    ))
                };
                (command, outcome)
            }
            CapturedCommandStatus::TimedOut => {
                let reason = format!(
                    "{} timed out after {} ms",
                    command_text, execution.timeout_ms
                );
                (
                    CommandProof::new(
                        command_text,
                        class,
                        CommandStatus::Blocked {
                            reason: reason.clone(),
                        },
                    ),
                    OutcomeProof::blocked(reason),
                )
            }
        };
        if let Some(reason) = stale_receipt_reason(&store, &worker, &assignment)? {
            outcome = OutcomeProof::blocked(reason);
        }

        let mut receipt = crate::strategy::ProofReceipt::new(assignment.claim, summary)
            .with_worktree_identity(assignment.worktree.identity())
            .with_command(command)
            .with_outcome(outcome);
        receipt.receipt_id = receipt.receipt_id()?;
        write_receipt_artifact(receipt_path, &receipt)?;
        render_receipt(&receipt, json)
    }
}

fn stale_receipt_reason(
    store: &LanePassStore,
    worker: &WorkerId,
    assignment: &crate::strategy::LanePassAssignment,
) -> Result<Option<String>> {
    if store
        .validate_current_worktree_identity(assignment)
        .is_err()
    {
        return Ok(Some(format!(
            "worker {} worktree identity changed before receipt write",
            worker
        )));
    }
    let Some(current) = store.worker_assignment(worker)? else {
        return Ok(Some(format!(
            "worker {} lane claim changed before receipt write",
            worker
        )));
    };
    if current.claim.status != ClaimStatus::Claimed {
        return Ok(Some(format!(
            "worker {} lane claim changed before receipt write",
            worker
        )));
    }
    if current.claim.token != assignment.claim.token
        || current.lane != assignment.lane
        || current.pass != assignment.pass
    {
        return Ok(Some(format!(
            "worker {} lane claim changed before receipt write",
            worker
        )));
    }
    if !current.worktree.has_same_identity(&assignment.worktree) {
        return Ok(Some(format!(
            "worker {} worktree identity changed before receipt write",
            worker
        )));
    }
    Ok(None)
}
