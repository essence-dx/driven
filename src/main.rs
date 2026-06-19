//! Driven CLI - AI Development Orchestrator
//!
//! Command-line interface for managing AI coding rules across editors.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// AI Development Orchestrator
#[derive(Parser)]
#[command(name = "driven")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Verbose output
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize Driven in the current project
    Init {
        /// Run in interactive mode
        #[arg(short, long)]
        interactive: bool,
    },
    /// Synchronize rules to all editors
    Sync {
        /// Watch for changes
        #[arg(short, long)]
        watch: bool,
    },
    /// Convert rules between formats
    Convert {
        /// Input file
        input: PathBuf,
        /// Output file
        output: PathBuf,
        /// Target editor format
        #[arg(short, long)]
        editor: Option<String>,
    },
    /// Manage templates
    Template {
        #[command(subcommand)]
        action: TemplateAction,
    },
    /// Analyze project for context
    Analyze {
        /// Generate context rules
        #[arg(short, long)]
        context: bool,
        /// Index the codebase
        #[arg(short, long)]
        index: bool,
    },
    /// Validate rules
    Validate {
        /// Rules file to validate
        #[arg(default_value = ".driven/rules.md")]
        file: PathBuf,
        /// Strict mode (fail on warnings)
        #[arg(short, long)]
        strict: bool,
    },
    /// Manage agent hooks
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Manage agent steering rules
    Steer {
        #[command(subcommand)]
        action: SteerAction,
    },
    /// Model DX lane/pass strategy and worktree handoffs
    Strategy {
        #[command(subcommand)]
        action: StrategyAction,
    },
}

#[derive(Subcommand)]
enum TemplateAction {
    /// List available templates
    List,
    /// Search templates
    Search {
        /// Search query
        query: String,
    },
    /// Apply a template
    Apply {
        /// Template name
        name: String,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// List all hooks
    List,
    /// Add a new hook
    Add {
        /// Hook ID (unique identifier)
        id: String,
        /// Trigger type (file, git, build, test, manual, scheduled)
        #[arg(short, long)]
        trigger: String,
        /// Trigger value (patterns, operations, events, or command)
        #[arg(short = 'v', long)]
        trigger_value: String,
        /// Agent to invoke
        #[arg(short, long)]
        agent: String,
        /// Message to send to the agent
        #[arg(short, long)]
        message: String,
        /// Hook name (defaults to ID)
        #[arg(short, long)]
        name: Option<String>,
        /// Workflow to run
        #[arg(short, long)]
        workflow: Option<String>,
        /// Condition expression
        #[arg(short, long)]
        condition: Option<String>,
        /// Disable the hook initially
        #[arg(long)]
        disabled: bool,
    },
    /// Remove a hook
    Remove {
        /// Hook ID to remove
        id: String,
    },
    /// Manually trigger a hook
    Trigger {
        /// Command name to trigger
        command: String,
    },
    /// Enable a hook
    Enable {
        /// Hook ID to enable
        id: String,
    },
    /// Disable a hook
    Disable {
        /// Hook ID to disable
        id: String,
    },
    /// Show hook details
    Show {
        /// Hook ID to show
        id: String,
    },
}

#[derive(Subcommand)]
enum SteerAction {
    /// List all steering rules
    List,
    /// Add a new steering rule
    Add {
        /// Rule ID (unique identifier)
        id: String,
        /// Inclusion type (always, fileMatch, manual)
        #[arg(short, long, default_value = "always")]
        inclusion: String,
        /// Pattern (for fileMatch) or key (for manual)
        #[arg(short, long)]
        pattern: Option<String>,
        /// Rule content (markdown)
        #[arg(short, long)]
        content: String,
        /// Rule name (defaults to ID)
        #[arg(short, long)]
        name: Option<String>,
        /// Priority (lower = higher priority)
        #[arg(long)]
        priority: Option<u8>,
    },
    /// Remove a steering rule
    Remove {
        /// Rule ID to remove
        id: String,
    },
    /// Test which rules apply to a file
    Test {
        /// File path to test
        file: PathBuf,
        /// Manual keys to include
        #[arg(short, long)]
        keys: Vec<String>,
    },
    /// Show steering rule details
    Show {
        /// Rule ID to show
        id: String,
    },
    /// Get combined steering content for a context
    Inject {
        /// File path (optional)
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Manual keys to include
        #[arg(short, long)]
        keys: Vec<String>,
    },
}

#[derive(Subcommand)]
enum StrategyAction {
    /// Inspect Git/worktree metadata without mutating the workspace
    InspectWorktree {
        /// Path to inspect (defaults to the current directory)
        path: Option<PathBuf>,
        /// Render deterministic JSON
        #[arg(long)]
        json: bool,
    },
    /// Model a lane claim and next-pass handoff
    Claim {
        /// Lane number to claim
        #[arg(long)]
        lane: u8,
        /// Pass number for this worker pass
        #[arg(long, default_value_t = 1)]
        pass: u32,
        /// Stable worker identity
        #[arg(long)]
        worker_id: String,
        /// Lane scope or responsibility
        #[arg(long)]
        scope: String,
        /// Next action for the following pass
        #[arg(long)]
        next_action: String,
        /// Render deterministic JSON
        #[arg(long)]
        json: bool,
    },
    /// Preview the next lane claim without mutating state
    Peek {
        /// State directory for lane/pass files
        #[arg(long, default_value = ".driven/lanes")]
        state_dir: PathBuf,
        /// Lane scope or responsibility
        #[arg(long)]
        scope: String,
        /// Maximum lane count
        #[arg(long, default_value_t = 30)]
        max_lanes: u8,
        /// Maximum pass count per worker
        #[arg(long, default_value_t = 3)]
        max_passes: u32,
        /// Reuse released lanes after max-lanes is reached
        #[arg(long)]
        cycle_lanes: bool,
        /// Project root used for worktree metadata
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Require a clean isolated worktree before mutating lane state
        #[arg(long)]
        strict_isolation: bool,
        /// Render deterministic JSON
        #[arg(long)]
        json: bool,
    },
    /// Claim a lane in a state directory
    ClaimState {
        /// State directory for lane/pass files
        #[arg(long, default_value = ".driven/lanes")]
        state_dir: PathBuf,
        /// Stable worker identity
        #[arg(long)]
        worker_id: String,
        /// Lane scope or responsibility
        #[arg(long)]
        scope: String,
        /// Maximum lane count
        #[arg(long, default_value_t = 30)]
        max_lanes: u8,
        /// Maximum pass count per worker
        #[arg(long, default_value_t = 3)]
        max_passes: u32,
        /// Reuse released lanes after max-lanes is reached
        #[arg(long)]
        cycle_lanes: bool,
        /// Project root used for worktree metadata
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Require a clean isolated worktree before mutating lane state
        #[arg(long)]
        strict_isolation: bool,
        /// Render deterministic JSON
        #[arg(long)]
        json: bool,
    },
    /// Advance a worker to the next pass while keeping the same lane
    Next {
        /// State directory for lane/pass files
        #[arg(long, default_value = ".driven/lanes")]
        state_dir: PathBuf,
        /// Stable worker identity
        #[arg(long)]
        worker_id: String,
        /// Lane scope or responsibility
        #[arg(long)]
        scope: String,
        /// Maximum lane count
        #[arg(long, default_value_t = 30)]
        max_lanes: u8,
        /// Maximum pass count per worker
        #[arg(long, default_value_t = 3)]
        max_passes: u32,
        /// Reuse released lanes after max-lanes is reached
        #[arg(long)]
        cycle_lanes: bool,
        /// Project root used for worktree metadata
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Require a clean isolated worktree before mutating lane state
        #[arg(long)]
        strict_isolation: bool,
        /// Require a durable receipt-backed handoff for this next-pass advance
        #[arg(long)]
        durable_next: bool,
        /// Allow the legacy non-receipt next-pass advance
        #[arg(long)]
        unsafe_legacy_next: bool,
        /// Canonical proof receipt JSON used to create and persist a next-pass handoff
        #[arg(long)]
        receipt: Option<PathBuf>,
        /// Next action to persist in the handoff when --receipt is supplied
        #[arg(long)]
        next_action: Option<String>,
        /// Render deterministic JSON
        #[arg(long)]
        json: bool,
    },
    /// Release a worker lane so another worker can claim it when cycling is enabled
    Release {
        /// State directory for lane/pass files
        #[arg(long, default_value = ".driven/lanes")]
        state_dir: PathBuf,
        /// Stable worker identity
        #[arg(long)]
        worker_id: String,
        /// Lane scope or responsibility
        #[arg(long)]
        scope: String,
        /// Maximum lane count
        #[arg(long, default_value_t = 30)]
        max_lanes: u8,
        /// Maximum pass count per worker
        #[arg(long, default_value_t = 3)]
        max_passes: u32,
        /// Reuse released lanes after max-lanes is reached
        #[arg(long)]
        cycle_lanes: bool,
        /// Project root used for worktree metadata
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Require a clean isolated worktree before mutating lane state
        #[arg(long)]
        strict_isolation: bool,
        /// Render deterministic JSON
        #[arg(long)]
        json: bool,
    },
    /// Run a command and write a canonical proof receipt for the active lane/pass
    Receipt {
        /// State directory for lane/pass files
        #[arg(long, default_value = ".driven/lanes")]
        state_dir: PathBuf,
        /// Stable worker identity
        #[arg(long)]
        worker_id: String,
        /// Lane scope or responsibility
        #[arg(long)]
        scope: String,
        /// Maximum lane count
        #[arg(long, default_value_t = 30)]
        max_lanes: u8,
        /// Maximum pass count per worker
        #[arg(long, default_value_t = 3)]
        max_passes: u32,
        /// Reuse released lanes after max-lanes is reached
        #[arg(long)]
        cycle_lanes: bool,
        /// Project root used for worktree metadata and command cwd
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Require a clean isolated worktree before mutating lane state
        #[arg(long)]
        strict_isolation: bool,
        /// Receipt summary
        #[arg(long)]
        summary: String,
        /// Verification class for the command
        #[arg(long, default_value = "small")]
        class: StrategyVerificationClass,
        /// Output path for canonical receipt JSON
        #[arg(long)]
        out: PathBuf,
        /// Timeout in milliseconds for the receipt command
        #[arg(long, default_value_t = 300_000)]
        timeout_ms: u64,
        /// Maximum stdout/stderr bytes retained in receipt evidence metadata
        #[arg(long, default_value_t = 1_048_576)]
        max_output_bytes: u64,
        /// Render deterministic JSON to stdout
        #[arg(long)]
        json: bool,
        /// Command to run. Pass after `--`, for example: -- cargo fmt --check
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Mark a worker lane/pass lifecycle as complete
    Complete {
        /// State directory for lane/pass files
        #[arg(long, default_value = ".driven/lanes")]
        state_dir: PathBuf,
        /// Stable worker identity
        #[arg(long)]
        worker_id: String,
        /// Lane scope or responsibility
        #[arg(long)]
        scope: String,
        /// Maximum lane count
        #[arg(long, default_value_t = 30)]
        max_lanes: u8,
        /// Maximum pass count per worker
        #[arg(long, default_value_t = 3)]
        max_passes: u32,
        /// Reuse released lanes after max-lanes is reached
        #[arg(long)]
        cycle_lanes: bool,
        /// Project root used for worktree metadata
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Require a clean isolated worktree before mutating lane state
        #[arg(long)]
        strict_isolation: bool,
        /// Canonical proof receipt JSON required to complete the lane/pass
        #[arg(long)]
        receipt: PathBuf,
        /// Render deterministic JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum StrategyVerificationClass {
    Small,
    Targeted,
    Heavy,
}

impl From<StrategyVerificationClass> for driven::VerificationClass {
    fn from(value: StrategyVerificationClass) -> Self {
        match value {
            StrategyVerificationClass::Small => Self::Small,
            StrategyVerificationClass::Targeted => Self::Targeted,
            StrategyVerificationClass::Heavy => Self::Heavy,
        }
    }
}

fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();
    let project_root = std::env::current_dir()?;

    match cli.command {
        Commands::Init { interactive } => {
            driven::cli::InitCommand::run(&project_root, interactive)?;
        }
        Commands::Sync { watch } => {
            if watch {
                driven::cli::SyncCommand::watch(&project_root)?;
            } else {
                driven::cli::SyncCommand::run(&project_root)?;
            }
        }
        Commands::Convert {
            input,
            output,
            editor,
        } => {
            let editor = editor.and_then(|e| match e.to_lowercase().as_str() {
                "cursor" => Some(driven::Editor::Cursor),
                "copilot" => Some(driven::Editor::Copilot),
                "windsurf" => Some(driven::Editor::Windsurf),
                "claude" => Some(driven::Editor::Claude),
                "aider" => Some(driven::Editor::Aider),
                "cline" => Some(driven::Editor::Cline),
                _ => None,
            });
            driven::cli::ConvertCommand::run(&input, &output, editor)?;
        }
        Commands::Template { action } => match action {
            TemplateAction::List => {
                driven::cli::TemplateCommand::list()?;
            }
            TemplateAction::Search { query } => {
                driven::cli::TemplateCommand::search(&query)?;
            }
            TemplateAction::Apply { name } => {
                driven::cli::TemplateCommand::apply(&project_root, &name)?;
            }
        },
        Commands::Analyze { context, index } => {
            if context {
                let output = project_root.join(".driven/context.md");
                driven::cli::AnalyzeCommand::generate_context(&project_root, &output)?;
            } else if index {
                driven::cli::AnalyzeCommand::index(&project_root)?;
            } else {
                driven::cli::AnalyzeCommand::run(&project_root)?;
            }
        }
        Commands::Validate { file, strict } => {
            if strict {
                driven::cli::ValidateCommand::run_strict(&file)?;
            } else {
                driven::cli::ValidateCommand::run(&file)?;
            }
        }
        Commands::Hook { action } => match action {
            HookAction::List => {
                let hooks = driven::cli::HookCommand::list(&project_root)?;
                driven::cli::print_hooks_table(&hooks);
            }
            HookAction::Add {
                id,
                trigger,
                trigger_value,
                agent,
                message,
                name,
                workflow,
                condition,
                disabled,
            } => {
                driven::cli::HookCommand::add(
                    &project_root,
                    &id,
                    name.as_deref(),
                    &trigger,
                    &trigger_value,
                    &agent,
                    &message,
                    workflow.as_deref(),
                    condition.as_deref(),
                    !disabled,
                )?;
            }
            HookAction::Remove { id } => {
                driven::cli::HookCommand::remove(&project_root, &id)?;
            }
            HookAction::Trigger { command } => {
                driven::cli::HookCommand::trigger(&project_root, &command)?;
            }
            HookAction::Enable { id } => {
                driven::cli::HookCommand::enable(&project_root, &id)?;
            }
            HookAction::Disable { id } => {
                driven::cli::HookCommand::disable(&project_root, &id)?;
            }
            HookAction::Show { id } => {
                let hook = driven::cli::HookCommand::show(&project_root, &id)?;
                driven::cli::print_hook_details(&hook);
            }
        },
        Commands::Steer { action } => match action {
            SteerAction::List => {
                let rules = driven::cli::SteerCommand::list(&project_root)?;
                driven::cli::print_steering_table(&rules);
            }
            SteerAction::Add {
                id,
                inclusion,
                pattern,
                content,
                name,
                priority,
            } => {
                driven::cli::SteerCommand::add(
                    &project_root,
                    &id,
                    name.as_deref(),
                    &inclusion,
                    pattern.as_deref(),
                    &content,
                    priority,
                )?;
            }
            SteerAction::Remove { id } => {
                driven::cli::SteerCommand::remove(&project_root, &id)?;
            }
            SteerAction::Test { file, keys } => {
                let rules = driven::cli::SteerCommand::test(&project_root, &file, &keys)?;
                if rules.is_empty() {
                    println!("No steering rules apply to this file.");
                } else {
                    println!("Applicable steering rules:");
                    driven::cli::print_steering_table(&rules);
                }
            }
            SteerAction::Show { id } => {
                let rule = driven::cli::SteerCommand::show(&project_root, &id)?;
                driven::cli::print_steering_details(&rule);
            }
            SteerAction::Inject { file, keys } => {
                let content =
                    driven::cli::SteerCommand::inject(&project_root, file.as_deref(), &keys)?;
                println!("{}", content);
            }
        },
        Commands::Strategy { action } => match action {
            StrategyAction::InspectWorktree { path, json } => {
                let path = path.unwrap_or_else(|| project_root.clone());
                let output = driven::cli::StrategyCommand::inspect_worktree(&path, json)?;
                print!("{}", output);
            }
            StrategyAction::Claim {
                lane,
                pass,
                worker_id,
                scope,
                next_action,
                json,
            } => {
                let output = driven::cli::StrategyCommand::claim(
                    &project_root,
                    lane,
                    pass,
                    &worker_id,
                    &scope,
                    &next_action,
                    json,
                )?;
                print!("{}", output);
            }
            StrategyAction::Peek {
                state_dir,
                scope,
                max_lanes,
                max_passes,
                cycle_lanes,
                project_root,
                strict_isolation,
                json,
            } => {
                let output = driven::cli::StrategyCommand::peek_state_with_options(
                    state_dir,
                    &scope,
                    max_lanes,
                    max_passes,
                    json,
                    strategy_state_options(cycle_lanes, project_root, strict_isolation),
                )?;
                print!("{}", output);
            }
            StrategyAction::ClaimState {
                state_dir,
                worker_id,
                scope,
                max_lanes,
                max_passes,
                cycle_lanes,
                project_root,
                strict_isolation,
                json,
            } => {
                let output = driven::cli::StrategyCommand::claim_state_with_options(
                    state_dir,
                    &scope,
                    max_lanes,
                    max_passes,
                    &worker_id,
                    json,
                    strategy_state_options(cycle_lanes, project_root, strict_isolation),
                )?;
                print!("{}", output);
            }
            StrategyAction::Next {
                state_dir,
                worker_id,
                scope,
                max_lanes,
                max_passes,
                cycle_lanes,
                project_root,
                strict_isolation,
                durable_next,
                unsafe_legacy_next,
                receipt,
                next_action,
                json,
            } => {
                if durable_next && unsafe_legacy_next {
                    return Err(anyhow::anyhow!(
                        "strategy next cannot combine --durable-next and --unsafe-legacy-next"
                    ));
                }
                let options = strategy_state_options_with_handoff(
                    cycle_lanes,
                    project_root,
                    strict_isolation,
                    !unsafe_legacy_next,
                );
                let output = match (receipt, next_action) {
                    (Some(receipt), Some(next_action)) => {
                        driven::cli::StrategyCommand::next_state_with_handoff_options(
                            state_dir,
                            &scope,
                            max_lanes,
                            max_passes,
                            &worker_id,
                            &receipt,
                            &next_action,
                            json,
                            options,
                        )?
                    }
                    (None, None) if unsafe_legacy_next => {
                        driven::cli::StrategyCommand::next_state_with_options(
                            state_dir, &scope, max_lanes, max_passes, &worker_id, json, options,
                        )?
                    }
                    (None, None) => {
                        return Err(anyhow::anyhow!(
                            "durable handoff required: strategy next requires --receipt and --next-action; pass --unsafe-legacy-next to use legacy non-receipt advancement"
                        ));
                    }
                    _ => {
                        return Err(anyhow::anyhow!(
                            "strategy next requires both --receipt and --next-action for a durable handoff"
                        ));
                    }
                };
                print!("{}", output);
            }
            StrategyAction::Release {
                state_dir,
                worker_id,
                scope,
                max_lanes,
                max_passes,
                cycle_lanes,
                project_root,
                strict_isolation,
                json,
            } => {
                let output = driven::cli::StrategyCommand::release_state_with_options(
                    state_dir,
                    &scope,
                    max_lanes,
                    max_passes,
                    &worker_id,
                    json,
                    strategy_state_options(cycle_lanes, project_root, strict_isolation),
                )?;
                print!("{}", output);
            }
            StrategyAction::Receipt {
                state_dir,
                worker_id,
                scope,
                max_lanes,
                max_passes,
                cycle_lanes,
                project_root,
                strict_isolation,
                summary,
                class,
                out,
                timeout_ms,
                max_output_bytes,
                json,
                command,
            } => {
                let (program, args) = command
                    .split_first()
                    .ok_or_else(|| anyhow::anyhow!("receipt command cannot be empty"))?;
                let output = driven::cli::StrategyCommand::receipt_state_with_execution_options(
                    state_dir,
                    &scope,
                    max_lanes,
                    max_passes,
                    &worker_id,
                    &summary,
                    class.into(),
                    program,
                    args,
                    &out,
                    json,
                    driven::cli::StrategyReceiptExecutionOptions::default()
                        .with_timeout_ms(timeout_ms)
                        .with_max_output_bytes(max_output_bytes),
                    strategy_state_options(cycle_lanes, project_root, strict_isolation),
                )?;
                print!("{}", output);
            }
            StrategyAction::Complete {
                state_dir,
                worker_id,
                scope,
                max_lanes,
                max_passes,
                cycle_lanes,
                project_root,
                strict_isolation,
                receipt,
                json,
            } => {
                let output = driven::cli::StrategyCommand::complete_state_with_receipt_options(
                    state_dir,
                    &scope,
                    max_lanes,
                    max_passes,
                    &worker_id,
                    &receipt,
                    json,
                    strategy_state_options(cycle_lanes, project_root, strict_isolation),
                )?;
                print!("{}", output);
            }
        },
    }

    Ok(())
}

fn strategy_state_options(
    cycle_lanes: bool,
    project_root: Option<PathBuf>,
    strict_isolation: bool,
) -> driven::cli::StrategyStateOptions {
    strategy_state_options_with_handoff(cycle_lanes, project_root, strict_isolation, false)
}

fn strategy_state_options_with_handoff(
    cycle_lanes: bool,
    project_root: Option<PathBuf>,
    strict_isolation: bool,
    handoff_required_for_next: bool,
) -> driven::cli::StrategyStateOptions {
    let options = driven::cli::StrategyStateOptions::default()
        .with_lane_cycling(cycle_lanes)
        .with_strict_isolation(strict_isolation)
        .with_handoff_required_for_next(handoff_required_for_next);
    match project_root {
        Some(project_root) => options.with_project_root(project_root),
        None => options,
    }
}
