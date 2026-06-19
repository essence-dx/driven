use driven::strategy::{
    ClaimStatus, CommandProof, CommandStatus, LanePassAssignment, LanePassAssignmentStatus,
    NextPassHandoff, OutcomeProof, ProofReceipt, VerificationClass, WorktreeIdentity,
};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn driven() -> Command {
    Command::new(env!("CARGO_BIN_EXE_driven"))
}

fn successful_stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "expected command to succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn assert_next_handoff_pair_rejected(output: Output) {
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(stderr.contains("requires both --receipt and --next-action"));
}

#[cfg(windows)]
fn noisy_stdout_command() -> (&'static str, Vec<String>) {
    (
        "powershell",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            "$line = -join [char[]](100,114,105,118,101,110,45,112,114,111,111,102,45,108,105,110,101); 1..40 | ForEach-Object { Write-Output $line }".into(),
        ],
    )
}

#[cfg(not(windows))]
fn noisy_stdout_command() -> (&'static str, Vec<String>) {
    (
        "sh",
        vec![
            "-c".into(),
            "line=$(printf '\\144\\162\\151\\166\\145\\156\\055\\160\\162\\157\\157\\146\\055\\154\\151\\156\\145'); i=0; while [ $i -lt 40 ]; do printf '%s\\n' \"$line\"; i=$((i+1)); done".into(),
        ],
    )
}

#[cfg(windows)]
fn slow_command() -> (&'static str, Vec<String>) {
    (
        "powershell",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            "Start-Sleep -Seconds 1".into(),
        ],
    )
}

#[cfg(not(windows))]
fn slow_command() -> (&'static str, Vec<String>) {
    ("sh", vec!["-c".into(), "sleep 1".into()])
}

#[test]
fn strategy_binary_inspect_worktree_json_includes_isolation_plan() {
    let temp = tempfile::tempdir().unwrap();
    let inspect_path = temp.path().to_string_lossy().to_string();
    let output = driven()
        .args(["strategy", "inspect-worktree", &inspect_path, "--json"])
        .output()
        .unwrap();
    let stdout = successful_stdout(output);
    let value: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(value["worktree"]["kind"], "not_repository");
    assert_eq!(value["worktree_plan"]["creation_decision"], "blocked");
    assert!(value["worktree_plan"]["blockers"].is_array());
}

#[test]
fn strategy_binary_completes_active_claim_with_verified_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let state_dir_arg = state_dir.to_string_lossy().to_string();
    let claim_output = driven()
        .args([
            "strategy",
            "claim-state",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary state",
            "--json",
        ])
        .output()
        .unwrap();
    let claim_stdout = successful_stdout(claim_output);
    let assignment: LanePassAssignment = serde_json::from_str(&claim_stdout).unwrap();

    let mut receipt = ProofReceipt::new(assignment.claim.clone(), "binary completion proof")
        .with_worktree_identity(assignment.worktree.identity())
        .with_command(
            CommandProof::observed(
                "cargo fmt --check",
                VerificationClass::Small,
                0,
                Path::new("G:/Dx/driven"),
                b"ok\n",
                b"",
                1_000,
                1_001,
            )
            .unwrap(),
        )
        .with_outcome(OutcomeProof::verified("small command passed"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let receipt_arg = receipt_path.to_string_lossy().to_string();
    fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();

    let complete_output = driven()
        .args([
            "strategy",
            "complete",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary state",
            "--receipt",
            &receipt_arg,
            "--json",
        ])
        .output()
        .unwrap();
    let complete_stdout = successful_stdout(complete_output);
    let completed: LanePassAssignment = serde_json::from_str(&complete_stdout).unwrap();

    assert_eq!(completed.status, LanePassAssignmentStatus::Completed);
    assert_eq!(completed.claim.status, ClaimStatus::Completed);
    assert_eq!(
        completed.receipt_id.as_deref(),
        Some(receipt.receipt_id.as_str())
    );
}

#[test]
fn strategy_binary_receipt_records_output_cap_policy() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let state_dir_arg = state_dir.to_string_lossy().to_string();
    let receipt_path = temp.path().join("receipt.json");
    let receipt_arg = receipt_path.to_string_lossy().to_string();
    let claim_output = driven()
        .args([
            "strategy",
            "claim-state",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin-cap",
            "--scope",
            "binary receipt cap",
            "--json",
        ])
        .output()
        .unwrap();
    successful_stdout(claim_output);
    let (program, args) = noisy_stdout_command();

    let mut command = driven();
    command.args([
        "strategy",
        "receipt",
        "--state-dir",
        &state_dir_arg,
        "--worker-id",
        "worker-bin-cap",
        "--scope",
        "binary receipt cap",
        "--summary",
        "binary output cap proof",
        "--class",
        "small",
        "--out",
        &receipt_arg,
        "--max-output-bytes",
        "64",
        "--json",
        "--",
    ]);
    command.arg(program).args(args);
    let stdout = successful_stdout(command.output().unwrap());
    let receipt: ProofReceipt = serde_json::from_str(&stdout).unwrap();
    let evidence = receipt.commands[0].evidence.as_ref().unwrap();

    assert_eq!(evidence.output_limit_bytes, Some(64));
    assert!(evidence.stdout_bytes > 64);
    assert!(evidence.stdout_truncated);
    assert!(!stdout.contains("driven-proof-line"));
    assert!(receipt_path.exists());
}

#[test]
fn strategy_binary_receipt_timeout_writes_blocked_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let state_dir_arg = state_dir.to_string_lossy().to_string();
    let receipt_path = temp.path().join("receipt.json");
    let receipt_arg = receipt_path.to_string_lossy().to_string();
    let claim_output = driven()
        .args([
            "strategy",
            "claim-state",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin-timeout",
            "--scope",
            "binary receipt timeout",
            "--json",
        ])
        .output()
        .unwrap();
    successful_stdout(claim_output);
    let (program, args) = slow_command();

    let mut command = driven();
    command.args([
        "strategy",
        "receipt",
        "--state-dir",
        &state_dir_arg,
        "--worker-id",
        "worker-bin-timeout",
        "--scope",
        "binary receipt timeout",
        "--summary",
        "binary timeout proof",
        "--class",
        "small",
        "--out",
        &receipt_arg,
        "--timeout-ms",
        "50",
        "--json",
        "--",
    ]);
    command.arg(program).args(args);
    let stdout = successful_stdout(command.output().unwrap());
    let receipt: ProofReceipt = serde_json::from_str(&stdout).unwrap();

    assert!(matches!(
        receipt.commands[0].status,
        CommandStatus::Blocked { .. }
    ));
    assert!(receipt.commands[0].evidence.is_none());
    assert!(!receipt.receipt_id.is_empty());
    assert!(receipt_path.exists());
}

#[test]
fn strategy_binary_next_with_handoff_receipt_persists_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let state_dir_arg = state_dir.to_string_lossy().to_string();
    let claim_output = driven()
        .args([
            "strategy",
            "claim-state",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary handoff",
            "--json",
        ])
        .output()
        .unwrap();
    let claim_stdout = successful_stdout(claim_output);
    let assignment: LanePassAssignment = serde_json::from_str(&claim_stdout).unwrap();

    let mut receipt = ProofReceipt::new(assignment.claim.clone(), "binary handoff proof")
        .with_worktree_identity(assignment.worktree.identity())
        .with_command(
            CommandProof::observed(
                "cargo fmt --check",
                VerificationClass::Small,
                0,
                Path::new("G:/Dx/driven"),
                b"ok\n",
                b"",
                1_000,
                1_001,
            )
            .unwrap(),
        )
        .with_outcome(OutcomeProof::verified("small command passed"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let receipt_arg = receipt_path.to_string_lossy().to_string();
    fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();

    let next_output = driven()
        .args([
            "strategy",
            "next",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary handoff",
            "--receipt",
            &receipt_arg,
            "--next-action",
            "continue with pass 2",
            "--json",
        ])
        .output()
        .unwrap();
    let next_stdout = successful_stdout(next_output);
    let advanced: LanePassAssignment = serde_json::from_str(&next_stdout).unwrap();

    assert_eq!(advanced.status, LanePassAssignmentStatus::Advanced);
    assert_eq!(advanced.pass.value(), 2);
    assert_eq!(
        advanced.receipt_id.as_deref(),
        Some(receipt.receipt_id.as_str())
    );
    let stored_receipt: ProofReceipt =
        serde_json::from_str(&fs::read_to_string(advanced.receipt_path.as_ref().unwrap()).unwrap())
            .unwrap();
    let stored_handoff: NextPassHandoff =
        serde_json::from_str(&fs::read_to_string(advanced.handoff_path.as_ref().unwrap()).unwrap())
            .unwrap();

    assert_eq!(stored_receipt.schema, "driven.proof_receipt.v1");
    assert_eq!(stored_receipt.receipt_id, receipt.receipt_id);
    assert_eq!(stored_handoff.schema, "driven.lane_handoff.v1");
    assert_eq!(stored_handoff.receipt_id, receipt.receipt_id);
    assert_eq!(stored_handoff.next_action, "continue with pass 2");
    assert_eq!(stored_handoff.lane, assignment.lane);
    assert_eq!(stored_handoff.completed_pass, assignment.pass);
    assert_eq!(stored_handoff.next_pass, advanced.pass);
    assert_eq!(stored_handoff.worker_id, assignment.worker_id);
    assert_eq!(
        stored_handoff.worktree_identity.as_ref(),
        Some(&WorktreeIdentity::from_metadata(&assignment.worktree))
    );
}

#[test]
fn strategy_binary_durable_next_rejects_plain_next_without_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let state_dir_arg = state_dir.to_string_lossy().to_string();
    let claim_output = driven()
        .args([
            "strategy",
            "claim-state",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary handoff",
            "--json",
        ])
        .output()
        .unwrap();
    successful_stdout(claim_output);

    let output = driven()
        .args([
            "strategy",
            "next",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary handoff",
            "--durable-next",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(stderr.contains("durable handoff"));
}

#[test]
fn strategy_binary_next_requires_durable_handoff_by_default() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let state_dir_arg = state_dir.to_string_lossy().to_string();
    let claim_output = driven()
        .args([
            "strategy",
            "claim-state",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin-default-durable",
            "--scope",
            "binary default durable",
            "--json",
        ])
        .output()
        .unwrap();
    successful_stdout(claim_output);

    let output = driven()
        .args([
            "strategy",
            "next",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin-default-durable",
            "--scope",
            "binary default durable",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(stderr.contains("durable handoff"));
}

#[test]
fn strategy_binary_next_allows_explicit_unsafe_legacy_next() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let state_dir_arg = state_dir.to_string_lossy().to_string();
    let claim_output = driven()
        .args([
            "strategy",
            "claim-state",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin-legacy-next",
            "--scope",
            "binary legacy next",
            "--json",
        ])
        .output()
        .unwrap();
    successful_stdout(claim_output);

    let output = driven()
        .args([
            "strategy",
            "next",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin-legacy-next",
            "--scope",
            "binary legacy next",
            "--unsafe-legacy-next",
            "--json",
        ])
        .output()
        .unwrap();
    let stdout = successful_stdout(output);
    let advanced: LanePassAssignment = serde_json::from_str(&stdout).unwrap();

    assert_eq!(advanced.status, LanePassAssignmentStatus::Advanced);
    assert_eq!(advanced.pass.value(), 2);
    assert!(advanced.receipt_path.is_none());
    assert!(advanced.handoff_path.is_none());
}

#[test]
fn strategy_binary_next_rejects_receipt_without_next_action() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir_arg = temp.path().join("state").to_string_lossy().to_string();
    let receipt_arg = temp
        .path()
        .join("receipt.json")
        .to_string_lossy()
        .to_string();

    let output = driven()
        .args([
            "strategy",
            "next",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary handoff",
            "--receipt",
            &receipt_arg,
            "--json",
        ])
        .output()
        .unwrap();

    assert_next_handoff_pair_rejected(output);
}

#[test]
fn strategy_binary_next_rejects_next_action_without_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir_arg = temp.path().join("state").to_string_lossy().to_string();

    let output = driven()
        .args([
            "strategy",
            "next",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary handoff",
            "--next-action",
            "continue with pass 2",
            "--json",
        ])
        .output()
        .unwrap();

    assert_next_handoff_pair_rejected(output);
}

#[test]
fn strategy_binary_complete_requires_receipt_argument() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir_arg = temp.path().join("state").to_string_lossy().to_string();
    let output = driven()
        .args([
            "strategy",
            "complete",
            "--state-dir",
            &state_dir_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary state",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(stderr.contains("receipt"));
}

#[test]
fn strategy_binary_claim_state_uses_explicit_project_root_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir_arg = temp.path().join("state").to_string_lossy().to_string();
    let project_root_arg = temp.path().join("project").to_string_lossy().to_string();
    fs::create_dir_all(&project_root_arg).unwrap();

    let output = driven()
        .args([
            "strategy",
            "claim-state",
            "--state-dir",
            &state_dir_arg,
            "--project-root",
            &project_root_arg,
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary state",
            "--json",
        ])
        .output()
        .unwrap();
    let stdout = successful_stdout(output);
    let assignment: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(
        assignment["worktree"]["input_root"],
        Value::String(project_root_arg)
    );
}

#[test]
fn strategy_binary_strict_isolation_rejects_non_git_project_root() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir_arg = temp.path().join("state").to_string_lossy().to_string();
    let project_root_arg = temp.path().join("project").to_string_lossy().to_string();
    fs::create_dir_all(&project_root_arg).unwrap();

    let output = driven()
        .args([
            "strategy",
            "claim-state",
            "--state-dir",
            &state_dir_arg,
            "--project-root",
            &project_root_arg,
            "--strict-isolation",
            "--worker-id",
            "worker-bin",
            "--scope",
            "binary state",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(stderr.contains("isolated worktree"));
}

#[test]
fn strategy_binary_cycle_lanes_reuses_released_lane() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir_arg = temp.path().join("state").to_string_lossy().to_string();
    let common = [
        "--state-dir",
        state_dir_arg.as_str(),
        "--scope",
        "binary state",
        "--max-lanes",
        "1",
        "--cycle-lanes",
        "--json",
    ];

    let first = driven()
        .args(["strategy", "claim-state"])
        .args(common)
        .args(["--worker-id", "worker-one"])
        .output()
        .unwrap();
    successful_stdout(first);

    let release = driven()
        .args(["strategy", "release"])
        .args(common)
        .args(["--worker-id", "worker-one"])
        .output()
        .unwrap();
    successful_stdout(release);

    let second = driven()
        .args(["strategy", "claim-state"])
        .args(common)
        .args(["--worker-id", "worker-two"])
        .output()
        .unwrap();
    let stdout = successful_stdout(second);
    let assignment: LanePassAssignment = serde_json::from_str(&stdout).unwrap();

    assert_eq!(assignment.lane.value(), 1);
}
