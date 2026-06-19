//! CLI helpers for DX lane/pass strategy commands.

use crate::Result;
use crate::strategy::artifacts::read_receipt_artifact;
use crate::strategy::{
    LaneClaim, LaneId, LanePassAssignment, LanePassConfig, LanePassStore, OutcomeProof, PassNumber,
    ProofReceipt, ReceiptFormat, WorkerId, WorktreeIsolationPlan, WorktreeMetadata,
    detect_worktree_metadata,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct StrategyCommand;

#[derive(Debug, Clone, Default)]
pub struct StrategyStateOptions {
    pub cycle_lanes: bool,
    pub project_root: Option<PathBuf>,
    pub strict_isolation: bool,
    pub handoff_required_for_next: bool,
}

impl StrategyStateOptions {
    pub fn with_lane_cycling(mut self, cycle_lanes: bool) -> Self {
        self.cycle_lanes = cycle_lanes;
        self
    }

    pub fn with_project_root(mut self, project_root: impl Into<PathBuf>) -> Self {
        self.project_root = Some(project_root.into());
        self
    }

    pub fn with_strict_isolation(mut self, strict_isolation: bool) -> Self {
        self.strict_isolation = strict_isolation;
        self
    }

    pub fn with_handoff_required_for_next(mut self, required: bool) -> Self {
        self.handoff_required_for_next = required;
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategyClaimOutput {
    pub claim: LaneClaim,
    pub receipt_id: String,
    pub handoff: crate::strategy::NextPassHandoff,
    pub worktree: WorktreeMetadata,
    pub worktree_plan: WorktreeIsolationPlan,
    pub recommended_small_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategyWorktreeInspection {
    pub worktree: WorktreeMetadata,
    pub worktree_plan: WorktreeIsolationPlan,
}

impl StrategyCommand {
    pub fn inspect_worktree(path: &Path, json: bool) -> Result<String> {
        let metadata = detect_worktree_metadata(path);
        let plan = WorktreeIsolationPlan::from_metadata(metadata.clone());
        let inspection = StrategyWorktreeInspection {
            worktree: metadata,
            worktree_plan: plan,
        };
        if json {
            return serde_json::to_string_pretty(&inspection)
                .map(|mut output| {
                    output.push('\n');
                    output
                })
                .map_err(|e| crate::DrivenError::Format(format!("failed to render JSON: {}", e)));
        }

        Ok(format!(
            "Worktree kind: {:?}\nRoot: {}\nBranch: {}\nDirty: {}\nDecision: {:?}\n",
            inspection.worktree.kind,
            inspection
                .worktree
                .worktree_root
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "not a repository".to_string()),
            inspection
                .worktree
                .branch
                .as_deref()
                .unwrap_or("detached-or-none"),
            inspection
                .worktree
                .is_dirty
                .map(|dirty| dirty.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            inspection.worktree_plan.creation_decision
        ))
    }

    pub fn claim(
        project_root: &Path,
        lane: u8,
        pass: u32,
        worker_id: &str,
        scope: &str,
        next_action: &str,
        json: bool,
    ) -> Result<String> {
        let worker = WorkerId::new(worker_id)?;
        let claim = LaneClaim::new(LaneId::new(lane)?, PassNumber::new(pass)?, worker, scope);
        let receipt = ProofReceipt::new(claim.clone(), "lane/pass claim modeled").with_outcome(
            OutcomeProof::partial(
                "claim created; run verification commands before closing the pass",
            ),
        );
        let receipt_id = receipt.receipt_id()?;
        let handoff = claim.next_pass_handoff(receipt.clone(), next_action)?;

        let worktree = detect_worktree_metadata(project_root);
        let worktree_plan = WorktreeIsolationPlan::from_metadata(worktree.clone());
        let output = StrategyClaimOutput {
            claim,
            receipt_id,
            handoff,
            worktree,
            worktree_plan,
            recommended_small_commands: vec![
                "git status --short --branch".to_string(),
                "rg -n \"(<{7}|>{7}|={7})\"".to_string(),
                "cargo fmt --check".to_string(),
            ],
        };

        if json {
            serde_json::to_string_pretty(&output)
                .map(|mut output| {
                    output.push('\n');
                    output
                })
                .map_err(|e| crate::DrivenError::Format(format!("failed to render JSON: {}", e)))
        } else {
            render_claim_markdown(&output, &receipt)
        }
    }

    pub fn peek_state(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        json: bool,
    ) -> Result<String> {
        Self::peek_state_with_options(
            state_dir,
            scope,
            max_lanes,
            max_passes,
            json,
            StrategyStateOptions::default(),
        )
    }

    pub fn peek_state_with_options(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        json: bool,
        options: StrategyStateOptions,
    ) -> Result<String> {
        let store = LanePassStore::new(
            state_config(state_dir, scope, &options)
                .with_max_lanes(max_lanes)
                .with_max_passes(max_passes),
        )?;
        render_assignment(&store.peek_next_claim()?, json)
    }

    pub fn claim_state(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        json: bool,
    ) -> Result<String> {
        Self::claim_state_with_options(
            state_dir,
            scope,
            max_lanes,
            max_passes,
            worker_id,
            json,
            StrategyStateOptions::default(),
        )
    }

    pub fn claim_state_with_options(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        json: bool,
        options: StrategyStateOptions,
    ) -> Result<String> {
        let config = state_config(state_dir, scope, &options)
            .with_max_lanes(max_lanes)
            .with_max_passes(max_passes);
        enforce_strict_isolation(&config, &options)?;
        let store = LanePassStore::new(config)?;
        render_assignment(&store.claim(WorkerId::new(worker_id)?)?, json)
    }

    pub fn next_state(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        json: bool,
    ) -> Result<String> {
        Self::next_state_with_options(
            state_dir,
            scope,
            max_lanes,
            max_passes,
            worker_id,
            json,
            StrategyStateOptions::default(),
        )
    }

    pub fn next_state_with_options(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        json: bool,
        options: StrategyStateOptions,
    ) -> Result<String> {
        let config = state_config(state_dir, scope, &options)
            .with_max_lanes(max_lanes)
            .with_max_passes(max_passes);
        enforce_strict_isolation(&config, &options)?;
        let store = LanePassStore::new(config)?;
        render_assignment(&store.next_pass(&WorkerId::new(worker_id)?)?, json)
    }

    pub fn next_state_with_handoff_options(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        receipt_path: &Path,
        next_action: &str,
        json: bool,
        options: StrategyStateOptions,
    ) -> Result<String> {
        let config = state_config(state_dir, scope, &options)
            .with_max_lanes(max_lanes)
            .with_max_passes(max_passes);
        enforce_strict_isolation(&config, &options)?;
        let store = LanePassStore::new(config)?;
        let worker = WorkerId::new(worker_id)?;
        let receipt = read_receipt_artifact(receipt_path)?;
        render_assignment(
            &store.next_pass_with_handoff(&worker, &receipt, next_action)?,
            json,
        )
    }

    pub fn release_state(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        json: bool,
    ) -> Result<String> {
        Self::release_state_with_options(
            state_dir,
            scope,
            max_lanes,
            max_passes,
            worker_id,
            json,
            StrategyStateOptions::default(),
        )
    }

    pub fn release_state_with_options(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        json: bool,
        options: StrategyStateOptions,
    ) -> Result<String> {
        let config = state_config(state_dir, scope, &options)
            .with_max_lanes(max_lanes)
            .with_max_passes(max_passes);
        enforce_strict_isolation(&config, &options)?;
        let store = LanePassStore::new(config)?;
        render_assignment(&store.release_lane(&WorkerId::new(worker_id)?)?, json)
    }

    pub fn complete_state(
        _state_dir: PathBuf,
        _scope: &str,
        _max_lanes: u8,
        _max_passes: u32,
        _worker_id: &str,
        _json: bool,
    ) -> Result<String> {
        Err(crate::DrivenError::Validation(
            "completion requires a canonical proof receipt; use complete_state_with_receipt"
                .to_string(),
        ))
    }

    pub fn complete_state_with_receipt(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        receipt_path: &Path,
        json: bool,
    ) -> Result<String> {
        Self::complete_state_with_receipt_options(
            state_dir,
            scope,
            max_lanes,
            max_passes,
            worker_id,
            receipt_path,
            json,
            StrategyStateOptions::default(),
        )
    }

    pub fn complete_state_with_receipt_options(
        state_dir: PathBuf,
        scope: &str,
        max_lanes: u8,
        max_passes: u32,
        worker_id: &str,
        receipt_path: &Path,
        json: bool,
        options: StrategyStateOptions,
    ) -> Result<String> {
        let config = state_config(state_dir, scope, &options)
            .with_max_lanes(max_lanes)
            .with_max_passes(max_passes);
        enforce_strict_isolation(&config, &options)?;
        let store = LanePassStore::new(config)?;
        let worker = WorkerId::new(worker_id)?;
        let receipt = read_receipt_artifact(receipt_path)?;
        render_assignment(&store.complete_pass_with_receipt(&worker, &receipt)?, json)
    }
}

pub(super) fn render_receipt(receipt: &ProofReceipt, json: bool) -> Result<String> {
    if json {
        return receipt.render(ReceiptFormat::Json);
    }

    receipt.render(ReceiptFormat::Markdown)
}

fn render_claim_markdown(output: &StrategyClaimOutput, receipt: &ProofReceipt) -> Result<String> {
    let mut markdown = String::new();
    markdown.push_str("# DX Lane/Pass Claim\n\n");
    markdown.push_str(&format!("- Lane: {}\n", output.claim.lane));
    markdown.push_str(&format!("- Pass: {}\n", output.claim.pass));
    markdown.push_str(&format!("- Worker: {}\n", output.claim.worker_id));
    markdown.push_str(&format!(
        "- Scope: {}\n",
        escape_markdown_text(&output.claim.scope)
    ));
    markdown.push_str(&format!("- Next pass: {}\n", output.handoff.next_pass));
    markdown.push_str(&format!(
        "- Next action: {}\n",
        escape_markdown_text(&output.handoff.next_action)
    ));
    markdown.push_str(&format!("- Worktree kind: {:?}\n\n", output.worktree.kind));
    markdown.push_str(&format!(
        "- Worktree decision: {:?}\n",
        output.worktree_plan.creation_decision
    ));
    for blocker in &output.worktree_plan.blockers {
        markdown.push_str(&format!(
            "- Worktree blocker: {}\n",
            escape_markdown_text(blocker)
        ));
    }
    markdown.push('\n');
    markdown.push_str("## Small Commands First\n");
    for command in &output.recommended_small_commands {
        markdown.push_str(&format!("- `{}`\n", command));
    }
    markdown.push('\n');
    markdown.push_str(&receipt.render(ReceiptFormat::Markdown)?);
    Ok(markdown)
}

fn render_assignment(assignment: &LanePassAssignment, json: bool) -> Result<String> {
    if json {
        return serde_json::to_string_pretty(assignment)
            .map(|mut output| {
                output.push('\n');
                output
            })
            .map_err(|e| crate::DrivenError::Format(format!("failed to render JSON: {}", e)));
    }

    Ok(format!(
        "# DX Lane/Pass Assignment\n\n- Status: {:?}\n- Scope: {}\n- Lane: {}\n- Pass: {}\n- Worker: {}\n- Counter: {}\n- Claims: {}\n",
        assignment.status,
        escape_markdown_text(&assignment.scope),
        assignment.lane,
        assignment.pass,
        assignment.worker_id,
        assignment.paths.counter_path.display(),
        assignment.paths.claims_path.display()
    ))
}

pub(super) fn state_config(
    state_dir: PathBuf,
    scope: &str,
    options: &StrategyStateOptions,
) -> LanePassConfig {
    let config = LanePassConfig::new(state_dir, scope);
    let config = if let Some(project_root) = &options.project_root {
        config.with_project_root(project_root.clone())
    } else {
        match std::env::current_dir() {
            Ok(project_root) => config.with_project_root(project_root),
            Err(_) => config,
        }
    };
    config
        .with_lane_cycling(options.cycle_lanes)
        .with_handoff_required_for_next(options.handoff_required_for_next)
}

pub(super) fn enforce_strict_isolation(
    config: &LanePassConfig,
    options: &StrategyStateOptions,
) -> Result<()> {
    if !options.strict_isolation {
        return Ok(());
    }
    let project_root = match &config.project_root {
        Some(project_root) => project_root.clone(),
        None => std::env::current_dir().map_err(crate::DrivenError::Io)?,
    };
    let metadata = detect_worktree_metadata(&project_root);
    let plan = WorktreeIsolationPlan::from_metadata(metadata);
    if plan.native_isolation_detected && plan.blockers.is_empty() && plan.warnings.is_empty() {
        return Ok(());
    }
    Err(crate::DrivenError::Validation(format!(
        "strict isolation requires a clean isolated worktree; decision={:?}; blockers={}; warnings={}",
        plan.creation_decision,
        plan.blockers.join("; "),
        plan.warnings.join("; ")
    )))
}

pub(super) fn unix_seconds_now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|e| crate::DrivenError::Validation(format!("system time is before epoch: {}", e)))
}

pub(super) fn command_display(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .map(|arg| display_command_arg(&arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_command_arg(arg: &str) -> String {
    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | '\\' | ':'))
    {
        return arg.to_string();
    }
    format!("{:?}", arg)
}

fn escape_markdown_text(value: &str) -> String {
    value
        .replace('\r', "")
        .replace('|', "\\|")
        .replace('\n', "<br>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_json_contains_lane_worker_and_next_action() {
        let temp = tempfile::tempdir().unwrap();
        let output = StrategyCommand::claim(
            temp.path(),
            2,
            1,
            "worker-cli",
            "cli strategy",
            "continue with receipts",
            true,
        )
        .unwrap();

        assert!(output.contains("\"lane\": 2"));
        assert!(output.contains("\"worker_id\": \"worker-cli\""));
        assert!(output.contains("continue with receipts"));
    }

    #[test]
    fn claim_state_json_preserves_lane_across_next_pass() {
        let temp = tempfile::tempdir().unwrap();
        let state_dir = temp.path().join("state");

        let claimed =
            StrategyCommand::claim_state(state_dir.clone(), "cli state", 30, 3, "worker-cli", true)
                .unwrap();
        let next =
            StrategyCommand::next_state(state_dir, "cli state", 30, 3, "worker-cli", true).unwrap();

        assert!(claimed.contains("\"lane\": 1"));
        assert!(claimed.contains("\"pass\": 1"));
        assert!(next.contains("\"lane\": 1"));
        assert!(next.contains("\"pass\": 2"));
    }
}
