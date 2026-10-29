use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Everything the plugin reads from the process environment, read once.
pub struct Environment {
    pub state_dir: PathBuf,
    pub socket_path: Option<String>,
    pub herdr_bin: String,
    /// Where Herdr loads its config from, if it loads one and we can tell.
    pub config_path: Option<PathBuf>,
    pub event_json: Option<String>,
}

impl Environment {
    pub fn from_process() -> Self {
        Self::from_lookup(|name| std::env::var_os(name))
    }

    fn from_lookup(get: impl Fn(&str) -> Option<OsString>) -> Self {
        let text = |name: &str| get(name).and_then(|value| value.into_string().ok());
        // Herdr hands us an empty variable where it means "not set".
        let filled = |name: &str| text(name).filter(|value| !value.is_empty());
        Self {
            state_dir: get("HERDR_PLUGIN_STATE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("herdr-alt-tab")),
            socket_path: filled("HERDR_SOCKET_PATH"),
            herdr_bin: filled("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".to_string()),
            config_path: config_path(
                text("HERDR_CONFIG_PATH"),
                filled("XDG_CONFIG_HOME"),
                filled("HOME"),
            ),
            event_json: text("HERDR_PLUGIN_EVENT_JSON"),
        }
    }
}

/// Herdr's own lookup order. An explicitly empty path means it loads no config.
fn config_path(
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
    use std::collections::HashMap;

    fn some(text: &str) -> Option<String> {
        Some(text.to_string())
    }

    fn environment_of(vars: &[(&str, &str)]) -> Environment {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), OsString::from(value)))
            .collect();
        Environment::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn variables_are_read_as_given() {
        let environment = environment_of(&[
            ("HERDR_PLUGIN_STATE_DIR", "/state"),
            ("HERDR_SOCKET_PATH", "/run/herdr.sock"),
            ("HERDR_BIN_PATH", "/opt/herdr"),
            ("HERDR_PLUGIN_EVENT_JSON", "{}"),
        ]);

        assert_eq!(environment.state_dir, PathBuf::from("/state"));
        assert_eq!(environment.socket_path.as_deref(), Some("/run/herdr.sock"));
        assert_eq!(environment.herdr_bin, "/opt/herdr");
        assert_eq!(environment.event_json.as_deref(), Some("{}"));
    }

    #[test]
    fn unset_or_empty_variables_fall_back_to_defaults() {
        let environment = environment_of(&[("HERDR_SOCKET_PATH", ""), ("HERDR_BIN_PATH", "")]);

        assert_eq!(environment.socket_path, None);
        assert_eq!(environment.herdr_bin, "herdr");
        assert_eq!(
            environment.state_dir,
            std::env::temp_dir().join("herdr-alt-tab")
        );
        assert_eq!(environment.event_json, None);
    }

    #[test]
    fn an_explicit_path_wins() {
        let found = config_path(some("/x/c.toml"), some("/xdg"), some("/home/me"));

        assert_eq!(found, Some(PathBuf::from("/x/c.toml")));
    }

    #[test]
    fn without_an_explicit_path_xdg_config_home_wins() {
        let found = config_path(None, some("/xdg"), some("/home/me"));

        assert_eq!(found, Some(PathBuf::from("/xdg/herdr/config.toml")));
    }

    #[test]
    fn otherwise_the_home_directory_is_used() {
        let found = config_path(None, None, some("/home/me"));

        assert_eq!(
            found,
            Some(PathBuf::from("/home/me/.config/herdr/config.toml"))
        );
    }

    #[test]
    fn an_empty_explicit_path_means_herdr_loads_no_config() {
        let found = config_path(some(""), some("/xdg"), some("/home/me"));

        assert_eq!(found, None);
    }

    #[test]
    fn nothing_to_go_on_means_no_config_path() {
        let environment = environment_of(&[]);

        assert_eq!(environment.config_path, None);
    }
}
