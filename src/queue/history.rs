use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::error::FluxError;

/// A single transfer history entry recording what was transferred and its outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub source: String,
    pub dest: String,
    pub bytes: u64,
    pub files: u64,
    pub duration_secs: f64,
    pub timestamp: DateTime<Utc>,
    pub status: String, // "completed", "failed", "cancelled"
    pub error: Option<String>,
}

/// Persistent history store backed by a JSON file.
///
/// Stores transfer history in `history.json` within the Flux data directory.
/// Entries are capped at a configurable limit; oldest entries are removed
/// when the limit is exceeded.
///
/// An exclusive advisory lock on `history.lock` is held for the entire
/// lifetime of this struct and released automatically on drop, preventing
/// concurrent writers from corrupting the history file.
pub struct HistoryStore {
    path: PathBuf,
    entries: Vec<HistoryEntry>,
    limit: usize,
    /// Holds the open lock file. The `fs2` exclusive lock is tied to the file
    /// descriptor; dropping this field releases the lock.
    _lock_file: File,
}

impl HistoryStore {
    /// Load history from `data_dir/history.json`.
    ///
    /// Acquires an exclusive advisory lock on `data_dir/history.lock` before
    /// reading the state file. The lock is held until the returned
    /// `HistoryStore` is dropped. If another process already holds the lock
    /// this call blocks until that process releases it.
    ///
    /// If the history file does not exist, returns an empty history. If the
    /// file is corrupted, logs a warning and starts fresh (graceful
    /// degradation).
    pub fn load(data_dir: &Path, limit: usize) -> Result<Self, FluxError> {
        let lock_path = data_dir.join("history.lock");
        let lock_file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| FluxError::Io { source: e })?;
        lock_file
            .lock_exclusive()
            .map_err(|e| FluxError::Io { source: e })?;

        let path = data_dir.join("history.json");

        if path.exists() {
            let contents = std::fs::read_to_string(&path).map_err(|e| FluxError::Io {
                source: e,
            })?;
            match serde_json::from_str::<Vec<HistoryEntry>>(&contents) {
                Ok(entries) => Ok(Self {
                    path,
                    entries,
                    limit,
                    _lock_file: lock_file,
                }),
                Err(e) => {
                    tracing::warn!("Corrupted history.json, starting fresh: {}", e);
                    Ok(Self {
                        path,
                        entries: Vec::new(),
                        limit,
                        _lock_file: lock_file,
                    })
                }
            }
        } else {
            Ok(Self {
                path,
                entries: Vec::new(),
                limit,
                _lock_file: lock_file,
            })
        }
    }

    /// Append a new entry to the history, truncating oldest if over limit.
    ///
    /// Automatically saves to disk after appending.
    pub fn append(&mut self, entry: HistoryEntry) -> Result<(), FluxError> {
        self.entries.push(entry);

        // Truncate oldest if over limit
        if self.entries.len() > self.limit {
            let excess = self.entries.len() - self.limit;
            self.entries.drain(..excess);
        }

        self.save()
    }

    /// Return a slice of all history entries.
    pub fn list(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Clear all history entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Save history to disk atomically (write temp, rename).
    pub fn save(&self) -> Result<(), FluxError> {
        let tmp_path = self.path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&tmp_path, json).map_err(|e| FluxError::Io { source: e })?;
        std::fs::rename(&tmp_path, &self.path).map_err(|e| FluxError::Io { source: e })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_history_returns_empty_slice() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::load(dir.path(), 1000).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn append_entry_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::load(dir.path(), 1000).unwrap();

        let entry = HistoryEntry {
            source: "/tmp/a.txt".to_string(),
            dest: "/tmp/b.txt".to_string(),
            bytes: 1024,
            files: 1,
            duration_secs: 0.5,
            timestamp: Utc::now(),
            status: "completed".to_string(),
            error: None,
        };

        store.append(entry).unwrap();
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].source, "/tmp/a.txt");
        assert_eq!(store.list()[0].bytes, 1024);
    }

    #[test]
    fn append_beyond_limit_truncates_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::load(dir.path(), 3).unwrap();

        for i in 0..5 {
            let entry = HistoryEntry {
                source: format!("src_{}", i),
                dest: format!("dst_{}", i),
                bytes: i as u64 * 100,
                files: 1,
                duration_secs: 0.1,
                timestamp: Utc::now(),
                status: "completed".to_string(),
                error: None,
            };
            store.append(entry).unwrap();
        }

        // Only last 3 entries should remain
        assert_eq!(store.list().len(), 3);
        assert_eq!(store.list()[0].source, "src_2");
        assert_eq!(store.list()[1].source, "src_3");
        assert_eq!(store.list()[2].source, "src_4");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();

        {
            let mut store = HistoryStore::load(dir.path(), 1000).unwrap();
            let entry = HistoryEntry {
                source: "roundtrip_src".to_string(),
                dest: "roundtrip_dst".to_string(),
                bytes: 2048,
                files: 2,
                duration_secs: 1.5,
                timestamp: Utc::now(),
                status: "completed".to_string(),
                error: None,
            };
            store.append(entry).unwrap();
        }

        {
            let store = HistoryStore::load(dir.path(), 1000).unwrap();
            assert_eq!(store.list().len(), 1);
            assert_eq!(store.list()[0].source, "roundtrip_src");
            assert_eq!(store.list()[0].bytes, 2048);
        }
    }

    #[test]
    fn clear_empties_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::load(dir.path(), 1000).unwrap();

        let entry = HistoryEntry {
            source: "src".to_string(),
            dest: "dst".to_string(),
            bytes: 100,
            files: 1,
            duration_secs: 0.1,
            timestamp: Utc::now(),
            status: "completed".to_string(),
            error: None,
        };

        store.append(entry).unwrap();
        assert_eq!(store.list().len(), 1);

        store.clear();
        assert!(store.list().is_empty());
    }

    #[test]
    fn corrupted_json_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("history.json"), "not valid json!!!").unwrap();
        let store = HistoryStore::load(dir.path(), 1000).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn failed_entry_with_error_message() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::load(dir.path(), 1000).unwrap();

        let entry = HistoryEntry {
            source: "fail_src".to_string(),
            dest: "fail_dst".to_string(),
            bytes: 0,
            files: 0,
            duration_secs: 0.0,
            timestamp: Utc::now(),
            status: "failed".to_string(),
            error: Some("Permission denied".to_string()),
        };

        store.append(entry).unwrap();
        let entries = store.list();
        assert_eq!(entries[0].status, "failed");
        assert_eq!(entries[0].error, Some("Permission denied".to_string()));
    }
}
