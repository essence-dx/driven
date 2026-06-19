use crate::{DrivenError, Result};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandExecutionPolicy {
    pub timeout_ms: u64,
    pub max_output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CapturedCommand {
    pub status: CapturedCommandStatus,
    pub stdout: CapturedStream,
    pub stderr: CapturedStream,
    pub started_unix_seconds: u64,
    pub finished_unix_seconds: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CapturedCommandStatus {
    Exited { exit_code: i32 },
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CapturedStream {
    pub digest: String,
    pub bytes: u64,
    pub truncated: bool,
}

pub(super) fn run_command_with_policy(
    program: &str,
    args: &[String],
    cwd: &Path,
    policy: &CommandExecutionPolicy,
    started_unix_seconds: u64,
) -> Result<CapturedCommand> {
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(DrivenError::Io)?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DrivenError::Validation("failed to capture command stdout".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| DrivenError::Validation("failed to capture command stderr".to_string()))?;
    let stdout_reader = read_stream(stdout, policy.max_output_bytes);
    let stderr_reader = read_stream(stderr, policy.max_output_bytes);

    let timeout = Duration::from_millis(policy.timeout_ms);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(DrivenError::Io)? {
            break CapturedCommandStatus::Exited {
                exit_code: status.code().unwrap_or(-1),
            };
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            break CapturedCommandStatus::TimedOut;
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_stream_reader(stdout_reader)?;
    let stderr = join_stream_reader(stderr_reader)?;
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let finished_unix_seconds = started_unix_seconds.saturating_add(duration_ms.div_ceil(1_000));

    Ok(CapturedCommand {
        status,
        stdout,
        stderr,
        started_unix_seconds,
        finished_unix_seconds,
        duration_ms,
    })
}

fn read_stream<R>(
    mut reader: R,
    max_output_bytes: u64,
) -> thread::JoinHandle<Result<CapturedStream>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut hasher = blake3::Hasher::new();
        let mut total_bytes = 0_u64;
        let mut truncated = false;
        let mut buffer = [0_u8; 8 * 1024];

        loop {
            let read = reader.read(&mut buffer).map_err(DrivenError::Io)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            total_bytes = total_bytes.saturating_add(read as u64);
            if total_bytes > max_output_bytes {
                truncated = true;
            }
        }

        Ok(CapturedStream {
            digest: hasher.finalize().to_hex().to_string(),
            bytes: total_bytes,
            truncated,
        })
    })
}

fn join_stream_reader(
    handle: thread::JoinHandle<Result<CapturedStream>>,
) -> Result<CapturedStream> {
    handle
        .join()
        .map_err(|_| DrivenError::Validation("command output reader thread panicked".to_string()))?
}
