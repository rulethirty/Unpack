//! UI-independent archive inspection and managed portable-application installation.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod archive;
pub mod compatibility;
pub mod config;
pub mod desktop;
pub mod detection;
pub mod identity;
pub mod installer;
pub mod security;

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid configuration or manifest: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Archive error: {0}")]
    Archive(String),
    #[error("Security validation failed: {0}")]
    Security(String),
    #[error("Operation conflicts with existing files: {0}")]
    Conflict(String),
    #[error("Operation cancelled; staged changes were discarded")]
    Cancelled,
    #[error("Unsupported operation: {0}")]
    Unsupported(String),
    #[error("Invalid input: {0}")]
    Invalid(String),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub phase: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub total: Option<u64>,
}

pub fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
