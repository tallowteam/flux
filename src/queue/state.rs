use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::FluxError;

/// Status of a queued transfer job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for QueueStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueueStatus::Pending => write!(f, "pending"),
            QueueStatus::Running => write!(f, "running"),
            QueueStatus::Paused => write!(f, "paused"),
            QueueStatus::Completed => write!(f, "completed"),
            QueueStatus::Failed => write!(f, "failed"),
            QueueStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A single transfer job in the queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: u64,
    pub status: QueueStatus,
    pub source: String,
    pub dest: String,
    pub recursive: bool,
    pub verify: bool,
    pub compress: bool,
    pub added_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub bytes_transferred: u64,
    pub error: Option<String>,
}

/// Persistent queue store backed by a JSON file.
///
/// Stores transfer jobs in `queue.json` within the Flux data directory.
/// Uses atomic writes (write to temp file, then rename) for crash safety.
pub struct QueueStore {
    path: PathBuf,
    entries: Vec<QueueEntry>,
    next_id: u64,
}

impl QueueStore {
    /// Load the queue from `data_dir/queue.json`.
    ///
    /// If the file does not exist, returns an empty queue starting at id 1.
    /// If the file is corrupted, logs a warning and starts fresh.
    pub fn load(data_dir: &Path) -> Result<Self, FluxError> {
        let path = data_dir.join("queue.json");

        if path.exists() {
            let contents = std::fs::read_to_string(&path).map_err(|e| FluxError::Io {
                source: e,
            })?;
            match serde_json::from_str::<Vec<QueueEntry>>(&contents) {
                Ok(entries) => {
                    let next_id = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
                    Ok(Self {
                        path,
                        entries,
                        next_id,
                    })
                }
                Err(e) => {
                    tracing::warn!("Corrupted queue.json, starting fresh: {}", e);
                    Ok(Self {
                        path,
                        entries: Vec::new(),
                        next_id: 1,
                    })
                }
            }
        } else {
            Ok(Self {
                path,
                entries: Vec::new(),
                next_id: 1,
            })
        }
    }

    /// Atomically save the queue to `queue.json`.
    ///
    /// Writes to a temporary file first, then renames for crash safety.
    pub fn save(&self) -> Result<(), FluxError> {
        let tmp_path = self.path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&tmp_path, json).map_err(|e| FluxError::Io { source: e })?;
        std::fs::rename(&tmp_path, &self.path).map_err(|e| FluxError::Io { source: e })?;
        Ok(())
    }

    /// Add a new transfer job to the queue.
    ///
    /// Returns the assigned job ID.
    pub fn add(
        &mut self,
        source: String,
        dest: String,
        recursive: bool,
        verify: bool,
        compress: bool,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.entries.push(QueueEntry {
            id,
            status: QueueStatus::Pending,
            source,
            dest,
            recursive,
            verify,
            compress,
            added_at: Utc::now(),
            started_at: None,
            completed_at: None,
            bytes_transferred: 0,
            error: None,
        });

        id
    }

    /// Return a slice of all queue entries.
    pub fn list(&self) -> &[QueueEntry] {
        &self.entries
    }

    /// Find an entry by ID.
    pub fn get(&self, id: u64) -> Option<&QueueEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Find a mutable entry by ID.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut QueueEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Pause a queued transfer.
    ///
    /// Only Pending or Running jobs can be paused. Returns an error for
    /// Completed, Failed, or Cancelled jobs.
    pub fn pause(&mut self, id: u64) -> Result<(), FluxError> {
        let entry = self
            .get_mut(id)
            .ok_or_else(|| FluxError::QueueError(format!("Job #{} not found", id)))?;

        match entry.status {
            QueueStatus::Pending | QueueStatus::Running => {
                entry.status = QueueStatus::Paused;
                Ok(())
            }
            QueueStatus::Paused => Ok(()), // Already paused, idempotent
            _ => Err(FluxError::QueueError(format!(
                "Cannot pause job #{} with status '{}'",
                id, entry.status
            ))),
        }
    }

    /// Resume a paused transfer, setting its status back to Pending.
    pub fn resume(&mut self, id: u64) -> Result<(), FluxError> {
        let entry = self
            .get_mut(id)
            .ok_or_else(|| FluxError::QueueError(format!("Job #{} not found", id)))?;

        match entry.status {
            QueueStatus::Paused => {
                entry.status = QueueStatus::Pending;
                Ok(())
            }
            QueueStatus::Pending => Ok(()), // Already pending, idempotent
            _ => Err(FluxError::QueueError(format!(
                "Cannot resume job #{} with status '{}'",
                id, entry.status
            ))),
        }
    }

    /// Cancel a queued transfer.
    ///
    /// Sets the status to Cancelled and records the completion time.
    /// Returns an error if already completed.
    pub fn cancel(&mut self, id: u64) -> Result<(), FluxError> {
        let entry = self
            .get_mut(id)
            .ok_or_else(|| FluxError::QueueError(format!("Job #{} not found", id)))?;

        match entry.status {
            QueueStatus::Completed => Err(FluxError::QueueError(format!(
                "Cannot cancel job #{}: already completed",
                id
            ))),
            QueueStatus::Cancelled => Ok(()), // Already cancelled, idempotent
            _ => {
                entry.status = QueueStatus::Cancelled;
                entry.completed_at = Some(Utc::now());
                Ok(())
            }
        }
    }

    /// Return entries with Pending status, ordered by ID.
    pub fn pending_entries(&self) -> Vec<&QueueEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == QueueStatus::Pending)
            .collect()
    }

    /// Remove all Completed, Failed, and Cancelled entries from the queue.
    pub fn clear_completed(&mut self) {
        self.entries.retain(|e| {
            !matches!(
                e.status,
                QueueStatus::Completed | QueueStatus::Failed | QueueStatus::Cancelled
            )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, QueueStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = QueueStore::load(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn add_entry_increments_id() {
        let (_dir, mut store) = temp_store();
        let id1 = store.add("a.txt".into(), "b.txt".into(), false, false, false);
        let id2 = store.add("c.txt".into(), "d.txt".into(), false, false, false);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn add_entry_sets_pending_status() {
        let (_dir, mut store) = temp_store();
        let id = store.add("src".into(), "dst".into(), true, true, false);
        let entry = store.get(id).unwrap();
        assert_eq!(entry.status, QueueStatus::Pending);
        assert_eq!(entry.source, "src");
        assert_eq!(entry.dest, "dst");
        assert!(entry.recursive);
        assert!(entry.verify);
        assert!(!entry.compress);
        assert!(entry.started_at.is_none());
        assert!(entry.completed_at.is_none());
    }

    #[test]
    fn pause_pending_sets_paused() {
        let (_dir, mut store) = temp_store();
        let id = store.add("a".into(), "b".into(), false, false, false);
        store.pause(id).unwrap();
        assert_eq!(store.get(id).unwrap().status, QueueStatus::Paused);
    }

    #[test]
    fn resume_paused_sets_pending() {
        let (_dir, mut store) = temp_store();
        let id = store.add("a".into(), "b".into(), false, false, false);
        store.pause(id).unwrap();
        store.resume(id).unwrap();
        assert_eq!(store.get(id).unwrap().status, QueueStatus::Pending);
    }

    #[test]
    fn cancel_sets_cancelled_and_completed_at() {
        let (_dir, mut store) = temp_store();
        let id = store.add("a".into(), "b".into(), false, false, false);
        store.cancel(id).unwrap();
        let entry = store.get(id).unwrap();
        assert_eq!(entry.status, QueueStatus::Cancelled);
        assert!(entry.completed_at.is_some());
    }

    #[test]
    fn pause_completed_returns_error() {
        let (_dir, mut store) = temp_store();
        let id = store.add("a".into(), "b".into(), false, false, false);
        // Manually set to Completed
        store.get_mut(id).unwrap().status = QueueStatus::Completed;
        let err = store.pause(id).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Cannot pause"));
    }

    #[test]
    fn resume_running_returns_error() {
        let (_dir, mut store) = temp_store();
        let id = store.add("a".into(), "b".into(), false, false, false);
        store.get_mut(id).unwrap().status = QueueStatus::Running;
        let err = store.resume(id).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Cannot resume"));
    }

    #[test]
    fn cancel_completed_returns_error() {
        let (_dir, mut store) = temp_store();
        let id = store.add("a".into(), "b".into(), false, false, false);
        store.get_mut(id).unwrap().status = QueueStatus::Completed;
        let err = store.cancel(id).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Cannot cancel"));
    }

    #[test]
    fn pause_nonexistent_returns_error() {
        let (_dir, mut store) = temp_store();
        let err = store.pause(999).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not found"));
    }

    #[test]
    fn pending_entries_returns_only_pending() {
        let (_dir, mut store) = temp_store();
        store.add("a".into(), "b".into(), false, false, false);
        store.add("c".into(), "d".into(), false, false, false);
        store.add("e".into(), "f".into(), false, false, false);
        store.pause(2).unwrap();
        let pending = store.pending_entries();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].id, 1);
        assert_eq!(pending[1].id, 3);
    }

    #[test]
    fn clear_completed_removes_finished_entries() {
        let (_dir, mut store) = temp_store();
        store.add("a".into(), "b".into(), false, false, false); // 1: Pending
        store.add("c".into(), "d".into(), false, false, false); // 2: will be Completed
        store.add("e".into(), "f".into(), false, false, false); // 3: will be Failed
        store.add("g".into(), "h".into(), false, false, false); // 4: will be Cancelled

        store.get_mut(2).unwrap().status = QueueStatus::Completed;
        store.get_mut(3).unwrap().status = QueueStatus::Failed;
        store.cancel(4).unwrap();

        store.clear_completed();
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].id, 1);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();

        {
            let mut store = QueueStore::load(dir.path()).unwrap();
            store.add("src1".into(), "dst1".into(), true, false, true);
            store.add("src2".into(), "dst2".into(), false, true, false);
            store.pause(2).unwrap();
            store.save().unwrap();
        }

        {
            let store = QueueStore::load(dir.path()).unwrap();
            assert_eq!(store.list().len(), 2);
            assert_eq!(store.get(1).unwrap().source, "src1");
            assert_eq!(store.get(1).unwrap().status, QueueStatus::Pending);
            assert_eq!(store.get(2).unwrap().status, QueueStatus::Paused);
        }
    }

    #[test]
    fn load_continues_ids_after_reload() {
        let dir = tempfile::tempdir().unwrap();

        {
            let mut store = QueueStore::load(dir.path()).unwrap();
            store.add("a".into(), "b".into(), false, false, false); // id 1
            store.add("c".into(), "d".into(), false, false, false); // id 2
            store.save().unwrap();
        }

        {
            let mut store = QueueStore::load(dir.path()).unwrap();
            let id = store.add("e".into(), "f".into(), false, false, false);
            assert_eq!(id, 3); // continues from max(2) + 1
        }
    }

    #[test]
    fn load_empty_dir_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let store = QueueStore::load(dir.path()).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn corrupted_json_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("queue.json"), "not valid json!!!").unwrap();
        let store = QueueStore::load(dir.path()).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn queue_status_display() {
        assert_eq!(format!("{}", QueueStatus::Pending), "pending");
        assert_eq!(format!("{}", QueueStatus::Running), "running");
        assert_eq!(format!("{}", QueueStatus::Paused), "paused");
        assert_eq!(format!("{}", QueueStatus::Completed), "completed");
        assert_eq!(format!("{}", QueueStatus::Failed), "failed");
        assert_eq!(format!("{}", QueueStatus::Cancelled), "cancelled");
    }
}
