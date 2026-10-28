use std::io;
use std::path::PathBuf;

use super::block::{with_bindings, without_bindings};
use super::config_path;
use super::live_config::LiveConfig;
use crate::connection::Connection;
use crate::state_file::StateFile;

#[derive(Debug, thiserror::Error)]
pub enum KeysError {
    #[error("cannot tell where Herdr's config.toml is")]
    ConfigPathUnknown,
    #[error("the config already fails `herdr config check`, so nothing was changed")]
    ConfigAlreadyInvalid,
    #[error("Herdr rejected the new config, so the original was restored")]
    ConfigRejected,
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub struct ConfigEditor<'a> {
    config_path: PathBuf,
    lock: StateFile,
    herdr: &'a dyn LiveConfig,
}

impl<'a> ConfigEditor<'a> {
    pub fn for_connection(
        connection: &Connection,
        herdr: &'a dyn LiveConfig,
    ) -> Result<Self, KeysError> {
        Ok(Self {
            config_path: config_path::from_env().ok_or(KeysError::ConfigPathUnknown)?,
            lock: connection.state_file("keys"),
            herdr,
        })
    }

    pub fn bind(&self) -> Result<(), KeysError> {
        self.rewrite(with_bindings)
    }

    pub fn unbind(&self) -> Result<(), KeysError> {
        self.rewrite(without_bindings)
    }

    // Writes through a symlinked config, and puts the original back if Herdr rejects the result.
    fn rewrite(&self, transform: fn(&str) -> String) -> Result<(), KeysError> {
        let _turn = self.lock.lock()?;
        let config = StateFile::at(
            std::fs::canonicalize(&self.config_path).unwrap_or_else(|_| self.config_path.clone()),
        );
        let original = config.read();
        let before = original.as_deref().unwrap_or("");
        let after = transform(before);
        if after == before {
            return Ok(());
        }

        self.herdr
            .check()
            .map_err(|_| KeysError::ConfigAlreadyInvalid)?;
        config.write(&after)?;
        if self.herdr.check().is_err() {
            match &original {
                Some(original) => config.write(original)?,
                None => config.remove(),
            }
            return Err(KeysError::ConfigRejected);
        }
        self.herdr.reload();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::live_config::CheckFailed;
    use super::*;
    use std::cell::Cell;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    const OTHER_SETTINGS: &str = "[ui]\nsidebar_width = 30\n";

    struct FakeLiveConfig {
        config: PathBuf,
        rejects_config_containing: Option<&'static str>,
        reloads: Cell<u32>,
    }

    impl FakeLiveConfig {
        fn accepting(config: &Path) -> Self {
            Self {
                config: config.to_path_buf(),
                rejects_config_containing: None,
                reloads: Cell::new(0),
            }
        }

        fn rejecting(config: &Path, marker: &'static str) -> Self {
            Self {
                rejects_config_containing: Some(marker),
                ..Self::accepting(config)
            }
        }
    }

    impl LiveConfig for FakeLiveConfig {
        fn check(&self) -> Result<(), CheckFailed> {
            let contents = fs::read_to_string(&self.config).unwrap_or_default();
            match self.rejects_config_containing {
                Some(marker) if contents.contains(marker) => Err(CheckFailed),
                _ => Ok(()),
            }
        }

        fn reload(&self) {
            self.reloads.set(self.reloads.get() + 1);
        }
    }

    fn editor<'a>(dir: &TempDir, config: &Path, herdr: &'a FakeLiveConfig) -> ConfigEditor<'a> {
        ConfigEditor {
            config_path: config.to_path_buf(),
            lock: StateFile::at(dir.path().join("state").join("keys.json")),
            herdr,
        }
    }

    fn config_file_with(dir: &TempDir, contents: &str) -> PathBuf {
        let path = dir.path().join("config.toml");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn binding_writes_the_bindings() {
        let dir = TempDir::new().unwrap();
        let path = config_file_with(&dir, OTHER_SETTINGS);
        let herdr = FakeLiveConfig::accepting(&path);
        editor(&dir, &path, &herdr).bind().unwrap();

        let written = fs::read_to_string(&path).unwrap();

        assert_eq!(written, with_bindings(OTHER_SETTINGS));
    }

    #[test]
    fn binding_reloads_herdr() {
        let dir = TempDir::new().unwrap();
        let path = config_file_with(&dir, OTHER_SETTINGS);
        let herdr = FakeLiveConfig::accepting(&path);

        editor(&dir, &path, &herdr).bind().unwrap();

        assert_eq!(herdr.reloads.get(), 1);
    }

    #[test]
    fn binding_creates_a_config_that_does_not_exist_yet() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("herdr").join("config.toml");
        let herdr = FakeLiveConfig::accepting(&path);
        editor(&dir, &path, &herdr).bind().unwrap();

        let written = fs::read_to_string(&path).unwrap();

        assert_eq!(written, with_bindings(""));
    }

    #[test]
    fn binding_an_already_bound_config_does_not_reload_herdr() {
        let dir = TempDir::new().unwrap();
        let path = config_file_with(&dir, &with_bindings(OTHER_SETTINGS));
        let herdr = FakeLiveConfig::accepting(&path);

        editor(&dir, &path, &herdr).bind().unwrap();

        assert_eq!(herdr.reloads.get(), 0);
    }

    #[test]
    fn a_config_herdr_rejects_is_restored() {
        let dir = TempDir::new().unwrap();
        let path = config_file_with(&dir, OTHER_SETTINGS);
        let herdr = FakeLiveConfig::rejecting(&path, "alt+tab");
        let result = editor(&dir, &path, &herdr).bind();

        let restored = fs::read_to_string(&path).unwrap();

        assert!(matches!(result, Err(KeysError::ConfigRejected)));
        assert_eq!(restored, OTHER_SETTINGS);
        assert_eq!(herdr.reloads.get(), 0);
    }

    #[test]
    fn a_config_that_was_already_broken_is_left_alone() {
        let dir = TempDir::new().unwrap();
        let path = config_file_with(&dir, "[ui\nbroken");
        let herdr = FakeLiveConfig::rejecting(&path, "broken");
        let result = editor(&dir, &path, &herdr).bind();

        let untouched = fs::read_to_string(&path).unwrap();

        assert!(matches!(result, Err(KeysError::ConfigAlreadyInvalid)));
        assert_eq!(untouched, "[ui\nbroken");
    }

    #[test]
    fn binding_through_a_symlink_updates_the_file_it_points_to() {
        let dir = TempDir::new().unwrap();
        let target = config_file_with(&dir, OTHER_SETTINGS);
        let link = dir.path().join("link.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let herdr = FakeLiveConfig::accepting(&target);
        editor(&dir, &link, &herdr).bind().unwrap();

        let still_a_link = fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink();
        let written = fs::read_to_string(&target).unwrap();

        assert!(still_a_link);
        assert_eq!(written, with_bindings(OTHER_SETTINGS));
    }

    #[test]
    fn unbinding_removes_the_bindings() {
        let dir = TempDir::new().unwrap();
        let path = config_file_with(&dir, &with_bindings(OTHER_SETTINGS));
        let herdr = FakeLiveConfig::accepting(&path);
        editor(&dir, &path, &herdr).unbind().unwrap();

        let written = fs::read_to_string(&path).unwrap();

        assert_eq!(written, OTHER_SETTINGS);
    }

    #[test]
    fn unbinding_with_no_config_creates_nothing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("herdr").join("config.toml");
        let herdr = FakeLiveConfig::accepting(&path);
        editor(&dir, &path, &herdr).unbind().unwrap();

        let created = path.exists() || path.parent().unwrap().exists();

        assert!(!created);
    }
}
