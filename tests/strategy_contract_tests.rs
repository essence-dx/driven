use driven::cli::{StrategyCommand, StrategyReceiptExecutionOptions, StrategyStateOptions};
use driven::strategy::{
    CommandEvidence, CommandProof, CommandStatus, FileProof, LaneClaim, LaneId, LanePassAssignment,
    LanePassAssignmentStatus, LanePassConfig, LanePassStore, NextPassHandoff, OutcomeProof,
    PassNumber, ProofReceipt, ReceiptFormat, SubagentDelegation, VerificationClass, WorkerId,
    WorktreeCreationDecision, WorktreeIdentity, WorktreeIsolationMode, detect_worktree_metadata,
    plan_worktree_isolation,
};
use std::fs;
use std::path::Path;

fn observed_small_command(command: &str) -> CommandProof {
    CommandProof::observed(
        command,
        VerificationClass::Small,
        0,
        Path::new("G:/Dx/driven"),
        b"ok\n",
        b"",
        1_000,
        1_001,
    )
    .unwrap()
}

fn successful_receipt_for_assignment(
    assignment: &LanePassAssignment,
    summary: &str,
) -> ProofReceipt {
    let mut receipt = ProofReceipt::new(assignment.claim.clone(), summary)
        .with_worktree_identity(assignment.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::verified("small command passed"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    receipt
}

#[cfg(windows)]
fn small_echo_command() -> (&'static str, Vec<String>) {
    ("cmd", vec!["/C".into(), "echo driven-proof".into()])
}

#[cfg(not(windows))]
fn small_echo_command() -> (&'static str, Vec<String>) {
    ("sh", vec!["-c".into(), "printf driven-proof".into()])
}

#[cfg(windows)]
fn assert_no_verbatim_path_prefix(path: &Path) {
    assert!(!path.to_string_lossy().starts_with(r"\\?\"));
}

#[cfg(not(windows))]
fn assert_no_verbatim_path_prefix(_path: &Path) {}

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
            "Start-Sleep -Milliseconds 800".into(),
        ],
    )
}

#[cfg(not(windows))]
fn slow_command() -> (&'static str, Vec<String>) {
    ("sh", vec!["-c".into(), "sleep 1".into()])
}

#[cfg(windows)]
fn remove_path_command(path: &Path) -> (&'static str, Vec<String>) {
    let escaped = path.to_string_lossy().replace('\'', "''");
    (
        "powershell",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            format!("Remove-Item -LiteralPath '{}' -Force", escaped),
        ],
    )
}

#[cfg(not(windows))]
fn remove_path_command(path: &Path) -> (&'static str, Vec<String>) {
    (
        "sh",
        vec![
            "-c".into(),
            "rm -f \"$1\"".into(),
            "remove-path".into(),
            path.to_string_lossy().to_string(),
        ],
    )
}

#[cfg(windows)]
fn write_path_command(path: &Path) -> (&'static str, Vec<String>) {
    let escaped = path.to_string_lossy().replace('\'', "''");
    (
        "powershell",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            format!(
                "Set-Content -LiteralPath '{}' -Value ran -NoNewline",
                escaped
            ),
        ],
    )
}

#[cfg(not(windows))]
fn write_path_command(path: &Path) -> (&'static str, Vec<String>) {
    (
        "sh",
        vec![
            "-c".into(),
            "printf ran > \"$1\"".into(),
            "write-path".into(),
            path.to_string_lossy().to_string(),
        ],
    )
}

#[test]
fn lane_claim_builds_next_pass_handoff_for_same_worker() {
    let worker = WorkerId::new("worker-friday-01").unwrap();
    let claim = LaneClaim::new(
        LaneId::new(7).unwrap(),
        PassNumber::new(2).unwrap(),
        worker.clone(),
        "route-handler request behavior",
    )
    .with_subagent(SubagentDelegation::new(
        worker,
        LaneId::new(7).unwrap(),
        PassNumber::new(2).unwrap(),
        "verify proof contract",
    ));

    let receipt = ProofReceipt::new(claim.clone(), "pass proof")
        .with_command(CommandProof::passed(
            "rg -n \"request.url\" src",
            VerificationClass::Small,
        ))
        .with_file(FileProof::new(
            "src/delivery/server_contract.rs",
            "request alias normalization",
        ))
        .with_outcome(OutcomeProof::verified(
            "focused guard documents alias handling",
        ));

    let handoff = claim
        .next_pass_handoff(receipt, "continue with HEAD/OPTIONS request behavior")
        .unwrap();

    assert_eq!(handoff.lane, LaneId::new(7).unwrap());
    assert_eq!(handoff.completed_pass, PassNumber::new(2).unwrap());
    assert_eq!(handoff.next_pass, PassNumber::new(3).unwrap());
    assert_eq!(handoff.worker_id.as_str(), "worker-friday-01");
    assert!(handoff.next_action.contains("HEAD/OPTIONS"));
}

#[test]
fn receipt_validation_rejects_heavy_command_without_small_first_proof() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-heavy").unwrap(),
        "build proof",
    );

    let receipt = ProofReceipt::new(claim, "heavy first")
        .with_command(CommandProof::new(
            "cargo test -p dx-driven --lib",
            VerificationClass::Heavy,
            CommandStatus::Passed { exit_code: 0 },
        ))
        .with_outcome(OutcomeProof::partial("heavy command was attempted first"));

    let err = receipt.validate().unwrap_err();
    assert!(err.to_string().contains("small command"));
}

#[test]
fn command_proof_observed_records_captured_evidence() {
    let evidence =
        CommandEvidence::from_streams(Path::new("G:/Dx/driven"), b"driven-proof\n", b"", 10, 11)
            .unwrap();
    let proof = CommandProof::observed(
        "cmd /C echo driven-proof",
        VerificationClass::Small,
        0,
        Path::new("G:/Dx/driven"),
        b"driven-proof\n",
        b"",
        10,
        11,
    )
    .unwrap();

    assert_eq!(proof.status, CommandStatus::Passed { exit_code: 0 });
    assert_eq!(proof.evidence.as_ref(), Some(&evidence));
    assert_eq!(evidence.stdout_bytes, 13);
    assert_eq!(evidence.stderr_bytes, 0);
    assert_eq!(
        evidence.stdout_digest,
        blake3::hash(b"driven-proof\n").to_hex().to_string()
    );
}

#[test]
fn receipt_validation_rejects_empty_outcomes() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-outcome").unwrap(),
        "outcome proof",
    );
    let receipt = ProofReceipt::new(claim, "missing outcome").with_command(CommandProof::passed(
        "cargo fmt --check",
        VerificationClass::Small,
    ));

    let err = receipt.validate().unwrap_err();
    assert!(err.to_string().contains("outcome proof"));
}

#[test]
fn receipt_validation_rejects_verified_outcome_without_command_proof() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-empty-proof").unwrap(),
        "empty proof",
    );
    let receipt =
        ProofReceipt::new(claim, "no commands").with_outcome(OutcomeProof::verified("done"));

    let err = receipt.validate().unwrap_err();
    assert!(err.to_string().contains("command proof"));
}

#[test]
fn receipt_validation_rejects_skipped_command_with_verified_outcome() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-skipped").unwrap(),
        "skipped proof",
    );
    let receipt = ProofReceipt::new(claim, "skipped command")
        .with_command(CommandProof::new(
            "cargo fmt --check",
            VerificationClass::Small,
            CommandStatus::Skipped {
                reason: "formatting blocked by unrelated file".to_string(),
            },
        ))
        .with_outcome(OutcomeProof::verified("everything passed"));

    let err = receipt.validate().unwrap_err();
    assert!(err.to_string().contains("verified receipt"));
}

#[test]
fn receipt_rendering_redacts_common_secrets() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-redact").unwrap(),
        "receipt redaction",
    );
    let receipt = ProofReceipt::new(claim, "checked Authorization: Bearer secret-token")
        .with_command(CommandProof::passed(
            "OPENAI_API_KEY=sk-secret cargo test token=abc123",
            VerificationClass::Small,
        ))
        .with_file(FileProof::new(
            "src/strategy/receipt.rs",
            "removed password=hunter2 from proof output",
        ))
        .with_outcome(OutcomeProof::partial(
            "blocked by api_key=secret-value in local env",
        ));

    let json = receipt.render(ReceiptFormat::Json).unwrap();
    let markdown = receipt.render(ReceiptFormat::Markdown).unwrap();

    for rendered in [&json, &markdown] {
        assert!(!rendered.contains("secret-token"));
        assert!(!rendered.contains("sk-secret"));
        assert!(!rendered.contains("abc123"));
        assert!(!rendered.contains("hunter2"));
        assert!(!rendered.contains("secret-value"));
        assert!(rendered.contains("[REDACTED]"));
    }
}

#[test]
fn receipt_rendering_preserves_canonical_id_and_labels_redacted_digest() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-digest").unwrap(),
        "receipt digest token=scope-secret",
    );
    let receipt = ProofReceipt::new(claim.clone(), "checked token=summary-secret")
        .with_command(CommandProof::passed(
            "TOKEN=visible token=command-secret cargo fmt --check",
            VerificationClass::Small,
        ))
        .with_outcome(OutcomeProof::verified("passed"));
    let canonical_id = receipt.receipt_id().unwrap();
    let handoff = claim
        .next_pass_handoff(receipt.clone(), "continue")
        .unwrap();

    let rendered_json = receipt.render(ReceiptFormat::Json).unwrap();
    let rendered: ProofReceipt = serde_json::from_str(&rendered_json).unwrap();
    let rendered_markdown = receipt.render(ReceiptFormat::Markdown).unwrap();

    assert_eq!(handoff.receipt_id, canonical_id);
    assert_eq!(rendered.receipt_id, canonical_id);
    assert!(rendered.redacted);
    assert!(rendered.redacted_payload_digest.is_some());
    assert_ne!(
        rendered.redacted_payload_digest.as_deref(),
        Some(canonical_id.as_str())
    );
    rendered.validate().unwrap();
    assert!(rendered_markdown.contains("Canonical receipt:"));
    assert!(rendered_markdown.contains("Redacted payload:"));
    assert!(!rendered_json.contains("summary-secret"));
    assert!(!rendered_json.contains("command-secret"));
}

#[test]
fn receipt_rendering_redacts_structured_secret_keys_and_auth_headers() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-structured-redaction").unwrap(),
        "scope",
    );
    let receipt = ProofReceipt::new(
        claim,
        r#"checked {"api_key":"sk-live","client_secret": "client-secret-value"} refresh_token = refresh-value"#,
    )
    .with_command(CommandProof::new(
        r#"curl -H "Authorization: Basic abc123" --data "password : p@ss" https://example.test"#,
        VerificationClass::Small,
        CommandStatus::Skipped {
            reason: r#"blocked by access_token : access-value"#.to_string(),
        },
    ))
    .with_outcome(OutcomeProof::blocked(
        r#"waiting on {"token": "json-token"} and Authorization: Bearer bearer-value"#,
    ));

    let rendered = receipt.render(ReceiptFormat::Json).unwrap();

    for secret in [
        "sk-live",
        "client-secret-value",
        "refresh-value",
        "abc123",
        "p@ss",
        "access-value",
        "json-token",
        "bearer-value",
    ] {
        assert!(!rendered.contains(secret), "leaked secret: {secret}");
    }
    assert!(rendered.contains("[REDACTED]"));
}

#[test]
fn handoff_rejects_redacted_receipt() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-redacted-handoff").unwrap(),
        "handoff redaction",
    );
    let receipt = ProofReceipt::new(claim.clone(), "contains token=secret")
        .with_command(CommandProof::passed(
            "cargo fmt --check token=secret",
            VerificationClass::Small,
        ))
        .with_outcome(OutcomeProof::partial("continue"));
    let redacted: ProofReceipt =
        serde_json::from_str(&receipt.render(ReceiptFormat::Json).unwrap()).unwrap();

    let err = claim
        .next_pass_handoff(redacted, "continue safely")
        .unwrap_err();

    assert!(err.to_string().contains("unredacted"));
}

#[test]
fn receipt_markdown_escapes_injected_headings() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-injection").unwrap(),
        "receipt injection\n## Forged Scope",
    );
    let receipt = ProofReceipt::new(claim, "summary\n## Forged Summary | cell")
        .with_command(CommandProof::new(
            "cargo fmt --check",
            VerificationClass::Small,
            CommandStatus::Blocked {
                reason: "blocked\n## Forged Blocker | cell".to_string(),
            },
        ))
        .with_outcome(OutcomeProof::blocked("blocked\n## Forged Outcome | cell"));

    let markdown = receipt.render(ReceiptFormat::Markdown).unwrap();

    assert_eq!(markdown.matches("\n## Digest\n").count(), 1);
    assert!(!markdown.lines().any(|line| line == "## Forged Summary"));
    assert!(!markdown.lines().any(|line| line == "## Forged Blocker"));
    assert!(!markdown.lines().any(|line| line == "## Forged Outcome"));
    assert!(markdown.contains("summary<br>## Forged Summary \\| cell"));
    assert!(markdown.contains("blocked<br>## Forged Blocker \\| cell"));
}

#[test]
fn receipt_validation_rejects_stale_receipt_id() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-stale").unwrap(),
        "receipt integrity",
    );
    let mut receipt = ProofReceipt::new(claim, "original summary")
        .with_command(CommandProof::passed(
            "cargo fmt --check",
            VerificationClass::Small,
        ))
        .with_outcome(OutcomeProof::verified("passed"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    receipt.summary = "tampered summary".to_string();

    let err = receipt.validate().unwrap_err();
    assert!(err.to_string().contains("receipt id"));
}

#[test]
fn receipt_validation_rejects_failed_small_command_before_heavy() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-small").unwrap(),
        "small-command-first",
    );
    let receipt = ProofReceipt::new(claim, "failed small")
        .with_command(CommandProof::new(
            "cargo fmt --check",
            VerificationClass::Small,
            CommandStatus::Failed { exit_code: 1 },
        ))
        .with_command(CommandProof::new(
            "cargo test --lib -j1",
            VerificationClass::Heavy,
            CommandStatus::Passed { exit_code: 0 },
        ))
        .with_outcome(OutcomeProof::partial("small command failed"));

    let err = receipt.validate().unwrap_err();
    assert!(err.to_string().contains("passed small command"));
}

#[test]
fn receipt_validation_rejects_verified_outcome_with_failed_command() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-failed").unwrap(),
        "verified honesty",
    );
    let receipt = ProofReceipt::new(claim, "failed command")
        .with_command(CommandProof::new(
            "cargo fmt --check",
            VerificationClass::Small,
            CommandStatus::Failed { exit_code: 1 },
        ))
        .with_outcome(OutcomeProof::verified("everything passed"));

    let err = receipt.validate().unwrap_err();
    assert!(err.to_string().contains("verified receipt"));
}

#[test]
fn lane_claim_rejects_subagent_on_different_lane_or_pass() {
    let claim = LaneClaim::new(
        LaneId::new(7).unwrap(),
        PassNumber::new(2).unwrap(),
        WorkerId::new("worker-parent").unwrap(),
        "subagent containment",
    )
    .with_subagent(SubagentDelegation::new(
        WorkerId::new("worker-child").unwrap(),
        LaneId::new(8).unwrap(),
        PassNumber::new(2).unwrap(),
        "review sibling lane",
    ));

    let err = claim.validate().unwrap_err();
    assert!(err.to_string().contains("subagent lane/pass"));
}

#[test]
fn lane_claim_rejects_deserialized_noncanonical_subagent_worker_id() {
    let claim = LaneClaim::new(
        LaneId::new(7).unwrap(),
        PassNumber::new(2).unwrap(),
        WorkerId::new("worker-parent").unwrap(),
        "subagent containment",
    )
    .with_subagent(SubagentDelegation::new(
        WorkerId::new("worker-child").unwrap(),
        LaneId::new(7).unwrap(),
        PassNumber::new(2).unwrap(),
        "review parent lane",
    ));
    let mut value = serde_json::to_value(&claim).unwrap();
    value["subagents"][0]["worker_id"] = serde_json::Value::String("Worker-Child".to_string());
    let tampered: LaneClaim = serde_json::from_value(value).unwrap();

    let err = tampered.validate().unwrap_err();

    assert!(err.to_string().contains("subagent worker id"));
}

#[test]
fn handoff_rejects_receipt_from_different_claim() {
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-a").unwrap(),
        "handoff integrity",
    );
    let other_claim = LaneClaim::new(
        LaneId::new(2).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-b").unwrap(),
        "handoff integrity",
    );
    let receipt = ProofReceipt::new(other_claim, "foreign receipt")
        .with_outcome(OutcomeProof::partial("not the same claim"));

    let err = claim.next_pass_handoff(receipt, "continue").unwrap_err();
    assert!(err.to_string().contains("receipt claim"));
}

#[test]
fn receipt_formats_are_deterministic_and_include_core_proof() {
    let claim = LaneClaim::new(
        LaneId::new(4).unwrap(),
        PassNumber::new(1).unwrap(),
        WorkerId::new("worker-json").unwrap(),
        "receipt serialization",
    );
    let receipt = ProofReceipt::new(claim, "serialization proof")
        .with_command(CommandProof::passed(
            "cargo fmt --check",
            VerificationClass::Small,
        ))
        .with_outcome(OutcomeProof::blocked("cargo cache is incomplete"));

    let json = receipt.render(ReceiptFormat::Json).unwrap();
    let markdown = receipt.render(ReceiptFormat::Markdown).unwrap();

    assert_eq!(json, receipt.render(ReceiptFormat::Json).unwrap());
    assert!(json.contains("\"lane\": 4"));
    assert!(json.contains("\"worker_id\": \"worker-json\""));
    assert!(markdown.contains("# DX Proof Receipt"));
    assert!(markdown.contains("cargo cache is incomplete"));
}

#[test]
fn worktree_detection_reports_not_repository_for_temp_directory() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = detect_worktree_metadata(temp.path());

    assert!(metadata.is_not_repository());
    assert_eq!(metadata.worktree_root, None);
    assert!(metadata.detection_error.is_some());
}

#[test]
fn worktree_detection_normalizes_relative_input_root_to_absolute_path() {
    let cwd = std::env::current_dir().unwrap();
    let temp = tempfile::tempdir_in(&cwd).unwrap();
    let relative = temp.path().strip_prefix(&cwd).unwrap();

    let metadata = detect_worktree_metadata(relative);

    assert!(metadata.input_root.is_absolute());
    assert_eq!(
        fs::canonicalize(&metadata.input_root).unwrap(),
        fs::canonicalize(temp.path()).unwrap()
    );
    assert_no_verbatim_path_prefix(&metadata.input_root);
}

#[test]
fn worktree_plan_blocks_git_worktree_creation_for_non_repository() {
    let temp = tempfile::tempdir().unwrap();
    let plan = plan_worktree_isolation(temp.path());

    assert_eq!(plan.mode, WorktreeIsolationMode::NoGitRepository);
    assert_eq!(plan.creation_decision, WorktreeCreationDecision::Blocked);
    assert!(!plan.can_create_git_worktree);
    assert!(!plan.native_isolation_detected);
    assert_eq!(plan.execution_root, None);
    assert!(
        plan.blockers
            .iter()
            .any(|blocker| blocker.contains("not a Git repository"))
    );
}

#[test]
fn worktree_plan_blocks_git_worktree_creation_for_unborn_repository() {
    let temp = tempfile::tempdir().unwrap();
    if !git_available() {
        return;
    }
    run_git(temp.path(), &["init"]).unwrap();

    let plan = plan_worktree_isolation(temp.path());

    assert_eq!(plan.mode, WorktreeIsolationMode::MainWorktree);
    assert_eq!(plan.creation_decision, WorktreeCreationDecision::Blocked);
    assert!(!plan.can_create_git_worktree);
    assert!(
        plan.blockers
            .iter()
            .any(|blocker| blocker.contains("branch with a resolved HEAD"))
    );
}

#[test]
fn worktree_plan_allows_git_worktree_creation_for_clean_main_worktree() {
    let temp = tempfile::tempdir().unwrap();
    if !git_available() {
        return;
    }
    initialize_clean_repo(temp.path());

    let plan = plan_worktree_isolation(temp.path());

    assert_eq!(plan.mode, WorktreeIsolationMode::MainWorktree);
    assert_eq!(
        plan.creation_decision,
        WorktreeCreationDecision::CreateGitWorktree
    );
    assert!(plan.can_create_git_worktree);
}

#[test]
fn worktree_plan_blocks_git_worktree_creation_for_dirty_main_worktree() {
    let temp = tempfile::tempdir().unwrap();
    if !git_available() {
        return;
    }
    initialize_clean_repo(temp.path());
    fs::write(temp.path().join("README.md"), "dirty\n").unwrap();

    let plan = plan_worktree_isolation(temp.path());

    assert_eq!(plan.mode, WorktreeIsolationMode::MainWorktree);
    assert_eq!(plan.creation_decision, WorktreeCreationDecision::Blocked);
    assert!(!plan.can_create_git_worktree);
    assert!(
        plan.blockers
            .iter()
            .any(|blocker| blocker.contains("local changes"))
    );
}

#[test]
fn worktree_plan_reuses_clean_linked_worktree() {
    let temp = tempfile::tempdir().unwrap();
    if !git_available() {
        return;
    }
    let main = temp.path().join("main");
    let linked = temp.path().join("linked");
    fs::create_dir_all(&main).unwrap();
    initialize_clean_repo(&main);
    run_git(
        &main,
        &[
            "worktree",
            "add",
            "-b",
            "lane-worker",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    )
    .unwrap();

    let plan = plan_worktree_isolation(&linked);

    assert_eq!(plan.mode, WorktreeIsolationMode::LinkedWorktree);
    assert_eq!(
        plan.creation_decision,
        WorktreeCreationDecision::ReuseCurrentWorktree
    );
    assert!(plan.native_isolation_detected);
    assert!(!plan.can_create_git_worktree);
}

#[test]
fn lane_pass_store_claims_first_lane_and_preserves_worker_lane_on_next_pass() {
    let temp = tempfile::tempdir().unwrap();
    let config = LanePassConfig::new(temp.path().join("state"), "strategy engine")
        .with_max_lanes(30)
        .with_max_passes(3);
    let store = LanePassStore::new(config).unwrap();

    let first = store.claim(WorkerId::new("Worker.One").unwrap()).unwrap();
    let second = store.next_pass(&first.worker_id).unwrap();

    assert_eq!(first.lane, LaneId::new(1).unwrap());
    assert_eq!(first.pass, PassNumber::new(1).unwrap());
    assert_eq!(second.lane, first.lane);
    assert_eq!(second.pass, PassNumber::new(2).unwrap());
    assert!(first.paths.counter_path.starts_with(temp.path()));
    assert!(first.paths.claims_path.starts_with(temp.path()));
}

#[test]
fn lane_pass_store_advances_with_durable_handoff_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(
        LanePassConfig::new(temp.path().join("state"), "strategy engine")
            .with_max_lanes(30)
            .with_max_passes(3),
    )
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-handoff").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "handoff proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();

    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "run targeted lane tests")
        .unwrap();

    assert_eq!(advanced.status, LanePassAssignmentStatus::Advanced);
    assert_eq!(advanced.lane, first.lane);
    assert_eq!(advanced.pass, PassNumber::new(2).unwrap());
    assert_eq!(
        advanced.receipt_id.as_deref(),
        Some(receipt.receipt_id.as_str())
    );
    let handoff_path = advanced.handoff_path.as_ref().unwrap();
    let stored_handoff: NextPassHandoff =
        serde_json::from_str(&fs::read_to_string(handoff_path).unwrap()).unwrap();
    stored_handoff.validate_against(&first.claim).unwrap();
    assert_eq!(stored_handoff.receipt_id, receipt.receipt_id);
    assert_eq!(stored_handoff.next_action, "run targeted lane tests");
    let stored_receipt: ProofReceipt =
        serde_json::from_str(&fs::read_to_string(advanced.receipt_path.as_ref().unwrap()).unwrap())
            .unwrap();
    assert_eq!(stored_receipt.receipt_id, receipt.receipt_id);
}

#[test]
fn lane_pass_store_redacts_secret_markers_in_persisted_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-handoff-redaction").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "handoff token=summary-secret")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue without secrets"));
    receipt.receipt_id = receipt.receipt_id().unwrap();

    let advanced = store
        .next_pass_with_handoff(
            &first.worker_id,
            &receipt,
            "continue token=next-action-secret",
        )
        .unwrap();

    let handoff_json = fs::read_to_string(advanced.handoff_path.as_ref().unwrap()).unwrap();
    assert!(!handoff_json.contains("summary-secret"));
    assert!(!handoff_json.contains("next-action-secret"));
    assert!(handoff_json.contains("token=[REDACTED]"));
}

#[test]
fn lane_pass_store_can_require_durable_handoff_for_next_pass() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(
        LanePassConfig::new(temp.path().join("state"), "strategy engine")
            .with_handoff_required_for_next(true),
    )
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-strict-handoff").unwrap())
        .unwrap();

    let err = store.next_pass(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("durable handoff"));

    let mut receipt = ProofReceipt::new(first.claim.clone(), "strict handoff proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();

    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue with pass 2")
        .unwrap();

    assert_eq!(advanced.status, LanePassAssignmentStatus::Advanced);
    assert_eq!(advanced.pass, PassNumber::new(2).unwrap());
}

#[test]
fn lane_pass_store_persists_handoff_worktree_identity() {
    let temp = tempfile::tempdir().unwrap();
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).unwrap();
    let store = LanePassStore::new(
        LanePassConfig::new(temp.path().join("state"), "strategy engine")
            .with_project_root(&project_root),
    )
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-worktree-identity").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "worktree handoff proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();

    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue with same checkout")
        .unwrap();

    let stored_handoff: NextPassHandoff =
        serde_json::from_str(&fs::read_to_string(advanced.handoff_path.as_ref().unwrap()).unwrap())
            .unwrap();
    assert_eq!(
        stored_handoff.worktree_identity.as_ref(),
        Some(&WorktreeIdentity::from_metadata(&first.worktree))
    );
}

#[test]
fn lane_pass_store_rejects_worktree_identity_change_on_handoff_advance() {
    let temp = tempfile::tempdir().unwrap();
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let state_dir = temp.path().join("state");
    let store_a = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_a),
    )
    .unwrap();
    let first = store_a
        .claim(WorkerId::new("worker-moved-root").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "moved root proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let store_b = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_b),
    )
    .unwrap();

    let err = store_b
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue from a different root")
        .unwrap_err();

    assert!(err.to_string().contains("worktree identity"));
    assert_eq!(
        store_a
            .worker_assignment(&first.worker_id)
            .unwrap()
            .unwrap()
            .pass,
        first.pass
    );
}

#[test]
fn lane_pass_store_rejects_worktree_identity_change_on_plain_next() {
    let temp = tempfile::tempdir().unwrap();
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let state_dir = temp.path().join("state");
    let store_a = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_a),
    )
    .unwrap();
    let first = store_a
        .claim(WorkerId::new("worker-plain-next-moved-root").unwrap())
        .unwrap();
    let store_b = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_b),
    )
    .unwrap();

    let err = store_b.next_pass(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("worktree identity"));
    assert_eq!(
        store_a
            .worker_assignment(&first.worker_id)
            .unwrap()
            .unwrap()
            .pass,
        first.pass
    );
}

#[test]
fn lane_pass_store_claim_rejects_existing_active_claim_from_different_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let state_dir = temp.path().join("state");
    let store_a = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_a),
    )
    .unwrap();
    let first = store_a
        .claim(WorkerId::new("worker-claim-moved-root").unwrap())
        .unwrap();
    let store_b = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_b),
    )
    .unwrap();

    let err = store_b.claim(first.worker_id.clone()).unwrap_err();

    assert!(err.to_string().contains("worktree identity"));
    assert_eq!(
        store_a
            .worker_assignment(&first.worker_id)
            .unwrap()
            .unwrap()
            .claim
            .token,
        first.claim.token
    );
}

#[test]
fn lane_pass_store_rejects_worktree_identity_change_on_completion() {
    let temp = tempfile::tempdir().unwrap();
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let state_dir = temp.path().join("state");
    let store_a = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_a),
    )
    .unwrap();
    let first = store_a
        .claim(WorkerId::new("worker-complete-moved-root").unwrap())
        .unwrap();
    let receipt = successful_receipt_for_assignment(&first, "moved root completion proof");
    let store_b = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_b),
    )
    .unwrap();

    let err = store_b
        .complete_pass_with_receipt(&first.worker_id, &receipt)
        .unwrap_err();

    assert!(err.to_string().contains("worktree identity"));
    assert_eq!(
        store_a
            .worker_assignment(&first.worker_id)
            .unwrap()
            .unwrap()
            .status,
        LanePassAssignmentStatus::Claimed
    );
}

#[test]
fn lane_pass_store_rejects_receipt_from_different_worktree_identity() {
    let temp = tempfile::tempdir().unwrap();
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let state_a = temp.path().join("state-a");
    let state_b = temp.path().join("state-b");
    let store_a = LanePassStore::new(
        LanePassConfig::new(&state_a, "strategy engine").with_project_root(&project_a),
    )
    .unwrap();
    let store_b = LanePassStore::new(
        LanePassConfig::new(&state_b, "strategy engine").with_project_root(&project_b),
    )
    .unwrap();
    let first = store_a
        .claim(WorkerId::new("worker-cross-worktree-receipt").unwrap())
        .unwrap();
    let second = store_b
        .claim(WorkerId::new("worker-cross-worktree-receipt").unwrap())
        .unwrap();
    let receipt = successful_receipt_for_assignment(&first, "project a proof");

    let err = store_b
        .complete_pass_with_receipt(&second.worker_id, &receipt)
        .unwrap_err();

    assert!(err.to_string().contains("worktree identity"));
    assert_eq!(
        store_b
            .worker_assignment(&second.worker_id)
            .unwrap()
            .unwrap()
            .status,
        LanePassAssignmentStatus::Claimed
    );
}

#[test]
fn lane_pass_store_rejects_cross_state_same_worktree_completion_replay() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let state_a = temp.path().join("state-a");
    let state_b = temp.path().join("state-b");
    let store_a = LanePassStore::new(
        LanePassConfig::new(&state_a, "strategy engine").with_project_root(&project),
    )
    .unwrap();
    let store_b = LanePassStore::new(
        LanePassConfig::new(&state_b, "strategy engine").with_project_root(&project),
    )
    .unwrap();
    let first = store_a
        .claim(WorkerId::new("worker-cross-state-replay").unwrap())
        .unwrap();
    let second = store_b
        .claim(WorkerId::new("worker-cross-state-replay").unwrap())
        .unwrap();
    let receipt = successful_receipt_for_assignment(&first, "state a proof");

    let err = store_b
        .complete_pass_with_receipt(&second.worker_id, &receipt)
        .unwrap_err();

    assert!(err.to_string().contains("state identity"));
    assert_eq!(
        store_b
            .worker_assignment(&second.worker_id)
            .unwrap()
            .unwrap()
            .status,
        LanePassAssignmentStatus::Claimed
    );
}

#[test]
fn lane_pass_store_persists_state_session_manifest_on_first_claim() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let store = LanePassStore::new(LanePassConfig::new(&state_dir, "strategy engine")).unwrap();

    let assignment = store
        .claim(WorkerId::new("worker-session-manifest").unwrap())
        .unwrap();
    let manifest_path = state_dir.join("state-session.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();

    assert!(manifest_path.exists());
    assert_eq!(
        manifest["state_session_id"],
        serde_json::Value::String(
            assignment
                .claim
                .state_session_id
                .as_ref()
                .unwrap()
                .as_str()
                .to_string()
        )
    );
    assert_eq!(
        manifest["scope"],
        serde_json::Value::String("strategy engine".to_string())
    );
}

#[test]
fn lane_pass_store_rejects_cross_state_same_worktree_handoff_replay() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let state_a = temp.path().join("state-a");
    let state_b = temp.path().join("state-b");
    let store_a = LanePassStore::new(
        LanePassConfig::new(&state_a, "strategy engine").with_project_root(&project),
    )
    .unwrap();
    let store_b = LanePassStore::new(
        LanePassConfig::new(&state_b, "strategy engine").with_project_root(&project),
    )
    .unwrap();
    let first = store_a
        .claim(WorkerId::new("worker-cross-state-handoff").unwrap())
        .unwrap();
    let second = store_b
        .claim(WorkerId::new("worker-cross-state-handoff").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "state a handoff proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();

    let err = store_b
        .next_pass_with_handoff(&second.worker_id, &receipt, "continue from state b")
        .unwrap_err();

    assert!(err.to_string().contains("state identity"));
    assert_eq!(
        store_b
            .worker_assignment(&second.worker_id)
            .unwrap()
            .unwrap()
            .status,
        LanePassAssignmentStatus::Claimed
    );
}

#[test]
fn lane_pass_store_rejects_worktree_identity_change_on_release() {
    let temp = tempfile::tempdir().unwrap();
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let state_dir = temp.path().join("state");
    let store_a = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_a),
    )
    .unwrap();
    let first = store_a
        .claim(WorkerId::new("worker-release-moved-root").unwrap())
        .unwrap();
    let store_b = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_b),
    )
    .unwrap();

    let err = store_b.release_lane(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("worktree identity"));
    assert_eq!(
        store_a
            .worker_assignment(&first.worker_id)
            .unwrap()
            .unwrap()
            .status,
        LanePassAssignmentStatus::Claimed
    );
}

#[test]
fn lane_pass_store_cycle_lanes_does_not_reclaim_different_worktree_lane() {
    let temp = tempfile::tempdir().unwrap();
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let state_dir = temp.path().join("state");
    let store_a = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine")
            .with_project_root(&project_a)
            .with_max_lanes(1)
            .with_lane_cycling(true),
    )
    .unwrap();
    let first = store_a
        .claim(WorkerId::new("worker-cycle-a").unwrap())
        .unwrap();
    store_a.release_lane(&first.worker_id).unwrap();
    let store_b = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine")
            .with_project_root(&project_b)
            .with_max_lanes(1)
            .with_lane_cycling(true),
    )
    .unwrap();

    let err = store_b
        .claim(WorkerId::new("worker-cycle-b").unwrap())
        .unwrap_err();

    assert!(err.to_string().contains("current worktree"));
}

#[test]
fn lane_pass_store_cycle_lanes_reclaims_same_worktree_lane() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let state_dir = temp.path().join("state");
    let store = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine")
            .with_project_root(&project)
            .with_max_lanes(1)
            .with_lane_cycling(true),
    )
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-cycle-one").unwrap())
        .unwrap();
    store.release_lane(&first.worker_id).unwrap();

    let second = store
        .claim(WorkerId::new("worker-cycle-two").unwrap())
        .unwrap();

    assert_eq!(second.lane, first.lane);
    assert_eq!(second.pass, PassNumber::first());
}

#[test]
fn lane_pass_store_rejects_missing_handoff_payload_digest_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-missing-handoff-digest").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "handoff digest proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue with valid artifacts")
        .unwrap();
    let handoff_path = advanced.handoff_path.as_ref().unwrap();
    let mut handoff_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(handoff_path).unwrap()).unwrap();
    handoff_json
        .as_object_mut()
        .unwrap()
        .remove("payload_digest");
    fs::write(
        handoff_path,
        serde_json::to_string_pretty(&handoff_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("handoff payload digest"));
}

#[test]
fn lane_pass_store_rejects_tampered_handoff_artifact_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-tampered-handoff").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "tamper proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue with valid artifacts")
        .unwrap();
    let handoff_path = advanced.handoff_path.as_ref().unwrap();
    let mut handoff_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(handoff_path).unwrap()).unwrap();
    handoff_json["receipt_id"] = serde_json::Value::String("tampered-receipt-id".to_string());
    fs::write(
        handoff_path,
        serde_json::to_string_pretty(&handoff_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("stored handoff receipt id"));
}

#[test]
fn lane_pass_store_rejects_noncanonical_receipt_artifact_path_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let store = LanePassStore::new(LanePassConfig::new(&state_dir, "strategy engine")).unwrap();
    let first = store
        .claim(WorkerId::new("worker-noncanonical-receipt").unwrap())
        .unwrap();
    let receipt = successful_receipt_for_assignment(&first, "completion proof");
    let completed = store
        .complete_pass_with_receipt(&first.worker_id, &receipt)
        .unwrap();
    let duplicate_receipt_path = state_dir.join("receipts").join("duplicate-receipt.json");
    fs::copy(
        completed.receipt_path.as_ref().unwrap(),
        &duplicate_receipt_path,
    )
    .unwrap();
    let worker_path = &completed.paths.worker_path;
    let mut assignment_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(worker_path).unwrap()).unwrap();
    assignment_json["receipt_path"] =
        serde_json::Value::String(duplicate_receipt_path.to_string_lossy().to_string());
    fs::write(
        worker_path,
        serde_json::to_string_pretty(&assignment_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("canonical receipt path"));
}

#[test]
fn lane_pass_store_rejects_receipt_artifact_path_outside_state_dir() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-outside-receipt").unwrap())
        .unwrap();
    let receipt = successful_receipt_for_assignment(&first, "outside receipt proof");
    let completed = store
        .complete_pass_with_receipt(&first.worker_id, &receipt)
        .unwrap();
    let outside_receipt_path = temp.path().join("outside-receipt.json");
    fs::copy(
        completed.receipt_path.as_ref().unwrap(),
        &outside_receipt_path,
    )
    .unwrap();
    let worker_path = &completed.paths.worker_path;
    let mut assignment_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(worker_path).unwrap()).unwrap();
    assignment_json["receipt_path"] =
        serde_json::Value::String(outside_receipt_path.to_string_lossy().to_string());
    fs::write(
        worker_path,
        serde_json::to_string_pretty(&assignment_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("receipt path"));
}

#[test]
fn lane_pass_store_rejects_noncanonical_handoff_artifact_path_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let store = LanePassStore::new(LanePassConfig::new(&state_dir, "strategy engine")).unwrap();
    let first = store
        .claim(WorkerId::new("worker-noncanonical-handoff").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "handoff proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue with valid artifacts")
        .unwrap();
    let duplicate_handoff_path = state_dir.join("handoffs").join("duplicate-handoff.json");
    fs::copy(
        advanced.handoff_path.as_ref().unwrap(),
        &duplicate_handoff_path,
    )
    .unwrap();
    let worker_path = &advanced.paths.worker_path;
    let mut assignment_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(worker_path).unwrap()).unwrap();
    assignment_json["handoff_path"] =
        serde_json::Value::String(duplicate_handoff_path.to_string_lossy().to_string());
    fs::write(
        worker_path,
        serde_json::to_string_pretty(&assignment_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("canonical handoff path"));
}

#[test]
fn lane_pass_store_rejects_foreign_stored_receipt_claim_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let store = LanePassStore::new(LanePassConfig::new(&state_dir, "strategy engine")).unwrap();
    let first = store
        .claim(WorkerId::new("worker-foreign-artifact").unwrap())
        .unwrap();
    let receipt = successful_receipt_for_assignment(&first, "completion proof");
    let completed = store
        .complete_pass_with_receipt(&first.worker_id, &receipt)
        .unwrap();
    let foreign_claim = LaneClaim::new(
        LaneId::new(2).unwrap(),
        PassNumber::first(),
        WorkerId::new("worker-foreign-source").unwrap(),
        "strategy engine",
    );
    let foreign_receipt = successful_receipt_for_assignment(
        &LanePassAssignment {
            claim: foreign_claim.clone(),
            lane: foreign_claim.lane,
            pass: foreign_claim.pass,
            worker_id: foreign_claim.worker_id.clone(),
            ..completed.clone()
        },
        "foreign completion proof",
    );
    let foreign_path = state_dir
        .join("receipts")
        .join(format!("{}.json", foreign_receipt.receipt_id));
    fs::write(
        &foreign_path,
        serde_json::to_string_pretty(&foreign_receipt).unwrap(),
    )
    .unwrap();
    let worker_path = &completed.paths.worker_path;
    let mut assignment_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(worker_path).unwrap()).unwrap();
    assignment_json["receipt_id"] = serde_json::Value::String(foreign_receipt.receipt_id.clone());
    assignment_json["receipt_path"] =
        serde_json::Value::String(foreign_path.to_string_lossy().to_string());
    fs::write(
        worker_path,
        serde_json::to_string_pretty(&assignment_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("stored receipt claim"));
}

#[test]
fn lane_pass_store_rejects_stored_receipt_worktree_identity_mismatch_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let state_dir = temp.path().join("state");
    let store = LanePassStore::new(
        LanePassConfig::new(&state_dir, "strategy engine").with_project_root(&project_a),
    )
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-stored-receipt-worktree").unwrap())
        .unwrap();
    let receipt = successful_receipt_for_assignment(&first, "completion proof");
    let completed = store
        .complete_pass_with_receipt(&first.worker_id, &receipt)
        .unwrap();

    let mut foreign_receipt = receipt.clone();
    foreign_receipt.receipt_id.clear();
    foreign_receipt.worktree_identity = Some(detect_worktree_metadata(&project_b).identity());
    foreign_receipt.receipt_id = foreign_receipt.receipt_id().unwrap();
    let foreign_path = state_dir
        .join("receipts")
        .join(format!("{}.json", foreign_receipt.receipt_id));
    fs::write(
        &foreign_path,
        serde_json::to_string_pretty(&foreign_receipt).unwrap(),
    )
    .unwrap();
    let worker_path = &completed.paths.worker_path;
    let mut assignment_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(worker_path).unwrap()).unwrap();
    assignment_json["receipt_id"] = serde_json::Value::String(foreign_receipt.receipt_id);
    assignment_json["receipt_path"] =
        serde_json::Value::String(foreign_path.to_string_lossy().to_string());
    fs::write(
        worker_path,
        serde_json::to_string_pretty(&assignment_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("receipt worktree identity"));
}

#[test]
fn lane_pass_store_rejects_tampered_handoff_from_claim_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-tampered-token").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "handoff token proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue with valid artifacts")
        .unwrap();
    let handoff_path = advanced.handoff_path.as_ref().unwrap();
    let mut handoff_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(handoff_path).unwrap()).unwrap();
    handoff_json["from_claim"] = serde_json::Value::String("tampered-claim-token".to_string());
    fs::write(
        handoff_path,
        serde_json::to_string_pretty(&handoff_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("stored handoff claim token"));
}

#[test]
fn lane_pass_store_rejects_tampered_handoff_summary_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-tampered-handoff-summary").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "handoff semantic proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue with valid artifacts")
        .unwrap();
    let handoff_path = advanced.handoff_path.as_ref().unwrap();
    let mut handoff_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(handoff_path).unwrap()).unwrap();
    handoff_json["summary"] = serde_json::Value::String("tampered summary".to_string());
    handoff_json
        .as_object_mut()
        .unwrap()
        .remove("payload_digest");
    fs::write(
        handoff_path,
        serde_json::to_string_pretty(&handoff_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("stored handoff summary"));
}

#[test]
fn lane_pass_store_rejects_tampered_handoff_next_action_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-tampered-handoff-next-action").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "handoff digest proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue with valid artifacts")
        .unwrap();
    let handoff_path = advanced.handoff_path.as_ref().unwrap();
    let mut handoff_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(handoff_path).unwrap()).unwrap();
    handoff_json["next_action"] = serde_json::Value::String("tampered next action".to_string());
    fs::write(
        handoff_path,
        serde_json::to_string_pretty(&handoff_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("handoff payload digest"));
}

#[test]
fn lane_pass_store_rejects_missing_handoff_worktree_identity_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).unwrap();
    let store = LanePassStore::new(
        LanePassConfig::new(temp.path().join("state"), "strategy engine")
            .with_project_root(&project_root),
    )
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-missing-handoff-identity").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "handoff identity proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::partial("continue in the next pass"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let advanced = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue with valid artifacts")
        .unwrap();
    let handoff_path = advanced.handoff_path.as_ref().unwrap();
    let mut handoff_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(handoff_path).unwrap()).unwrap();
    handoff_json
        .as_object_mut()
        .unwrap()
        .remove("worktree_identity");
    fs::write(
        handoff_path,
        serde_json::to_string_pretty(&handoff_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("stored handoff worktree identity"));
}

#[test]
fn lane_pass_store_rejects_assignment_status_claim_status_mismatch_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-status-mismatch").unwrap())
        .unwrap();
    let receipt = successful_receipt_for_assignment(&first, "completion proof");
    let completed = store
        .complete_pass_with_receipt(&first.worker_id, &receipt)
        .unwrap();
    let worker_path = &completed.paths.worker_path;
    let mut assignment_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(worker_path).unwrap()).unwrap();
    assignment_json["claim"]["status"] = serde_json::Value::String("claimed".to_string());
    fs::write(
        worker_path,
        serde_json::to_string_pretty(&assignment_json).unwrap(),
    )
    .unwrap();

    let err = store.worker_assignment(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("assignment status"));
}

#[test]
fn lane_pass_store_rejects_blocked_handoff_advancement() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();
    let first = store
        .claim(WorkerId::new("worker-blocked").unwrap())
        .unwrap();
    let mut receipt = ProofReceipt::new(first.claim.clone(), "blocked proof")
        .with_worktree_identity(first.worktree.identity())
        .with_command(CommandProof::new(
            "cargo fmt --check",
            VerificationClass::Small,
            CommandStatus::Blocked {
                reason: "waiting on isolated worktree".to_string(),
            },
        ))
        .with_outcome(OutcomeProof::blocked("waiting on isolated worktree"));
    receipt.receipt_id = receipt.receipt_id().unwrap();

    let err = store
        .next_pass_with_handoff(&first.worker_id, &receipt, "continue after blocker")
        .unwrap_err();

    assert!(err.to_string().contains("blocked handoff"));
}

#[test]
fn lane_pass_store_keeps_dot_and_underscore_worker_files_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(
        LanePassConfig::new(temp.path().join("state"), "strategy engine").with_max_lanes(30),
    )
    .unwrap();

    let underscore = store.claim(WorkerId::new("worker_a").unwrap()).unwrap();
    let dotted = store.claim(WorkerId::new("worker.a").unwrap()).unwrap();

    assert_ne!(underscore.worker_id, dotted.worker_id);
    assert_ne!(underscore.lane, dotted.lane);
    assert_ne!(underscore.paths.worker_path, dotted.paths.worker_path);
}

#[test]
fn lane_pass_store_peek_does_not_create_state_files() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let store = LanePassStore::new(LanePassConfig::new(&state_dir, "strategy engine")).unwrap();

    let peek = store.peek_next_claim().unwrap();

    assert_eq!(peek.lane, LaneId::new(1).unwrap());
    assert_eq!(peek.pass, PassNumber::new(1).unwrap());
    assert!(!state_dir.exists());
}

#[test]
fn lane_pass_store_rejects_next_pass_after_max_pass() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(
        LanePassConfig::new(temp.path().join("state"), "strategy engine")
            .with_max_lanes(30)
            .with_max_passes(1),
    )
    .unwrap();

    let first = store.claim(WorkerId::new("worker-max").unwrap()).unwrap();
    let err = store.next_pass(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("max passes"));
}

#[test]
fn lane_pass_store_does_not_cycle_into_active_lanes() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(
        LanePassConfig::new(temp.path().join("state"), "strategy engine")
            .with_max_lanes(2)
            .with_lane_cycling(true),
    )
    .unwrap();

    store.claim(WorkerId::new("worker-one").unwrap()).unwrap();
    store.claim(WorkerId::new("worker-two").unwrap()).unwrap();
    let err = store
        .claim(WorkerId::new("worker-three").unwrap())
        .unwrap_err();

    assert!(err.to_string().contains("no released lane"));
}

#[test]
fn lane_pass_store_recycles_released_lane_when_cycling_is_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(
        LanePassConfig::new(temp.path().join("state"), "strategy engine")
            .with_max_lanes(1)
            .with_lane_cycling(true),
    )
    .unwrap();

    let first = store.claim(WorkerId::new("worker-one").unwrap()).unwrap();
    let released = store.release_lane(&first.worker_id).unwrap();
    let second = store.claim(WorkerId::new("worker-two").unwrap()).unwrap();

    assert_eq!(
        released.claim.status,
        driven::strategy::ClaimStatus::Released
    );
    assert_eq!(second.lane, LaneId::new(1).unwrap());
    assert_eq!(second.worker_id.as_str(), "worker-two");
}

#[test]
fn lane_pass_store_mints_new_claim_identity_when_recycling_lane_for_same_worker() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(
        LanePassConfig::new(temp.path().join("state"), "strategy engine")
            .with_max_lanes(1)
            .with_lane_cycling(true),
    )
    .unwrap();

    let first = store
        .claim(WorkerId::new("worker-replay").unwrap())
        .unwrap();
    let stale_receipt = successful_receipt_for_assignment(&first, "old proof");
    store.release_lane(&first.worker_id).unwrap();
    let second = store
        .claim(WorkerId::new("worker-replay").unwrap())
        .unwrap();

    assert_eq!(second.lane, first.lane);
    assert_eq!(second.pass, first.pass);
    assert_eq!(second.worker_id, first.worker_id);
    assert_ne!(second.claim.claim_id, first.claim.claim_id);
    assert_ne!(second.claim.token, first.claim.token);

    let err = store
        .complete_pass_with_receipt(&second.worker_id, &stale_receipt)
        .unwrap_err();

    assert!(err.to_string().contains("receipt claim"));
}

#[test]
fn lane_pass_store_rejects_next_pass_after_release() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "strategy engine",
    ))
    .unwrap();

    let first = store
        .claim(WorkerId::new("worker-release").unwrap())
        .unwrap();
    store.release_lane(&first.worker_id).unwrap();
    let err = store.next_pass(&first.worker_id).unwrap_err();

    assert!(err.to_string().contains("not active"));
}

#[test]
fn lane_pass_store_complete_pass_requires_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let store = LanePassStore::new(LanePassConfig::new(
        temp.path().join("state"),
        "proof completion",
    ))
    .unwrap();
    let claim = store
        .claim(WorkerId::new("worker-proof-required").unwrap())
        .unwrap();

    let err = store.complete_pass(&claim.worker_id).unwrap_err();

    assert!(err.to_string().contains("proof receipt"));
}

#[test]
fn lane_pass_store_recovers_stale_lock_file() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join(".driven-lane-pass.lock"),
        r#"{
  "schema": "driven.lane_pass.lock.v1",
  "owner_pid": 1,
  "acquired_unix_seconds": 1,
  "stale_after_seconds": 1
}
"#,
    )
    .unwrap();

    let store = LanePassStore::new(LanePassConfig::new(&state_dir, "strategy engine")).unwrap();
    let claim = store
        .claim(WorkerId::new("worker-stale-lock").unwrap())
        .unwrap();

    assert_eq!(claim.lane, LaneId::new(1).unwrap());
    assert!(!state_dir.join(".driven-lane-pass.lock").exists());
}

#[test]
fn lane_pass_store_blocks_fresh_lock_file_with_owner_context() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join(".driven-lane-pass.lock"),
        format!(
            r#"{{
  "schema": "driven.lane_pass.lock.v1",
  "owner_pid": 4242,
  "acquired_unix_seconds": {},
  "stale_after_seconds": 1800
}}
"#,
            4_102_444_800_u64
        ),
    )
    .unwrap();

    let store = LanePassStore::new(LanePassConfig::new(&state_dir, "strategy engine")).unwrap();
    let err = store
        .claim(WorkerId::new("worker-blocked-lock").unwrap())
        .unwrap_err();

    assert!(err.to_string().contains("locked"));
    assert!(err.to_string().contains("4242"));
}

#[test]
fn strategy_cli_state_json_peek_claim_next_regression() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");

    let peek_json =
        StrategyCommand::peek_state(state_dir.clone(), "cli state", 30, 3, true).unwrap();
    let peek: LanePassAssignment = serde_json::from_str(&peek_json).unwrap();
    assert_eq!(peek.status, LanePassAssignmentStatus::Peeked);
    assert_eq!(peek.lane, LaneId::new(1).unwrap());
    assert_eq!(peek.pass, PassNumber::new(1).unwrap());
    assert_eq!(peek.worker_id.as_str(), "unclaimed");
    assert!(!state_dir.exists());

    let claim_json =
        StrategyCommand::claim_state(state_dir.clone(), "cli state", 30, 3, "worker-cli", true)
            .unwrap();
    let claim: LanePassAssignment = serde_json::from_str(&claim_json).unwrap();
    assert_eq!(claim.status, LanePassAssignmentStatus::Claimed);
    assert_eq!(claim.lane, LaneId::new(1).unwrap());
    assert_eq!(claim.pass, PassNumber::new(1).unwrap());

    let next_json =
        StrategyCommand::next_state(state_dir, "cli state", 30, 3, "worker-cli", true).unwrap();
    let next: LanePassAssignment = serde_json::from_str(&next_json).unwrap();
    assert_eq!(next.status, LanePassAssignmentStatus::Advanced);
    assert_eq!(next.lane, LaneId::new(1).unwrap());
    assert_eq!(next.pass, PassNumber::new(2).unwrap());
}

#[test]
fn strategy_cli_state_json_release_and_complete_regression() {
    let temp = tempfile::tempdir().unwrap();
    let release_state_dir = temp.path().join("release-state");
    StrategyCommand::claim_state(
        release_state_dir.clone(),
        "cli state",
        30,
        3,
        "worker-release",
        true,
    )
    .unwrap();

    let released_json = StrategyCommand::release_state(
        release_state_dir,
        "cli state",
        30,
        3,
        "worker-release",
        true,
    )
    .unwrap();
    let released: LanePassAssignment = serde_json::from_str(&released_json).unwrap();
    assert_eq!(released.status, LanePassAssignmentStatus::Released);
    assert_eq!(
        released.claim.status,
        driven::strategy::ClaimStatus::Released
    );

    let complete_state_dir = temp.path().join("complete-state");
    let complete_claim_json = StrategyCommand::claim_state(
        complete_state_dir.clone(),
        "cli state",
        30,
        3,
        "worker-complete",
        true,
    )
    .unwrap();
    let complete_assignment: LanePassAssignment =
        serde_json::from_str(&complete_claim_json).unwrap();
    let receipt = successful_receipt_for_assignment(&complete_assignment, "completion proof");
    let receipt_path = temp.path().join("complete-receipt.json");
    fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let completed_json = StrategyCommand::complete_state_with_receipt(
        complete_state_dir,
        "cli state",
        30,
        3,
        "worker-complete",
        &receipt_path,
        true,
    )
    .unwrap();
    let completed: LanePassAssignment = serde_json::from_str(&completed_json).unwrap();
    assert_eq!(completed.status, LanePassAssignmentStatus::Completed);
    assert_eq!(
        completed.claim.status,
        driven::strategy::ClaimStatus::Completed
    );
}

#[test]
fn strategy_cli_complete_state_without_receipt_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("complete-state");
    StrategyCommand::claim_state(state_dir.clone(), "cli state", 30, 3, "worker-proof", true)
        .unwrap();

    let err = StrategyCommand::complete_state(state_dir, "cli state", 30, 3, "worker-proof", true)
        .unwrap_err();

    assert!(err.to_string().contains("proof receipt"));
}

#[test]
fn strategy_cli_complete_requires_matching_verified_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("complete-state");
    let claim_json =
        StrategyCommand::claim_state(state_dir.clone(), "cli state", 30, 3, "worker-proof", true)
            .unwrap();
    let assignment: LanePassAssignment = serde_json::from_str(&claim_json).unwrap();
    let receipt = successful_receipt_for_assignment(&assignment, "completion proof");
    let receipt_path = temp.path().join("receipt.json");
    fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();

    let completed_json = StrategyCommand::complete_state_with_receipt(
        state_dir,
        "cli state",
        30,
        3,
        "worker-proof",
        &receipt_path,
        true,
    )
    .unwrap();
    let completed: LanePassAssignment = serde_json::from_str(&completed_json).unwrap();

    assert_eq!(completed.status, LanePassAssignmentStatus::Completed);
    assert_eq!(
        completed.receipt_id.as_deref(),
        Some(receipt.receipt_id.as_str())
    );
    let stored_receipt_path = completed.receipt_path.as_ref().unwrap();
    let stored_receipt: ProofReceipt =
        serde_json::from_str(&fs::read_to_string(stored_receipt_path).unwrap()).unwrap();
    assert_eq!(stored_receipt.receipt_id, receipt.receipt_id);
}

#[test]
fn strategy_cli_generates_evidence_receipt_and_completes_claim() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("complete-state");
    StrategyCommand::claim_state(
        state_dir.clone(),
        "cli evidence",
        30,
        3,
        "worker-proof",
        true,
    )
    .unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let (program, args) = small_echo_command();

    let receipt_json = StrategyCommand::receipt_state_with_options(
        state_dir.clone(),
        "cli evidence",
        30,
        3,
        "worker-proof",
        "evidence-backed completion proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        StrategyStateOptions::default(),
    )
    .unwrap();
    let receipt: ProofReceipt = serde_json::from_str(&receipt_json).unwrap();

    assert!(!receipt.receipt_id.is_empty());
    assert_eq!(receipt.commands.len(), 1);
    assert!(receipt.commands[0].evidence.is_some());
    assert_eq!(
        receipt.receipt_id,
        serde_json::from_str::<ProofReceipt>(&fs::read_to_string(&receipt_path).unwrap())
            .unwrap()
            .receipt_id
    );

    let completed_json = StrategyCommand::complete_state_with_receipt(
        state_dir,
        "cli evidence",
        30,
        3,
        "worker-proof",
        &receipt_path,
        true,
    )
    .unwrap();
    let completed: LanePassAssignment = serde_json::from_str(&completed_json).unwrap();

    assert_eq!(completed.status, LanePassAssignmentStatus::Completed);
    assert_eq!(
        completed.receipt_id.as_deref(),
        Some(receipt.receipt_id.as_str())
    );
}

#[test]
fn strategy_cli_receipt_records_output_cap_policy() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("output-cap-state");
    StrategyCommand::claim_state(
        state_dir.clone(),
        "cli output cap",
        30,
        3,
        "worker-output-cap",
        true,
    )
    .unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let (program, args) = noisy_stdout_command();

    let receipt_json = StrategyCommand::receipt_state_with_execution_options(
        state_dir,
        "cli output cap",
        30,
        3,
        "worker-output-cap",
        "output cap proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        StrategyReceiptExecutionOptions::default().with_max_output_bytes(64),
        StrategyStateOptions::default(),
    )
    .unwrap();
    let receipt: ProofReceipt = serde_json::from_str(&receipt_json).unwrap();
    let evidence = receipt.commands[0].evidence.as_ref().unwrap();

    assert_eq!(evidence.output_limit_bytes, Some(64));
    assert!(evidence.stdout_bytes > 64);
    assert!(evidence.stdout_truncated);
    assert!(!receipt_json.contains("driven-proof-line"));
}

#[test]
fn strategy_cli_receipt_json_redacts_secret_summary_but_persists_canonical_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("redacted-json-state");
    StrategyCommand::claim_state(
        state_dir.clone(),
        "cli receipt redaction",
        30,
        3,
        "worker-redacted-json",
        true,
    )
    .unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let (program, args) = small_echo_command();

    let receipt_json = StrategyCommand::receipt_state_with_options(
        state_dir,
        "cli receipt redaction",
        30,
        3,
        "worker-redacted-json",
        "checked token=summary-secret",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        StrategyStateOptions::default(),
    )
    .unwrap();
    let rendered: ProofReceipt = serde_json::from_str(&receipt_json).unwrap();
    let persisted: ProofReceipt =
        serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();

    assert!(rendered.redacted);
    assert!(rendered.redacted_payload_digest.is_some());
    assert!(!receipt_json.contains("summary-secret"));
    assert_eq!(rendered.receipt_id, persisted.receipt_id);
    assert!(!persisted.redacted);
    assert_eq!(persisted.summary(), "checked token=summary-secret");
}

#[test]
fn strategy_cli_receipt_blocks_when_claim_changes_before_receipt_write() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("stale-receipt-state");
    let claimed_json = StrategyCommand::claim_state(
        state_dir.clone(),
        "cli stale receipt",
        30,
        3,
        "worker-stale-receipt",
        true,
    )
    .unwrap();
    let claimed: LanePassAssignment = serde_json::from_str(&claimed_json).unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let (program, args) = remove_path_command(&claimed.paths.worker_path);

    let receipt_json = StrategyCommand::receipt_state_with_options(
        state_dir,
        "cli stale receipt",
        30,
        3,
        "worker-stale-receipt",
        "stale proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        StrategyStateOptions::default(),
    )
    .unwrap();
    let persisted: ProofReceipt =
        serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();

    assert!(receipt_path.exists());
    assert!(!receipt_json.contains("stale proof failed"));
    assert!(persisted.commands[0].evidence.is_some());
    assert!(persisted.outcomes.iter().any(|outcome| matches!(
        outcome,
        OutcomeProof::Blocked { reason } if reason.contains("changed before receipt write")
    )));
}

#[test]
fn strategy_cli_receipt_persists_worktree_identity() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("receipt-worktree-identity-state");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).unwrap();
    let claimed_json = StrategyCommand::claim_state_with_options(
        state_dir.clone(),
        "cli receipt worktree identity persisted",
        30,
        3,
        "worker-receipt-identity",
        true,
        StrategyStateOptions::default().with_project_root(&project_root),
    )
    .unwrap();
    let claimed: LanePassAssignment = serde_json::from_str(&claimed_json).unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let (program, args) = small_echo_command();

    let receipt_json = StrategyCommand::receipt_state_with_options(
        state_dir,
        "cli receipt worktree identity persisted",
        30,
        3,
        "worker-receipt-identity",
        "identity proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        StrategyStateOptions::default().with_project_root(&project_root),
    )
    .unwrap();
    let rendered: serde_json::Value = serde_json::from_str(&receipt_json).unwrap();
    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();

    assert_eq!(
        persisted["worktree_identity"]["input_root"],
        serde_json::Value::String(
            claimed
                .worktree
                .identity()
                .input_root
                .to_string_lossy()
                .to_string()
        )
    );
    assert_eq!(
        rendered["worktree_identity"]["input_root"],
        persisted["worktree_identity"]["input_root"]
    );
}

#[test]
fn strategy_cli_receipt_refuses_to_run_when_current_worktree_differs_from_claim() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("receipt-worktree-state");
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    StrategyCommand::claim_state_with_options(
        state_dir.clone(),
        "cli receipt worktree identity",
        30,
        3,
        "worker-receipt-worktree",
        true,
        StrategyStateOptions::default().with_project_root(&project_a),
    )
    .unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let sentinel = temp.path().join("receipt-command-ran.txt");
    let (program, args) = write_path_command(&sentinel);

    let result = StrategyCommand::receipt_state_with_options(
        state_dir,
        "cli receipt worktree identity",
        30,
        3,
        "worker-receipt-worktree",
        "wrong worktree proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        StrategyStateOptions::default().with_project_root(&project_b),
    );

    let error = result.unwrap_err().to_string();
    assert!(error.contains("worktree identity changed"));
    assert!(!sentinel.exists());
    assert!(!receipt_path.exists());
}

#[test]
fn strategy_cli_receipt_timeout_writes_blocked_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("timeout-state");
    StrategyCommand::claim_state(
        state_dir.clone(),
        "cli timeout",
        30,
        3,
        "worker-timeout",
        true,
    )
    .unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let (program, args) = slow_command();

    let receipt_json = StrategyCommand::receipt_state_with_execution_options(
        state_dir,
        "cli timeout",
        30,
        3,
        "worker-timeout",
        "timeout proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        StrategyReceiptExecutionOptions::default().with_timeout_ms(50),
        StrategyStateOptions::default(),
    )
    .unwrap();
    let receipt: ProofReceipt = serde_json::from_str(&receipt_json).unwrap();

    assert!(matches!(
        receipt.commands[0].status,
        CommandStatus::Blocked { .. }
    ));
    assert!(receipt.commands[0].evidence.is_none());
    assert!(
        receipt
            .outcomes
            .iter()
            .any(|outcome| matches!(outcome, OutcomeProof::Blocked { .. }))
    );
    assert!(!receipt.receipt_id.is_empty());
    assert!(receipt_path.exists());
}

#[test]
fn strategy_cli_next_with_handoff_receipt_persists_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("handoff-state");
    StrategyCommand::claim_state(
        state_dir.clone(),
        "cli handoff",
        30,
        3,
        "worker-proof",
        true,
    )
    .unwrap();
    let receipt_path = temp.path().join("receipt.json");
    let (program, args) = small_echo_command();
    StrategyCommand::receipt_state_with_options(
        state_dir.clone(),
        "cli handoff",
        30,
        3,
        "worker-proof",
        "handoff receipt proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        StrategyStateOptions::default(),
    )
    .unwrap();

    let advanced_json = StrategyCommand::next_state_with_handoff_options(
        state_dir,
        "cli handoff",
        30,
        3,
        "worker-proof",
        &receipt_path,
        "continue with pass 2",
        true,
        StrategyStateOptions::default(),
    )
    .unwrap();
    let advanced: LanePassAssignment = serde_json::from_str(&advanced_json).unwrap();

    assert_eq!(advanced.status, LanePassAssignmentStatus::Advanced);
    assert_eq!(advanced.pass, PassNumber::new(2).unwrap());
    assert!(advanced.handoff_path.as_ref().unwrap().exists());
}

#[test]
fn strategy_cli_next_options_can_require_durable_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("handoff-state");
    let options = StrategyStateOptions::default().with_handoff_required_for_next(true);
    StrategyCommand::claim_state_with_options(
        state_dir.clone(),
        "cli durable required",
        30,
        3,
        "worker-proof",
        true,
        options.clone(),
    )
    .unwrap();

    let err = StrategyCommand::next_state_with_options(
        state_dir.clone(),
        "cli durable required",
        30,
        3,
        "worker-proof",
        true,
        options.clone(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("durable handoff"));

    let receipt_path = temp.path().join("receipt.json");
    let (program, args) = small_echo_command();
    StrategyCommand::receipt_state_with_options(
        state_dir.clone(),
        "cli durable required",
        30,
        3,
        "worker-proof",
        "handoff receipt proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        options.clone(),
    )
    .unwrap();

    let advanced_json = StrategyCommand::next_state_with_handoff_options(
        state_dir,
        "cli durable required",
        30,
        3,
        "worker-proof",
        &receipt_path,
        "continue with pass 2",
        true,
        options,
    )
    .unwrap();
    let advanced: LanePassAssignment = serde_json::from_str(&advanced_json).unwrap();

    assert_eq!(advanced.status, LanePassAssignmentStatus::Advanced);
    assert_eq!(advanced.pass, PassNumber::new(2).unwrap());
}

#[test]
fn strategy_cli_complete_rejects_cross_state_same_worktree_receipt_replay() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let state_a = temp.path().join("complete-state-a");
    let state_b = temp.path().join("complete-state-b");
    let options = StrategyStateOptions::default().with_project_root(&project);
    StrategyCommand::claim_state_with_options(
        state_a.clone(),
        "cli cross-state",
        30,
        3,
        "worker-proof",
        true,
        options.clone(),
    )
    .unwrap();
    StrategyCommand::claim_state_with_options(
        state_b.clone(),
        "cli cross-state",
        30,
        3,
        "worker-proof",
        true,
        options.clone(),
    )
    .unwrap();
    let receipt_path = temp.path().join("state-a-receipt.json");
    let (program, args) = small_echo_command();
    StrategyCommand::receipt_state_with_options(
        state_a,
        "cli cross-state",
        30,
        3,
        "worker-proof",
        "state a proof",
        VerificationClass::Small,
        program,
        &args,
        &receipt_path,
        true,
        options.clone(),
    )
    .unwrap();

    let err = StrategyCommand::complete_state_with_receipt_options(
        state_b,
        "cli cross-state",
        30,
        3,
        "worker-proof",
        &receipt_path,
        true,
        options,
    )
    .unwrap_err();

    assert!(err.to_string().contains("state identity"));
}

#[test]
fn strategy_cli_complete_rejects_partial_or_foreign_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("complete-state");
    let claim_json =
        StrategyCommand::claim_state(state_dir.clone(), "cli state", 30, 3, "worker-proof", true)
            .unwrap();
    let assignment: LanePassAssignment = serde_json::from_str(&claim_json).unwrap();
    let mut partial_receipt = ProofReceipt::new(assignment.claim.clone(), "mixed proof")
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::verified("small command passed"))
        .with_outcome(OutcomeProof::partial("follow-up still pending"));
    partial_receipt.receipt_id = partial_receipt.receipt_id().unwrap();
    let partial_receipt_path = temp.path().join("partial-receipt.json");
    fs::write(
        &partial_receipt_path,
        serde_json::to_string_pretty(&partial_receipt).unwrap(),
    )
    .unwrap();

    let partial_err = StrategyCommand::complete_state_with_receipt(
        state_dir.clone(),
        "cli state",
        30,
        3,
        "worker-proof",
        &partial_receipt_path,
        true,
    )
    .unwrap_err();

    assert!(partial_err.to_string().contains("partial"));

    let foreign_claim = LaneClaim::new(
        LaneId::new(2).unwrap(),
        PassNumber::first(),
        WorkerId::new("worker-other").unwrap(),
        "cli state",
    );
    let mut receipt = ProofReceipt::new(foreign_claim, "foreign proof")
        .with_command(observed_small_command("cargo fmt --check"))
        .with_outcome(OutcomeProof::verified("small command passed"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let receipt_path = temp.path().join("foreign-receipt.json");
    fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();

    let err = StrategyCommand::complete_state_with_receipt(
        state_dir,
        "cli state",
        30,
        3,
        "worker-proof",
        &receipt_path,
        true,
    )
    .unwrap_err();

    assert!(err.to_string().contains("receipt claim"));
}

#[test]
fn strategy_cli_complete_requires_canonical_receipt_id() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("complete-state");
    let claim_json =
        StrategyCommand::claim_state(state_dir.clone(), "cli state", 30, 3, "worker-proof", true)
            .unwrap();
    let assignment: LanePassAssignment = serde_json::from_str(&claim_json).unwrap();
    let receipt = ProofReceipt::new(assignment.claim.clone(), "completion proof")
        .with_command(CommandProof::passed(
            "cargo fmt --check",
            VerificationClass::Small,
        ))
        .with_outcome(OutcomeProof::verified("small command passed"));
    let receipt_path = temp.path().join("receipt-without-id.json");
    fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();

    let err = StrategyCommand::complete_state_with_receipt(
        state_dir,
        "cli state",
        30,
        3,
        "worker-proof",
        &receipt_path,
        true,
    )
    .unwrap_err();

    assert!(err.to_string().contains("receipt id"));
}

#[test]
fn strategy_cli_complete_does_not_claim_unknown_worker_from_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("complete-state");
    let claim = LaneClaim::new(
        LaneId::new(1).unwrap(),
        PassNumber::first(),
        WorkerId::new("worker-proof").unwrap(),
        "cli state",
    );
    let mut receipt = ProofReceipt::new(claim, "completion proof")
        .with_command(CommandProof::passed(
            "cargo fmt --check",
            VerificationClass::Small,
        ))
        .with_outcome(OutcomeProof::verified("small command passed"));
    receipt.receipt_id = receipt.receipt_id().unwrap();
    let receipt_path = temp.path().join("receipt.json");
    fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();

    let err = StrategyCommand::complete_state_with_receipt(
        state_dir.clone(),
        "cli state",
        30,
        3,
        "worker-proof",
        &receipt_path,
        true,
    )
    .unwrap_err();

    assert!(err.to_string().contains("no lane claim"));
    assert!(!state_dir.exists());
}

#[test]
fn strategy_cli_inspect_worktree_json_includes_isolation_plan() {
    let temp = tempfile::tempdir().unwrap();
    let output = StrategyCommand::inspect_worktree(temp.path(), true).unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(
        value["worktree"]["kind"],
        serde_json::Value::String("not_repository".to_string())
    );
    assert_eq!(
        value["worktree_plan"]["creation_decision"],
        serde_json::Value::String("blocked".to_string())
    );
    assert!(value["worktree_plan"]["blockers"].is_array());
}

#[test]
fn strategy_cli_claim_markdown_escapes_scope_and_next_action() {
    let temp = tempfile::tempdir().unwrap();
    let markdown = StrategyCommand::claim(
        temp.path(),
        1,
        1,
        "worker-cli",
        "cli scope\n## Forged Scope | cell",
        "next\n## Forged Next | cell",
        false,
    )
    .unwrap();

    assert!(!markdown.lines().any(|line| line == "## Forged Scope"));
    assert!(!markdown.lines().any(|line| line == "## Forged Next"));
    assert!(markdown.contains("cli scope<br>## Forged Scope \\| cell"));
    assert!(markdown.contains("next<br>## Forged Next \\| cell"));
}

fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn initialize_clean_repo(root: &std::path::Path) {
    run_git(root, &["init"]).unwrap();
    run_git(root, &["config", "user.email", "dx@example.test"]).unwrap();
    run_git(root, &["config", "user.name", "DX Test"]).unwrap();
    fs::write(root.join("README.md"), "clean\n").unwrap();
    run_git(root, &["add", "README.md"]).unwrap();
    run_git(root, &["commit", "-m", "initial"]).unwrap();
}

fn run_git(root: &std::path::Path, args: &[&str]) -> std::io::Result<()> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}
