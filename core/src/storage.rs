use async_trait::async_trait;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("io: {0}")]
    IoError(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
}

#[async_trait]
pub trait Storage: Send + Sync {
    async fn store(&self, key: &str, data: &[u8]) -> Result<(), StorageError>;
    async fn retrieve(&self, key: &str) -> Result<Vec<u8>, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
}

pub struct LocalStorage {
    base_dir: PathBuf,
}

impl LocalStorage {
    pub fn new(base_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(base_dir)?;
        Ok(Self {
            base_dir: base_dir.to_path_buf(),
        })
    }

    /// Resolve and validate that the key stays within `base_dir`.
    ///
    /// Rejects keys containing `..` components, absolute paths, or
    /// any component that could escape the base directory.
    fn safe_path(&self, key: &str) -> Result<PathBuf, StorageError> {
        let key_path = Path::new(key);
        for component in key_path.components() {
            match component {
                Component::Normal(_) => {}
                _ => {
                    return Err(StorageError::IoError(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "path traversal attempt",
                    )));
                }
            }
        }
        Ok(self.base_dir.join(key))
    }
}

#[async_trait]
impl Storage for LocalStorage {
    async fn store(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let path = self.safe_path(key)?;
        if let Some(p) = path.parent() {
            tokio::fs::create_dir_all(p).await?;
        }
        tokio::fs::write(path, data).await?;
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        let path = self.safe_path(key)?;
        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Err(StorageError::NotFound(key.into()));
        }
        Ok(tokio::fs::read(path).await?)
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.safe_path(key)?;
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            tokio::fs::remove_file(path).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn roundtrip() {
        let tmp = TempDir::new().unwrap();
        let s = LocalStorage::new(tmp.path()).unwrap();
        s.store("test.txt", b"hello").await.unwrap();
        assert_eq!(s.retrieve("test.txt").await.unwrap(), b"hello");
    }

    #[tokio::test]
    async fn not_found() {
        let tmp = TempDir::new().unwrap();
        let s = LocalStorage::new(tmp.path()).unwrap();
        assert!(matches!(
            s.retrieve("nope").await,
            Err(StorageError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn delete_works() {
        let tmp = TempDir::new().unwrap();
        let s = LocalStorage::new(tmp.path()).unwrap();
        s.store("del.txt", b"x").await.unwrap();
        s.delete("del.txt").await.unwrap();
        assert!(matches!(
            s.retrieve("del.txt").await,
            Err(StorageError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let s = LocalStorage::new(tmp.path()).unwrap();
        let result = s.store("../escape.txt", b"x").await;
        assert!(result.is_err());
    }
}
