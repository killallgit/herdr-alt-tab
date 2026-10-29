use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::PathBuf;

use fs2::FileExt;

/// Excludes other Herdr Alt-Tab processes for as long as the guard is held.
pub struct Lock {
    path: PathBuf,
}

pub struct Held {
    _file: File,
}

impl Lock {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn acquire(&self) -> io::Result<Held> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.path)?;
        file.lock_exclusive()?;
        Ok(Held { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn the_lock_excludes_others_until_its_guard_drops() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("state.lock");
        let held = Lock::at(path.clone()).acquire().unwrap();
        let rival = || OpenOptions::new().write(true).open(&path).unwrap();

        let while_held = rival().try_lock_exclusive();
        drop(held);
        let after_drop = rival().try_lock_exclusive();

        assert!(while_held.is_err());
        assert!(after_drop.is_ok());
    }
}
