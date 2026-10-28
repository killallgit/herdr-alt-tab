use std::path::{Path, PathBuf};

use crate::herdr_env;

pub fn from_env() -> Option<PathBuf> {
    resolve(
        std::env::var("HERDR_CONFIG_PATH").ok(),
        herdr_env::var("XDG_CONFIG_HOME"),
        herdr_env::var("HOME"),
    )
}

/// Herdr's own lookup order. An explicitly empty path means it loads no config.
fn resolve(
    explicit: Option<String>,
    xdg_config_home: Option<String>,
    home: Option<String>,
) -> Option<PathBuf> {
    if let Some(explicit) = explicit {
        return Some(PathBuf::from(explicit)).filter(|path| !path.as_os_str().is_empty());
    }
    xdg_config_home
        .map(|xdg| Path::new(&xdg).join("herdr/config.toml"))
        .or_else(|| home.map(|home| Path::new(&home).join(".config/herdr/config.toml")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(text: &str) -> Option<String> {
        Some(text.to_string())
    }

    #[test]
    fn an_explicit_path_wins() {
        let found = resolve(some("/x/c.toml"), some("/xdg"), some("/home/me"));

        assert_eq!(found, Some(PathBuf::from("/x/c.toml")));
    }

    #[test]
    fn without_an_explicit_path_xdg_config_home_wins() {
        let found = resolve(None, some("/xdg"), some("/home/me"));

        assert_eq!(found, Some(PathBuf::from("/xdg/herdr/config.toml")));
    }

    #[test]
    fn otherwise_the_home_directory_is_used() {
        let found = resolve(None, None, some("/home/me"));

        assert_eq!(
            found,
            Some(PathBuf::from("/home/me/.config/herdr/config.toml"))
        );
    }

    #[test]
    fn an_empty_explicit_path_means_herdr_loads_no_config() {
        let found = resolve(some(""), some("/xdg"), some("/home/me"));

        assert_eq!(found, None);
    }

    #[test]
    fn nothing_to_go_on_means_no_path() {
        let found = resolve(None, None, None);

        assert_eq!(found, None);
    }
}
