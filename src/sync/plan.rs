use std::fmt;
use std::path::PathBuf;

use bytesize::ByteSize;

/// An individual action determined by comparing source and dest trees.
#[derive(Debug, Clone)]
pub enum SyncAction {
    /// File exists in source but not dest -- copy it.
    CopyNew {
        src: PathBuf,
        dest: PathBuf,
        size: u64,
    },
    /// File exists in both but source is newer or different size -- update it.
    UpdateChanged {
        src: PathBuf,
        dest: PathBuf,
        src_size: u64,
        dest_size: u64,
    },
    /// File exists in dest but not source -- delete it (only with --delete).
    DeleteOrphan {
        path: PathBuf,
        size: u64,
    },
    /// File is identical in both trees -- skip it.
    Skip {
        path: PathBuf,
        reason: &'static str,
    },
}

impl fmt::Display for SyncAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncAction::CopyNew { dest, size, .. } => {
                write!(
                    f,
                    "  COPY    {} ({})",
                    dest.display(),
                    ByteSize(*size)
                )
            }
            SyncAction::UpdateChanged { dest, .. } => {
                write!(f, "  UPDATE  {} (changed)", dest.display())
            }
            SyncAction::DeleteOrphan { path, .. } => {
                write!(f, "  DELETE  {}", path.display())
            }
            SyncAction::Skip { path, reason } => {
                write!(f, "  SKIP    {} ({})", path.display(), reason)
            }
        }
    }
}

/// A computed sync plan: a list of actions with summary counts.
#[derive(Debug)]
pub struct SyncPlan {
    pub actions: Vec<SyncAction>,
    pub total_copy_bytes: u64,
    pub files_to_copy: u64,
    pub files_to_update: u64,
    pub files_to_delete: u64,
    pub files_to_skip: u64,
}

impl SyncPlan {
    /// Build a SyncPlan from a list of actions, computing summary counts.
    pub fn from_actions(actions: Vec<SyncAction>) -> Self {
        let mut total_copy_bytes = 0u64;
        let mut files_to_copy = 0u64;
        let mut files_to_update = 0u64;
        let mut files_to_delete = 0u64;
        let mut files_to_skip = 0u64;

        for action in &actions {
            match action {
                SyncAction::CopyNew { size, .. } => {
                    files_to_copy += 1;
                    total_copy_bytes += size;
                }
                SyncAction::UpdateChanged { src_size, .. } => {
                    files_to_update += 1;
                    total_copy_bytes += src_size;
                }
                SyncAction::DeleteOrphan { .. } => {
                    files_to_delete += 1;
                }
                SyncAction::Skip { .. } => {
                    files_to_skip += 1;
                }
            }
        }

        Self {
            actions,
            total_copy_bytes,
            files_to_copy,
            files_to_update,
            files_to_delete,
            files_to_skip,
        }
    }

    /// Returns true if the plan contains any action that isn't Skip.
    pub fn has_changes(&self) -> bool {
        self.files_to_copy > 0 || self.files_to_update > 0 || self.files_to_delete > 0
    }

    /// Print a human-readable summary of the plan to stderr.
    pub fn print_summary(&self) {
        eprintln!("Sync plan:");
        for action in &self.actions {
            eprintln!("{}", action);
        }
        eprintln!();
        eprintln!(
            "  {} to copy, {} to update, {} to delete, {} unchanged",
            self.files_to_copy, self.files_to_update, self.files_to_delete, self.files_to_skip
        );
        if self.total_copy_bytes > 0 {
            eprintln!("  Total transfer: {}", ByteSize(self.total_copy_bytes));
        }
    }
}

/// Result of executing a sync plan.
#[derive(Debug, Default)]
pub struct SyncResult {
    pub files_copied: u64,
    pub files_updated: u64,
    pub files_deleted: u64,
    pub files_skipped: u64,
    pub bytes_transferred: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_sync_plan_from_actions_counts() {
        let actions = vec![
            SyncAction::CopyNew {
                src: PathBuf::from("a.txt"),
                dest: PathBuf::from("b/a.txt"),
                size: 100,
            },
            SyncAction::UpdateChanged {
                src: PathBuf::from("c.txt"),
                dest: PathBuf::from("b/c.txt"),
                src_size: 200,
                dest_size: 150,
            },
            SyncAction::DeleteOrphan {
                path: PathBuf::from("b/old.txt"),
                size: 50,
            },
            SyncAction::Skip {
                path: PathBuf::from("same.txt"),
                reason: "unchanged",
            },
            SyncAction::Skip {
                path: PathBuf::from("same2.txt"),
                reason: "unchanged",
            },
        ];

        let plan = SyncPlan::from_actions(actions);
        assert_eq!(plan.files_to_copy, 1);
        assert_eq!(plan.files_to_update, 1);
        assert_eq!(plan.files_to_delete, 1);
        assert_eq!(plan.files_to_skip, 2);
        assert_eq!(plan.total_copy_bytes, 300); // 100 + 200
    }

    #[test]
    fn test_sync_plan_has_changes_true() {
        let actions = vec![SyncAction::CopyNew {
            src: PathBuf::from("new.txt"),
            dest: PathBuf::from("dest/new.txt"),
            size: 42,
        }];
        let plan = SyncPlan::from_actions(actions);
        assert!(plan.has_changes());
    }

    #[test]
    fn test_sync_plan_has_changes_false_all_skips() {
        let actions = vec![
            SyncAction::Skip {
                path: PathBuf::from("a.txt"),
                reason: "unchanged",
            },
            SyncAction::Skip {
                path: PathBuf::from("b.txt"),
                reason: "unchanged",
            },
        ];
        let plan = SyncPlan::from_actions(actions);
        assert!(!plan.has_changes());
    }

    #[test]
    fn test_sync_action_display() {
        let copy = SyncAction::CopyNew {
            src: PathBuf::from("src/file.txt"),
            dest: PathBuf::from("dest/file.txt"),
            size: 1024,
        };
        let display = format!("{}", copy);
        assert!(display.contains("COPY"));
        assert!(display.contains("dest/file.txt") || display.contains("dest\\file.txt"));

        let update = SyncAction::UpdateChanged {
            src: PathBuf::from("src/readme.md"),
            dest: PathBuf::from("dest/readme.md"),
            src_size: 200,
            dest_size: 100,
        };
        let display = format!("{}", update);
        assert!(display.contains("UPDATE"));
        assert!(display.contains("changed"));

        let delete = SyncAction::DeleteOrphan {
            path: PathBuf::from("dest/old.txt"),
            size: 50,
        };
        let display = format!("{}", delete);
        assert!(display.contains("DELETE"));

        let skip = SyncAction::Skip {
            path: PathBuf::from("same.txt"),
            reason: "unchanged",
        };
        let display = format!("{}", skip);
        assert!(display.contains("SKIP"));
        assert!(display.contains("unchanged"));
    }
}
