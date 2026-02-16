//! Transfer resume manifest for persisting chunk completion state.
//!
//! When `--resume` is active, a `.flux-resume.json` sidecar file is created
//! next to the destination file. It tracks which chunks have been completed,
//! allowing interrupted transfers to continue from where they left off.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::FluxError;
use crate::transfer::chunk::ChunkPlan;

/// Persistent manifest for resumable transfers.
///
/// Serialized to JSON and saved as a sidecar file next to the destination.
/// On resume, completed chunks are skipped and only incomplete chunks are
/// transferred.
#[derive(Debug, Serialize, Deserialize)]
pub struct TransferManifest {
    /// Manifest format version (currently 1).
    pub version: u32,
    /// Original source path.
    pub source: PathBuf,
    /// Destination path.
    pub dest: PathBuf,
    /// Total file size in bytes.
    pub total_size: u64,
    /// Number of chunks in this transfer.
    pub chunk_count: usize,
    /// Per-chunk completion state.
    pub chunks: Vec<ChunkPlan>,
    /// Whether compression was enabled for this transfer.
    pub compress: bool,
    /// Whole-file BLAKE3 checksum (populated after completion if --verify).
    pub file_checksum: Option<String>,
}

impl TransferManifest {
    /// Compute the manifest sidecar file path for a given destination.
    ///
    /// Returns `<dest_dir>/<dest_filename>.flux-resume.json`.
    /// For example, `/tmp/data.bin` -> `/tmp/data.bin.flux-resume.json`.
    pub fn manifest_path(dest: &Path) -> PathBuf {
        let file_name = dest
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("unknown"));
        let mut manifest_name = file_name.to_os_string();
        manifest_name.push(".flux-resume.json");
        dest.with_file_name(manifest_name)
    }

    /// Create a new manifest for a fresh transfer.
    pub fn new(
        source: PathBuf,
        dest: PathBuf,
        total_size: u64,
        chunks: Vec<ChunkPlan>,
        compress: bool,
    ) -> Self {
        let chunk_count = chunks.len();
        Self {
            version: 1,
            source,
            dest,
            total_size,
            chunk_count,
            chunks,
            compress,
            file_checksum: None,
        }
    }

    /// Save the manifest to disk as a JSON sidecar file.
    ///
    /// Uses write + flush + sync_all for crash safety.
    pub fn save(&self, dest: &Path) -> Result<(), FluxError> {
        let path = Self::manifest_path(dest);
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            FluxError::ResumeError(format!("Failed to serialize manifest: {}", e))
        })?;
        let mut file = fs::File::create(&path).map_err(|e| {
            FluxError::ResumeError(format!(
                "Failed to create manifest file {}: {}",
                path.display(),
                e
            ))
        })?;
        file.write_all(json.as_bytes()).map_err(|e| {
            FluxError::ResumeError(format!("Failed to write manifest: {}", e))
        })?;
        file.flush().map_err(|e| {
            FluxError::ResumeError(format!("Failed to flush manifest: {}", e))
        })?;
        file.sync_all().map_err(|e| {
            FluxError::ResumeError(format!("Failed to sync manifest: {}", e))
        })?;
        Ok(())
    }

    /// Load a manifest from disk if one exists for the given destination.
    ///
    /// Returns `Ok(None)` if no manifest file is found.
    pub fn load(dest: &Path) -> Result<Option<Self>, FluxError> {
        let path = Self::manifest_path(dest);
        if !path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(&path).map_err(|e| {
            FluxError::ResumeError(format!(
                "Failed to read manifest {}: {}",
                path.display(),
                e
            ))
        })?;
        let manifest: Self = serde_json::from_str(&json).map_err(|e| {
            FluxError::ResumeError(format!("Failed to parse manifest: {}", e))
        })?;
        Ok(Some(manifest))
    }

    /// Delete the manifest sidecar file if it exists.
    ///
    /// Called after successful transfer completion.
    pub fn cleanup(dest: &Path) -> Result<(), FluxError> {
        let path = Self::manifest_path(dest);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| {
                FluxError::ResumeError(format!(
                    "Failed to remove manifest {}: {}",
                    path.display(),
                    e
                ))
            })?;
        }
        Ok(())
    }

    /// Check if this manifest is compatible with the given source and size.
    ///
    /// Returns `false` if the source path or file size has changed since
    /// the manifest was created, meaning the transfer must restart.
    pub fn is_compatible(&self, source: &Path, total_size: u64) -> bool {
        self.source == source && self.total_size == total_size
    }

    /// Count how many chunks have been completed.
    pub fn completed_count(&self) -> usize {
        self.chunks.iter().filter(|c| c.completed).count()
    }

    /// Sum the byte length of all completed chunks.
    pub fn completed_bytes(&self) -> u64 {
        self.chunks
            .iter()
            .filter(|c| c.completed)
            .map(|c| c.length)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::chunk::chunk_file;

    #[test]
    fn manifest_path_for_simple_dest() {
        let dest = PathBuf::from("/tmp/data.bin");
        let path = TransferManifest::manifest_path(&dest);
        assert_eq!(path, PathBuf::from("/tmp/data.bin.flux-resume.json"));
    }

    #[test]
    fn manifest_path_for_nested_dest() {
        let dest = PathBuf::from("/home/user/docs/report.pdf");
        let path = TransferManifest::manifest_path(&dest);
        assert_eq!(
            path,
            PathBuf::from("/home/user/docs/report.pdf.flux-resume.json")
        );
    }

    #[test]
    fn manifest_path_for_windows_style_path() {
        let dest = PathBuf::from("C:\\Users\\test\\file.txt");
        let path = TransferManifest::manifest_path(&dest);
        assert_eq!(
            path,
            PathBuf::from("C:\\Users\\test\\file.txt.flux-resume.json")
        );
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("output.bin");

        let chunks = chunk_file(1000, 4);
        let manifest = TransferManifest::new(
            PathBuf::from("/tmp/source.bin"),
            dest.clone(),
            1000,
            chunks,
            false,
        );

        manifest.save(&dest).unwrap();

        let loaded = TransferManifest::load(&dest).unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.source, PathBuf::from("/tmp/source.bin"));
        assert_eq!(loaded.total_size, 1000);
        assert_eq!(loaded.chunk_count, 4);
        assert_eq!(loaded.chunks.len(), 4);
        assert!(!loaded.compress);
        assert!(loaded.file_checksum.is_none());
    }

    #[test]
    fn save_and_load_with_completed_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("output.bin");

        let mut chunks = chunk_file(1000, 4);
        chunks[0].completed = true;
        chunks[0].checksum = Some("abc123".to_string());
        chunks[1].completed = true;
        chunks[1].checksum = Some("def456".to_string());

        let manifest = TransferManifest::new(
            PathBuf::from("/tmp/source.bin"),
            dest.clone(),
            1000,
            chunks,
            true,
        );

        manifest.save(&dest).unwrap();

        let loaded = TransferManifest::load(&dest).unwrap().unwrap();
        assert_eq!(loaded.completed_count(), 2);
        assert!(loaded.chunks[0].completed);
        assert_eq!(loaded.chunks[0].checksum, Some("abc123".to_string()));
        assert!(loaded.chunks[1].completed);
        assert!(!loaded.chunks[2].completed);
        assert!(!loaded.chunks[3].completed);
        assert!(loaded.compress);
    }

    #[test]
    fn load_returns_none_when_no_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("no_manifest_here.bin");

        let loaded = TransferManifest::load(&dest).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn cleanup_removes_manifest_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("output.bin");

        let chunks = chunk_file(100, 2);
        let manifest = TransferManifest::new(
            PathBuf::from("/tmp/src.bin"),
            dest.clone(),
            100,
            chunks,
            false,
        );

        manifest.save(&dest).unwrap();

        let manifest_path = TransferManifest::manifest_path(&dest);
        assert!(manifest_path.exists());

        TransferManifest::cleanup(&dest).unwrap();
        assert!(!manifest_path.exists());
    }

    #[test]
    fn cleanup_is_noop_when_no_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("nothing.bin");

        // Should not error
        TransferManifest::cleanup(&dest).unwrap();
    }

    #[test]
    fn is_compatible_matches_source_and_size() {
        let chunks = chunk_file(1000, 2);
        let manifest = TransferManifest::new(
            PathBuf::from("/tmp/source.bin"),
            PathBuf::from("/tmp/dest.bin"),
            1000,
            chunks,
            false,
        );

        assert!(manifest.is_compatible(Path::new("/tmp/source.bin"), 1000));
    }

    #[test]
    fn is_compatible_false_when_source_differs() {
        let chunks = chunk_file(1000, 2);
        let manifest = TransferManifest::new(
            PathBuf::from("/tmp/source.bin"),
            PathBuf::from("/tmp/dest.bin"),
            1000,
            chunks,
            false,
        );

        assert!(!manifest.is_compatible(Path::new("/tmp/other.bin"), 1000));
    }

    #[test]
    fn is_compatible_false_when_size_differs() {
        let chunks = chunk_file(1000, 2);
        let manifest = TransferManifest::new(
            PathBuf::from("/tmp/source.bin"),
            PathBuf::from("/tmp/dest.bin"),
            1000,
            chunks,
            false,
        );

        assert!(!manifest.is_compatible(Path::new("/tmp/source.bin"), 2000));
    }

    #[test]
    fn completed_count_and_bytes() {
        let mut chunks = chunk_file(1000, 4);
        // Each chunk is 250 bytes
        chunks[0].completed = true;
        chunks[2].completed = true;

        let manifest = TransferManifest::new(
            PathBuf::from("/tmp/src.bin"),
            PathBuf::from("/tmp/dst.bin"),
            1000,
            chunks,
            false,
        );

        assert_eq!(manifest.completed_count(), 2);
        assert_eq!(manifest.completed_bytes(), 500);
    }

    #[test]
    fn new_sets_chunk_count() {
        let chunks = chunk_file(500, 5);
        let manifest = TransferManifest::new(
            PathBuf::from("/src"),
            PathBuf::from("/dst"),
            500,
            chunks,
            false,
        );
        assert_eq!(manifest.chunk_count, 5);
    }
}
