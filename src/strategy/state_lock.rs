use crate::{DrivenError, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const LOCK_FILE: &str = ".driven-lane-pass.lock";
const LOCK_SCHEMA: &str = "driven.lane_pass.lock.v1";
const DEFAULT_STALE_AFTER_SECONDS: u64 = 30 * 60;
static LOCK_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub(super) struct StateLock {
    path: PathBuf,
    lock_id: String,
}

impl StateLock {
    pub(super) fn acquire(state_dir: &Path) -> Result<Self> {
        let path = state_dir.join(LOCK_FILE);
        match Self::create(&path) {
            Ok(lock) => Ok(lock),
            Err(AcquireLockError::Io(error))
                if error.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                Self::recover_or_block_at(path, unix_seconds()?)
            }
            Err(AcquireLockError::Io(error)) => Err(DrivenError::Io(error)),
            Err(AcquireLockError::Driven(error)) => Err(error),
        }
    }

    fn recover_or_block_at(path: PathBuf, now: u64) -> Result<Self> {
        match LockMetadata::read(&path) {
            Ok(metadata) => {
                if metadata.is_stale(now) {
                    Self::remove_lock_if_metadata_matches(&path, &metadata)?;
                    return Self::create(&path).map_err(AcquireLockError::into_driven);
                }

                Err(DrivenError::Validation(format!(
                    "lane/pass state is locked at {} by pid {} since {} (stale after {} seconds)",
                    path.display(),
                    metadata.owner_pid,
                    metadata.acquired_unix_seconds,
                    metadata.stale_after_seconds
                )))
            }
            Err(error) => {
                let modified = modified_unix_seconds(&path)?;
                if modified
                    .checked_add(DEFAULT_STALE_AFTER_SECONDS)
                    .is_some_and(|expires_at| expires_at <= now)
                {
                    Self::remove_unreadable_lock_if_modified_matches(&path, modified)?;
                    return Self::create(&path).map_err(AcquireLockError::into_driven);
                }

                Err(DrivenError::Validation(format!(
                    "{}; lock file modified at {} and will be treated as stale after {} seconds",
                    error, modified, DEFAULT_STALE_AFTER_SECONDS
                )))
            }
        }
    }

    fn remove_lock_if_metadata_matches(path: &Path, observed: &LockMetadata) -> Result<()> {
        let current = LockMetadata::read(path)?;
        if current.lock_id != observed.lock_id
            || current.owner_pid != observed.owner_pid
            || current.acquired_unix_seconds != observed.acquired_unix_seconds
        {
            return Err(DrivenError::Validation(format!(
                "lane/pass lock at {} was replaced during stale recovery",
                path.display()
            )));
        }
        fs::remove_file(path).map_err(DrivenError::Io)
    }

    fn remove_unreadable_lock_if_modified_matches(
        path: &Path,
        observed_modified: u64,
    ) -> Result<()> {
        let current_modified = modified_unix_seconds(path)?;
        if current_modified != observed_modified {
            return Err(DrivenError::Validation(format!(
                "lane/pass lock at {} changed during stale recovery",
                path.display()
            )));
        }
        fs::remove_file(path).map_err(DrivenError::Io)
    }

    fn create(path: &Path) -> std::result::Result<Self, AcquireLockError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(AcquireLockError::Io)?;
        let metadata = LockMetadata::new(unix_seconds().map_err(AcquireLockError::Driven)?);
        let lock_id = metadata.lock_id.clone();
        let content = serde_json::to_string_pretty(&metadata)
            .map(|mut value| {
                value.push('\n');
                value
            })
            .map_err(|e| {
                AcquireLockError::Driven(DrivenError::Format(format!(
                    "failed to render lane/pass lock metadata: {}",
                    e
                )))
            })?;
        if let Err(error) = file.write_all(content.as_bytes()) {
            let _ = fs::remove_file(path);
            return Err(AcquireLockError::Io(error));
        }
        Ok(Self {
            path: path.to_path_buf(),
            lock_id,
        })
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        if LockMetadata::read(&self.path)
            .ok()
            .is_some_and(|metadata| metadata.lock_id == self.lock_id)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug)]
enum AcquireLockError {
    Io(std::io::Error),
    Driven(DrivenError),
}

impl AcquireLockError {
    fn into_driven(self) -> DrivenError {
        match self {
            Self::Io(error) => DrivenError::Io(error),
            Self::Driven(error) => error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockMetadata {
    schema: String,
    #[serde(default)]
    lock_id: String,
    owner_pid: u32,
    acquired_unix_seconds: u64,
    stale_after_seconds: u64,
}

impl LockMetadata {
    fn new(acquired_unix_seconds: u64) -> Self {
        Self {
            schema: LOCK_SCHEMA.to_string(),
            lock_id: format!(
                "{}-{}-{}",
                std::process::id(),
                acquired_unix_seconds,
                LOCK_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            owner_pid: std::process::id(),
            acquired_unix_seconds,
            stale_after_seconds: DEFAULT_STALE_AFTER_SECONDS,
        }
    }

    fn read(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).map_err(DrivenError::Io)?;
        let metadata: Self = serde_json::from_str(&content).map_err(|e| {
            DrivenError::Validation(format!(
                "lane/pass state is locked at {}; lock metadata is unreadable: {}",
                path.display(),
                e
            ))
        })?;
        if metadata.schema != LOCK_SCHEMA {
            return Err(DrivenError::Validation(format!(
                "lane/pass state is locked at {}; unsupported lock schema {}",
                path.display(),
                metadata.schema
            )));
        }
        Ok(metadata)
    }

    fn is_stale(&self, now: u64) -> bool {
        self.acquired_unix_seconds
            .checked_add(self.stale_after_seconds)
            .is_some_and(|expires_at| expires_at <= now)
    }
}

fn unix_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|e| DrivenError::Validation(format!("system clock is before Unix epoch: {}", e)))
}

fn modified_unix_seconds(path: &Path) -> Result<u64> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(DrivenError::Io)?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|e| DrivenError::Validation(format!("lock mtime is before Unix epoch: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_lock_preserves_replaced_lock_file() {
        let temp = tempfile::tempdir().unwrap();
        let lock = StateLock::acquire(temp.path()).unwrap();
        let replacement = LockMetadata {
            schema: LOCK_SCHEMA.to_string(),
            lock_id: "replacement-owner".to_string(),
            owner_pid: 4242,
            acquired_unix_seconds: unix_seconds().unwrap(),
            stale_after_seconds: DEFAULT_STALE_AFTER_SECONDS,
        };
        fs::write(
            temp.path().join(LOCK_FILE),
            serde_json::to_string_pretty(&replacement).unwrap(),
        )
        .unwrap();

        drop(lock);

        let current = LockMetadata::read(&temp.path().join(LOCK_FILE)).unwrap();
        assert_eq!(current.lock_id, "replacement-owner");
    }

    #[test]
    fn stale_unreadable_lock_metadata_can_be_recovered() {
        let temp = tempfile::tempdir().unwrap();
        let lock_path = temp.path().join(LOCK_FILE);
        fs::write(&lock_path, "{").unwrap();

        let lock =
            StateLock::recover_or_block_at(lock_path.clone(), unix_seconds().unwrap() + 3600)
                .unwrap();

        assert_eq!(lock.path, lock_path);
    }

    #[test]
    fn stale_recovery_refuses_to_remove_replaced_lock_file() {
        let temp = tempfile::tempdir().unwrap();
        let lock_path = temp.path().join(LOCK_FILE);
        let stale = LockMetadata {
            schema: LOCK_SCHEMA.to_string(),
            lock_id: "stale-owner".to_string(),
            owner_pid: 1,
            acquired_unix_seconds: 1,
            stale_after_seconds: 1,
        };
        fs::write(&lock_path, serde_json::to_string_pretty(&stale).unwrap()).unwrap();
        let observed = LockMetadata::read(&lock_path).unwrap();
        let replacement = LockMetadata {
            schema: LOCK_SCHEMA.to_string(),
            lock_id: "replacement-owner".to_string(),
            owner_pid: 4242,
            acquired_unix_seconds: unix_seconds().unwrap(),
            stale_after_seconds: DEFAULT_STALE_AFTER_SECONDS,
        };
        fs::write(
            &lock_path,
            serde_json::to_string_pretty(&replacement).unwrap(),
        )
        .unwrap();

        let err = StateLock::remove_lock_if_metadata_matches(&lock_path, &observed).unwrap_err();

        assert!(err.to_string().contains("replaced"));
        assert_eq!(
            LockMetadata::read(&lock_path).unwrap().lock_id,
            "replacement-owner"
        );
    }
}
