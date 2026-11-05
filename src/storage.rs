//! The on-disk database shared by the store and the journal.
use std::path::Path;
use std::sync::Arc;

use redb::Database;

/// Open the database at `path`, creating the file when it does not exist. Its
/// directory must already exist. Only one process can hold it open at a time.
pub fn open(path: &Path) -> Result<Arc<Database>, redb::DatabaseError> {
    Database::create(path).map(Arc::new)
}

/// A unique directory for one test, removed when dropped.
#[cfg(test)]
pub(crate) struct TempDir(std::path::PathBuf);

#[cfg(test)]
impl TempDir {
    pub(crate) fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("rs3-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_open_of_the_same_file_is_refused() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let _first = open(&path).unwrap();
        assert!(open(&path).is_err());
    }

    #[test]
    fn missing_directory_is_an_error() {
        let dir = TempDir::new();
        assert!(open(&dir.path().join("missing").join("db")).is_err());
    }
}
