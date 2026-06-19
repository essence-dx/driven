use driven::cli::{StrategyCommand, StrategyStateOptions};
use driven::strategy::{
    LanePassAssignment, ProofReceipt, ReceiptFormat, VerificationClass, plan_worktree_isolation,
};
use std::fs;

#[cfg(windows)]
fn small_echo_command() -> (&'static str, Vec<String>) {
    ("cmd", vec!["/C".into(), "echo driven-proof".into()])
}

#[cfg(not(windows))]
fn small_echo_command() -> (&'static str, Vec<String>) {
    ("sh", vec!["-c".into(), "printf driven-proof".into()])
}

fn main() -> driven::Result<()> {
    let project_root = std::env::current_dir()?;
    let state_dir =
        std::env::temp_dir().join(format!("driven-lane-pass-example-{}", std::process::id()));
    let worker_id = "friday-worker-01";
    let scope = "strategy engine";
    let options = StrategyStateOptions::default().with_project_root(project_root.clone());

    let worktree_plan = plan_worktree_isolation(&project_root);
    let claimed_json = StrategyCommand::claim_state_with_options(
        state_dir.clone(),
        scope,
        30,
        3,
        worker_id,
        true,
        options.clone(),
    )?;
    let assignment: LanePassAssignment = serde_json::from_str(&claimed_json).map_err(|error| {
        driven::DrivenError::Parse(format!("failed to parse assignment JSON: {}", error))
    })?;

    let receipt_path = state_dir.join("receipt.json");
    let (program, args) = small_echo_command();
    let _rendered_receipt_json = StrategyCommand::receipt_state_with_options(
        state_dir.clone(),
        scope,
        30,
        3,
        worker_id,
        "worker claimed lane/pass with captured small-command proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        options.clone(),
    )?;
    let receipt_json = fs::read_to_string(&receipt_path).map_err(driven::DrivenError::Io)?;
    let receipt: ProofReceipt = serde_json::from_str(&receipt_json).map_err(|error| {
        driven::DrivenError::Parse(format!("failed to parse receipt JSON: {}", error))
    })?;
    let handoff = assignment
        .claim
        .next_pass_handoff(receipt.clone(), "continue with targeted lane/pass tests")?;

    let advanced_json = StrategyCommand::next_state_with_handoff_options(
        state_dir,
        scope,
        30,
        3,
        worker_id,
        &receipt_path,
        "continue with targeted lane/pass tests",
        true,
        options,
    )?;
    let advanced: LanePassAssignment = serde_json::from_str(&advanced_json).map_err(|error| {
        driven::DrivenError::Parse(format!("failed to parse advanced JSON: {}", error))
    })?;

    println!("worktree decision: {:?}", worktree_plan.creation_decision);
    println!("claimed lane/pass: {}/{}", assignment.lane, assignment.pass);
    println!("advanced lane/pass: {}/{}", advanced.lane, advanced.pass);
    println!("receipt id: {}", receipt.receipt_id);
    println!("receipt path: {}", receipt_path.display());
    if let Some(handoff_path) = &advanced.handoff_path {
        println!("handoff path: {}", handoff_path.display());
    }
    println!("next action: {}", handoff.next_action);
    println!("{}", receipt.render(ReceiptFormat::Markdown)?);
    Ok(())
}
