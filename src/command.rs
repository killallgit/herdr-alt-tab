use crate::session::Direction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Press(Direction),
    Track,
    Watch { started_at: u64 },
    SetupKeys,
    RemoveKeys,
}

impl Command {
    pub const USAGE: &str = "usage: herdr-alt-tab <older|newer|track|setup-keys|remove-keys>";

    pub fn parse(args: &[String]) -> Option<Command> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match args[..] {
            ["older"] => Some(Command::Press(Direction::Older)),
            ["newer"] => Some(Command::Press(Direction::Newer)),
            ["track"] => Some(Command::Track),
            ["setup-keys"] => Some(Command::SetupKeys),
            ["remove-keys"] => Some(Command::RemoveKeys),
            ["watch", started_at] => Some(Command::Watch {
                started_at: started_at.parse().ok()?,
            }),
            _ => None,
        }
    }

    pub fn args(&self) -> Vec<String> {
        match self {
            Command::Press(Direction::Older) => vec!["older".into()],
            Command::Press(Direction::Newer) => vec!["newer".into()],
            Command::Track => vec!["track".into()],
            Command::Watch { started_at } => vec!["watch".into(), started_at.to_string()],
            Command::SetupKeys => vec!["setup-keys".into()],
            Command::RemoveKeys => vec!["remove-keys".into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_parses_back_from_its_own_args() {
        let commands = vec![
            Command::Press(Direction::Older),
            Command::Press(Direction::Newer),
            Command::Track,
            Command::Watch { started_at: 42 },
            Command::SetupKeys,
            Command::RemoveKeys,
        ];

        let reparsed: Vec<_> = commands
            .iter()
            .map(|command| Command::parse(&command.args()))
            .collect();

        assert_eq!(reparsed, commands.into_iter().map(Some).collect::<Vec<_>>());
    }

    #[test]
    fn unknown_or_malformed_args_are_not_commands() {
        let attempts: [&[&str]; 4] = [&["sideways"], &["watch"], &["watch", "soon"], &[]];

        let parsed: Vec<_> = attempts
            .iter()
            .map(|args| Command::parse(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>()))
            .collect();

        assert!(parsed.iter().all(Option::is_none));
    }

    #[test]
    fn every_manifest_action_and_event_runs_a_known_command() {
        let manifest: toml::Table = include_str!("../herdr-plugin.toml").parse().unwrap();
        let invocations: Vec<Vec<String>> = ["actions", "events"]
            .into_iter()
            .flat_map(|kind| manifest[kind].as_array().unwrap().iter())
            .map(|item| {
                item["command"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .skip(1)
                    .map(|arg| arg.as_str().unwrap().to_string())
                    .collect()
            })
            .collect();

        let all_known = invocations
            .iter()
            .all(|args| Command::parse(args).is_some());

        assert!(all_known);
    }
}
