//! Leader lease so two keeper instances can run side by side.
//!
//! Choice: a small JSON file with holder id and expiry, written atomically
//! (temp file + rename). Both instances point at the same path (same host, or a
//! shared volume). The lease is advisory: if it is ever held twice the program's
//! state machine rejects duplicate cranks, so the cost is a few failed transactions,
//! never a wrong state.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize, Debug)]
struct LeaseFile {
    holder: String,
    expires_unix: u64,
}

pub struct FileLease {
    path: PathBuf,
    holder: String,
    ttl: Duration,
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl FileLease {
    pub fn new(path: impl Into<PathBuf>, holder: impl Into<String>, ttl: Duration) -> Self {
        Self { path: path.into(), holder: holder.into(), ttl }
    }

    pub fn holder(&self) -> &str {
        &self.holder
    }

    fn read(path: &Path) -> Option<LeaseFile> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write(&self, lease: &LeaseFile) -> Result<()> {
        let tmp = self.path.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec(lease)?).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).with_context(|| format!("renaming into {}", self.path.display()))?;
        Ok(())
    }

    /// Take or renew the lease. Returns true when this instance is the leader.
    pub fn try_acquire(&self) -> Result<bool> {
        let now = now_unix();
        let free = match Self::read(&self.path) {
            None => true,
            Some(l) => l.holder == self.holder || l.expires_unix <= now,
        };
        if !free {
            return Ok(false);
        }
        self.write(&LeaseFile { holder: self.holder.clone(), expires_unix: now + self.ttl.as_secs() })?;
        // Re-read to lose gracefully if another instance renamed over us in the same instant.
        Ok(Self::read(&self.path).map(|l| l.holder == self.holder).unwrap_or(false))
    }

    /// Give the lease up on shutdown so the standby takes over without waiting for expiry.
    pub fn release(&self) {
        if let Some(l) = Self::read(&self.path) {
            if l.holder == self.holder {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("carrera-lease-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn second_instance_waits_for_expiry() {
        let path = tmp("a");
        let a = FileLease::new(&path, "a", Duration::from_secs(60));
        let b = FileLease::new(&path, "b", Duration::from_secs(60));
        assert!(a.try_acquire().unwrap());
        assert!(!b.try_acquire().unwrap());
        assert!(a.try_acquire().unwrap()); // renew
        a.release();
        assert!(b.try_acquire().unwrap());
        b.release();
    }

    #[test]
    fn expired_lease_is_taken_over() {
        let path = tmp("b");
        let a = FileLease::new(&path, "a", Duration::from_secs(0));
        let b = FileLease::new(&path, "b", Duration::from_secs(60));
        assert!(a.try_acquire().unwrap());
        assert!(b.try_acquire().unwrap());
        assert!(!a.try_acquire().unwrap());
        b.release();
    }
}
