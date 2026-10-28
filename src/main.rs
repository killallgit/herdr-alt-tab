mod clock;
mod command;
mod connection;
mod cycler;
mod herdr;
mod herdr_env;
mod history;
mod keyboard;
mod keys;
mod release;
mod session;
mod session_store;
mod state_file;
mod track;

#[cfg(test)]
mod test_support;

use anyhow::Context;

use clock::SystemClock;
use command::Command;
use connection::Connection;
use cycler::Cycler;
use herdr::binary::HerdrBinary;
use history::History;
use keyboard::SystemKeyboard;
use keys::ConfigEditor;
use release::{DetachedWatcher, ReleaseWaiter};
use session_store::SessionStore;
use track::Tracker;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = Command::parse(&args).context(Command::USAGE)?;

    let connection = Connection::from_env();
    let history = History::new(connection.state_file("history"));
    let sessions = SessionStore::new(connection.state_file("session"));

    match command {
        Command::Press(direction) => Cycler {
            herdr: &herdr::connect(&connection),
            keyboard: &SystemKeyboard,
            watcher: &DetachedWatcher,
            history: &history,
            sessions: &sessions,
            clock: &SystemClock,
        }
        .press(direction),
        Command::Track => {
            let event_json = std::env::var("HERDR_PLUGIN_EVENT_JSON")
                .context("HERDR_PLUGIN_EVENT_JSON is not set")?;
            Tracker {
                herdr: &herdr::connect(&connection),
                history: &history,
                sessions: &sessions,
                clock: &SystemClock,
            }
            .track(track::announced_pane(&event_json)?.as_deref())
        }
        Command::Watch { started_at } => ReleaseWaiter {
            keyboard: &SystemKeyboard,
            sessions: &sessions,
            clock: &SystemClock,
        }
        .wait(started_at),
        Command::SetupKeys => {
            let binary = HerdrBinary::from_env();
            Ok(ConfigEditor::for_connection(&connection, &binary)?.bind()?)
        }
        Command::RemoveKeys => {
            let binary = HerdrBinary::from_env();
            Ok(ConfigEditor::for_connection(&connection, &binary)?.unbind()?)
        }
    }
}
