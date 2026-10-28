use std::ops::Range;

pub const PLUGIN_ID: &str = "killallgit.herdr-alt-tab";
const BLOCK_START: &str = "# >>> killallgit.herdr-alt-tab keys";
const BLOCK_END: &str = "# <<< killallgit.herdr-alt-tab keys";
const BINDINGS: [(&str, &str, &str); 2] = [
    ("alt+tab", "older", "Cycle panes: older"),
    ("alt+shift+tab", "newer", "Cycle panes: newer"),
];

fn block() -> String {
    let bindings: Vec<String> = BINDINGS
        .iter()
        .map(|(key, action, description)| {
            format!(
                "[[keys.command]]\nkey = \"{key}\"\ntype = \"plugin_action\"\n\
                 command = \"{PLUGIN_ID}.{action}\"\ndescription = \"{description}\"\n"
            )
        })
        .collect();
    format!("{BLOCK_START}\n{}{BLOCK_END}", bindings.join("\n"))
}

fn span_of_block(config: &str) -> Option<Range<usize>> {
    let start = config.find(BLOCK_START)?;
    let end = start + config[start..].find(BLOCK_END)? + BLOCK_END.len();
    Some(start..end)
}

pub fn with_bindings(config: &str) -> String {
    if let Some(span) = span_of_block(config) {
        return format!(
            "{}{}{}",
            &config[..span.start],
            block(),
            &config[span.end..]
        );
    }
    match config.trim_end() {
        "" => format!("{}\n", block()),
        existing => format!("{existing}\n\n{}\n", block()),
    }
}

pub fn without_bindings(config: &str) -> String {
    let Some(span) = span_of_block(config) else {
        return config.to_string();
    };
    let before = config[..span.start].trim_end();
    let after = config[span.end..].trim_start();
    match (before.is_empty(), after.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("{before}\n"),
        (true, false) => after.to_string(),
        (false, false) => format!("{before}\n\n{after}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OTHER_SETTINGS: &str = "[ui]\nsidebar_width = 30\n";

    fn bindings_in(config: &str) -> Vec<(String, String)> {
        let config: toml::Table = config.parse().unwrap();
        let commands = config
            .get("keys")
            .and_then(|keys| keys.get("command"))
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default();
        commands
            .iter()
            .map(|command| {
                let text = |field: &str| command[field].as_str().unwrap().to_string();
                (text("key"), text("command"))
            })
            .collect()
    }

    fn both_bindings() -> Vec<(String, String)> {
        vec![
            ("alt+tab".into(), format!("{PLUGIN_ID}.older")),
            ("alt+shift+tab".into(), format!("{PLUGIN_ID}.newer")),
        ]
    }

    #[test]
    fn both_keys_are_bound_to_their_actions() {
        let bound = with_bindings(OTHER_SETTINGS);

        let bindings = bindings_in(&bound);

        assert_eq!(bindings, both_bindings());
    }

    #[test]
    fn existing_settings_come_first_and_stay_intact() {
        let bound = with_bindings(OTHER_SETTINGS);

        assert!(bound.starts_with(OTHER_SETTINGS));
    }

    #[test]
    fn binding_twice_is_the_same_as_binding_once() {
        let once = with_bindings(OTHER_SETTINGS);

        let twice = with_bindings(&once);

        assert_eq!(twice, once);
    }

    #[test]
    fn an_outdated_block_is_replaced_where_it_stands() {
        let outdated = format!(
            "{BLOCK_START}\n[[keys.command]]\nkey = \"alt+q\"\ntype = \"plugin_action\"\n\
             command = \"{PLUGIN_ID}.older\"\n{BLOCK_END}\n\n{OTHER_SETTINGS}"
        );

        let bound = with_bindings(&outdated);

        assert_eq!(bindings_in(&bound), both_bindings());
        assert_eq!(bound.find(BLOCK_START), Some(0));
        assert!(bound.ends_with(OTHER_SETTINGS));
    }

    #[test]
    fn unbinding_leaves_the_rest_of_the_config_as_it_was() {
        let bound = with_bindings(OTHER_SETTINGS);

        let unbound = without_bindings(&bound);

        assert_eq!(unbound, OTHER_SETTINGS);
    }

    #[test]
    fn unbinding_a_block_between_other_settings_keeps_both_sides() {
        let sandwiched = format!("[a]\nx = 1\n\n{}\n\n[b]\ny = 2\n", block());

        let unbound = without_bindings(&sandwiched);

        assert_eq!(unbound, "[a]\nx = 1\n\n[b]\ny = 2\n");
    }

    #[test]
    fn unbinding_a_config_without_the_block_changes_nothing() {
        let unbound = without_bindings(OTHER_SETTINGS);

        assert_eq!(unbound, OTHER_SETTINGS);
    }

    #[test]
    fn every_bound_action_exists_in_the_manifest() {
        let manifest: toml::Table = include_str!("../../herdr-plugin.toml").parse().unwrap();
        let action_ids: Vec<String> = manifest["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|action| format!("{PLUGIN_ID}.{}", action["id"].as_str().unwrap()))
            .collect();

        let bound_commands: Vec<String> = bindings_in(&block())
            .into_iter()
            .map(|(_, command)| command)
            .collect();

        assert!(
            bound_commands
                .iter()
                .all(|command| action_ids.contains(command))
        );
        assert_eq!(manifest["id"].as_str(), Some(PLUGIN_ID));
    }
}
