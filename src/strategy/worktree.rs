use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitCheckoutKind {
    NotRepository,
    MainWorktree,
    LinkedWorktree,
    Submodule,
    Bare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeIsolationMode {
    NoGitRepository,
    MainWorktree,
    LinkedWorktree,
    Submodule,
    BareRepository,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeCreationDecision {
    Blocked,
    CreateGitWorktree,
    ReuseCurrentWorktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeMetadata {
    pub kind: GitCheckoutKind,
    pub input_root: PathBuf,
    pub worktree_root: Option<PathBuf>,
    pub git_dir: Option<PathBuf>,
    pub common_dir: Option<PathBuf>,
    pub superproject_root: Option<PathBuf>,
    pub branch: Option<String>,
    pub detached: bool,
    pub head_sha: Option<String>,
    pub remote: Option<String>,
    pub is_dirty: Option<bool>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub detection_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeIdentity {
    pub kind: GitCheckoutKind,
    pub input_root: PathBuf,
    pub worktree_root: Option<PathBuf>,
    pub git_dir: Option<PathBuf>,
    pub common_dir: Option<PathBuf>,
    pub superproject_root: Option<PathBuf>,
    pub branch: Option<String>,
    pub remote: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeIsolationPlan {
    pub schema: String,
    pub metadata: WorktreeMetadata,
    pub mode: WorktreeIsolationMode,
    pub creation_decision: WorktreeCreationDecision,
    pub can_create_git_worktree: bool,
    pub native_isolation_detected: bool,
    pub execution_root: Option<PathBuf>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

impl WorktreeMetadata {
    pub fn is_not_repository(&self) -> bool {
        self.kind == GitCheckoutKind::NotRepository
    }

    pub fn identity(&self) -> WorktreeIdentity {
        WorktreeIdentity::from_metadata(self)
    }

    pub fn has_same_identity(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl WorktreeIdentity {
    pub fn from_metadata(metadata: &WorktreeMetadata) -> Self {
        let input_root = metadata
            .worktree_root
            .clone()
            .unwrap_or_else(|| metadata.input_root.clone());
        Self {
            kind: metadata.kind,
            input_root,
            worktree_root: metadata.worktree_root.clone(),
            git_dir: metadata.git_dir.clone(),
            common_dir: metadata.common_dir.clone(),
            superproject_root: metadata.superproject_root.clone(),
            branch: metadata.branch.clone(),
            remote: metadata.remote.clone(),
        }
    }
}

impl WorktreeIsolationPlan {
    pub fn from_metadata(metadata: WorktreeMetadata) -> Self {
        let mut blockers = Vec::new();
        let mut warnings = Vec::new();

        let (mode, creation_decision, can_create_git_worktree, native_isolation_detected) =
            match metadata.kind {
                GitCheckoutKind::NotRepository => {
                    blockers.push(
                        "path is not a Git repository; git worktree creation is blocked"
                            .to_string(),
                    );
                    if let Some(error) = &metadata.detection_error {
                        blockers.push(error.clone());
                    }
                    (
                        WorktreeIsolationMode::NoGitRepository,
                        WorktreeCreationDecision::Blocked,
                        false,
                        false,
                    )
                }
                GitCheckoutKind::Bare => {
                    blockers.push(
                        "bare repository has no working tree; provide an explicit checkout target before isolation"
                            .to_string(),
                    );
                    (
                        WorktreeIsolationMode::BareRepository,
                        WorktreeCreationDecision::Blocked,
                        false,
                        false,
                    )
                }
                GitCheckoutKind::LinkedWorktree => {
                    if metadata.is_dirty == Some(true) {
                        warnings.push(
                            "linked worktree has local changes; confirm ownership before reuse"
                                .to_string(),
                        );
                    }
                    if metadata.detached {
                        warnings.push(
                            "linked worktree is detached; preserve external orchestration metadata"
                                .to_string(),
                        );
                    }
                    (
                        WorktreeIsolationMode::LinkedWorktree,
                        WorktreeCreationDecision::ReuseCurrentWorktree,
                        false,
                        true,
                    )
                }
                GitCheckoutKind::Submodule => {
                    warnings.push(
                        "path is a Git submodule; reuse current checkout unless the superproject owns orchestration"
                            .to_string(),
                    );
                    if metadata.is_dirty == Some(true) {
                        warnings.push(
                            "submodule has local changes; confirm ownership before reuse"
                                .to_string(),
                        );
                    }
                    (
                        WorktreeIsolationMode::Submodule,
                        WorktreeCreationDecision::ReuseCurrentWorktree,
                        false,
                        true,
                    )
                }
                GitCheckoutKind::MainWorktree => {
                    if metadata.is_dirty == Some(true) {
                        blockers.push(
                            "main worktree has local changes; commit, stash, or claim an existing isolated worktree before creating isolation"
                                .to_string(),
                        );
                    }
                    if metadata.detached {
                        blockers.push(
                            "main worktree must be on a branch with a resolved HEAD before creating a git worktree"
                                .to_string(),
                        );
                    }
                    if metadata.head_sha.is_none() {
                        blockers.push(
                            "main worktree must have a branch with a resolved HEAD before creating a git worktree"
                                .to_string(),
                        );
                    }
                    let can_create = blockers.is_empty();
                    (
                        WorktreeIsolationMode::MainWorktree,
                        if can_create {
                            WorktreeCreationDecision::CreateGitWorktree
                        } else {
                            WorktreeCreationDecision::Blocked
                        },
                        can_create,
                        false,
                    )
                }
            };

        let execution_root = match mode {
            WorktreeIsolationMode::NoGitRepository | WorktreeIsolationMode::BareRepository => None,
            _ => metadata.worktree_root.clone(),
        };

        Self {
            schema: "driven.worktree_isolation_plan.v1".to_string(),
            metadata,
            mode,
            creation_decision,
            can_create_git_worktree,
            native_isolation_detected,
            execution_root,
            blockers,
            warnings,
        }
    }
}

pub fn plan_worktree_isolation(root: &Path) -> WorktreeIsolationPlan {
    WorktreeIsolationPlan::from_metadata(detect_worktree_metadata(root))
}

pub fn detect_worktree_metadata(root: &Path) -> WorktreeMetadata {
    let input_root = normalize_worktree_root(root);
    let root = input_root.as_path();
    let worktree_root = match git(root, &["rev-parse", "--show-toplevel"]) {
        Ok(value) => PathBuf::from(value),
        Err(error) => {
            if git(root, &["rev-parse", "--is-bare-repository"])
                .map(|value| value == "true")
                .unwrap_or(false)
            {
                return bare_metadata(root, input_root.clone());
            }

            return WorktreeMetadata {
                kind: GitCheckoutKind::NotRepository,
                input_root,
                worktree_root: None,
                git_dir: None,
                common_dir: None,
                superproject_root: None,
                branch: None,
                detached: false,
                head_sha: None,
                remote: None,
                is_dirty: None,
                ahead: None,
                behind: None,
                detection_error: Some(error),
            };
        }
    };

    let bare = git(root, &["rev-parse", "--is-bare-repository"])
        .map(|value| value == "true")
        .unwrap_or(false);
    let git_dir = git(root, &["rev-parse", "--git-dir"])
        .ok()
        .map(|path| normalize_git_path(root, path));
    let common_dir = git(root, &["rev-parse", "--git-common-dir"])
        .ok()
        .map(|path| normalize_git_path(root, path));
    let superproject_root = git(root, &["rev-parse", "--show-superproject-working-tree"])
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    let kind = if bare {
        GitCheckoutKind::Bare
    } else if superproject_root.is_some() {
        GitCheckoutKind::Submodule
    } else if git_dir.is_some() && common_dir.is_some() && git_dir != common_dir {
        GitCheckoutKind::LinkedWorktree
    } else {
        GitCheckoutKind::MainWorktree
    };

    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|value| value != "HEAD");
    let detached = branch.is_none();
    let head_sha = git(root, &["rev-parse", "HEAD"]).ok();
    let remote = git(root, &["config", "--get", "remote.origin.url"]).ok();
    let is_dirty = git(root, &["status", "--porcelain"])
        .ok()
        .map(|value| !value.trim().is_empty());
    let (ahead, behind) = ahead_behind(root);

    WorktreeMetadata {
        kind,
        input_root,
        worktree_root: Some(worktree_root),
        git_dir,
        common_dir,
        superproject_root,
        branch,
        detached,
        head_sha,
        remote,
        is_dirty,
        ahead,
        behind,
        detection_error: None,
    }
}

fn bare_metadata(root: &Path, input_root: PathBuf) -> WorktreeMetadata {
    let git_dir = git(root, &["rev-parse", "--git-dir"])
        .ok()
        .map(|path| normalize_git_path(root, path));
    let common_dir = git(root, &["rev-parse", "--git-common-dir"])
        .ok()
        .map(|path| normalize_git_path(root, path));
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|value| value != "HEAD");

    WorktreeMetadata {
        kind: GitCheckoutKind::Bare,
        input_root,
        worktree_root: None,
        git_dir,
        common_dir,
        superproject_root: None,
        branch: branch.clone(),
        detached: branch.is_none(),
        head_sha: git(root, &["rev-parse", "HEAD"]).ok(),
        remote: git(root, &["config", "--get", "remote.origin.url"]).ok(),
        is_dirty: None,
        ahead: None,
        behind: None,
        detection_error: None,
    }
}

pub(crate) fn normalize_worktree_root(root: &Path) -> PathBuf {
    if let Ok(canonical) = root.canonicalize() {
        return normalize_platform_path(canonical);
    }
    if root.is_absolute() {
        return normalize_platform_path(root.to_path_buf());
    }
    let absolute = std::env::current_dir()
        .map(|cwd| cwd.join(root))
        .unwrap_or_else(|_| root.to_path_buf());
    normalize_platform_path(absolute)
}

#[cfg(windows)]
fn normalize_platform_path(path: PathBuf) -> PathBuf {
    let path_text = path.to_string_lossy();
    if let Some(rest) = path_text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{}", rest));
    }
    if let Some(rest) = path_text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

#[cfg(not(windows))]
fn normalize_platform_path(path: PathBuf) -> PathBuf {
    path
}

fn normalize_git_path(root: &Path, path: String) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn ahead_behind(root: &Path) -> (Option<u32>, Option<u32>) {
    let output = match git(
        root,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    ) {
        Ok(output) => output,
        Err(_) => return (None, None),
    };

    let mut parts = output.split_whitespace();
    let behind = parts.next().and_then(|value| value.parse().ok());
    let ahead = parts.next().and_then(|value| value.parse().ok());
    (ahead, behind)
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("failed to run git: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("git {:?} exited with {}", args, output.status))
        } else {
            Err(stderr)
        }
    }
}
