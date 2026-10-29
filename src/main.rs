mod clock;
mod command;
mod connection;
mod cycle;
mod environment;
mod focus_event;
mod herdr;
mod history;
mod keyboard;
mod keys;
mod lock;
mod release;
mod state_file;

#[cfg(test)]
mod test_support;

use anyhow::Context;

use clock::SystemClock;
use command::Command;
use connection::Connection;
use cycle::{Collaborators, Cycle};
use environment::Environment;
use history::History;
use keyboard::SystemKeyboard;
use keys::ConfigEditor;
use release::{DetachedWatcher, ReleaseWaiter};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = Command::parse(&args).context(Command::USAGE)?;

    let environment = Environment::from_process();
    let connection = Connection::from_environment(&environment);
    let history = History::new(connection.state_file("history"), connection.lock("history"));
    let herdr = herdr::connect(&environment);
    let cycle = Cycle::new(
        &connection,
        Collaborators {
            herdr: &herdr,
            keyboard: &SystemKeyboard,
            watcher: &DetachedWatcher,
            history: &history,
            clock: &SystemClock,
        },
    );

    match command {
        Command::Press(direction) => cycle.press(direction),
        Command::Track => {
            let event_json = environment
                .event_json
                .as_deref()
                .context("HERDR_PLUGIN_EVENT_JSON is not set")?;
            cycle.observe_focus(focus_event::announced_pane(event_json)?.as_deref())
        }
        Command::Watch { cycle: id } => ReleaseWaiter {
            keyboard: &SystemKeyboard,
            cycle: &cycle,
            clock: &SystemClock,
        }
        .wait(id),
        Command::SetupKeys => {
            Ok(
                ConfigEditor::new(&environment, &connection, &herdr::live_config(&environment))?
                    .bind()?,
            )
        }
        Command::RemoveKeys => {
            Ok(
                ConfigEditor::new(&environment, &connection, &herdr::live_config(&environment))?
                    .unbind()?,
            )
        }
    }
}
