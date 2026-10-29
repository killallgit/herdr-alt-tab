mod binary;
mod socket;

use std::io;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::environment::Environment;
use binary::HerdrBinary;
use socket::Socket;

#[derive(Debug, thiserror::Error)]
pub enum HerdrError {
    #[error("HERDR_SOCKET_PATH is not set")]
    NoSocket,
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("{request} failed: {reason}")]
    Rejected { request: String, reason: String },
    #[error(transparent)]
    Malformed(#[from] serde_json::Error),
    #[error("{0}: the response has no result")]
    MissingResult(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pane {
    #[serde(rename = "pane_id")]
    pub id: String,
    pub focused: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Panes {
    panes: Vec<Pane>,
}

impl Panes {
    #[cfg(test)]
    pub fn new(panes: Vec<Pane>) -> Self {
        Self { panes }
    }

    pub fn focused(&self) -> Option<&Pane> {
        self.panes.iter().find(|pane| pane.focused)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.panes.iter().map(|pane| pane.id.as_str())
    }
}

pub trait Herdr {
    fn panes(&self) -> Result<Panes, HerdrError>;
    fn focus(&self, pane_id: &str) -> Result<(), HerdrError>;
}

#[derive(Debug, thiserror::Error)]
#[error("`herdr config check` did not pass")]
pub struct CheckFailed;

/// The config as the running Herdr sees it.
pub trait LiveConfig {
    fn check(&self) -> Result<(), CheckFailed>;
    fn reload(&self);
}

enum Request<'a> {
    ListPanes,
    FocusPane(&'a str),
}

trait Transport {
    fn send(&self, request: &Request) -> Result<serde_json::Value, HerdrError>;
}

impl<T: Transport> Transport for Option<T> {
    fn send(&self, request: &Request) -> Result<serde_json::Value, HerdrError> {
        match self {
            Some(transport) => transport.send(request),
            None => Err(HerdrError::NoSocket),
        }
    }
}

struct Fallback<P, S> {
    primary: P,
    secondary: S,
}

impl<P: Transport, S: Transport> Fallback<P, S> {
    fn send<R: DeserializeOwned>(&self, request: &Request) -> Result<R, HerdrError> {
        let decode = |reply: serde_json::Value| Ok(serde_json::from_value(reply)?);
        self.primary
            .send(request)
            .and_then(decode)
            .or_else(|_| self.secondary.send(request).and_then(decode))
    }
}

impl<P: Transport, S: Transport> Herdr for Fallback<P, S> {
    fn panes(&self) -> Result<Panes, HerdrError> {
        self.send(&Request::ListPanes)
    }

    fn focus(&self, pane_id: &str) -> Result<(), HerdrError> {
        self.send::<serde_json::Value>(&Request::FocusPane(pane_id))
            .map(drop)
    }
}

/// Only the binary can check and reload the config.
pub fn live_config(environment: &Environment) -> impl LiveConfig {
    HerdrBinary::at(environment.herdr_bin.clone())
}

/// The socket is faster; the binary answers when the socket cannot.
pub fn connect(environment: &Environment) -> impl Herdr {
    Fallback {
        primary: environment.socket_path.as_deref().map(Socket::at),
        secondary: HerdrBinary::at(environment.herdr_bin.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeTransport {
        reply: Result<&'static str, &'static str>,
        asked: Cell<u32>,
    }

    impl FakeTransport {
        fn replying(json: &'static str) -> Self {
            Self {
                reply: Ok(json),
                asked: Cell::new(0),
            }
        }

        fn failing(reason: &'static str) -> Self {
            Self {
                reply: Err(reason),
                asked: Cell::new(0),
            }
        }
    }

    impl Transport for FakeTransport {
        fn send(&self, _: &Request) -> Result<serde_json::Value, HerdrError> {
            self.asked.set(self.asked.get() + 1);
            match self.reply {
                Ok(json) => Ok(serde_json::from_str(json).unwrap()),
                Err(reason) => Err(HerdrError::Rejected {
                    request: "fake".into(),
                    reason: reason.into(),
                }),
            }
        }
    }

    const ONE_PANE: &str = r#"{"panes":[{"pane_id":"p1","focused":true}]}"#;
    const ANOTHER_PANE: &str = r#"{"panes":[{"pane_id":"p2","focused":true}]}"#;

    fn first_pane_id(herdr: &impl Herdr) -> String {
        herdr.panes().unwrap().ids().next().unwrap().to_string()
    }

    fn two_panes_focused_on_the_second() -> Panes {
        serde_json::from_str(
            r#"{"panes":[{"pane_id":"p1","focused":false},{"pane_id":"p2","focused":true}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn panes_know_which_one_is_focused() {
        let panes = two_panes_focused_on_the_second();

        let focused = panes.focused().map(|pane| pane.id.as_str());

        assert_eq!(focused, Some("p2"));
    }

    #[test]
    fn the_primary_answers_when_it_can() {
        let herdr = Fallback {
            primary: FakeTransport::replying(ONE_PANE),
            secondary: FakeTransport::replying(ANOTHER_PANE),
        };

        let first = first_pane_id(&herdr);

        assert_eq!(first, "p1");
        assert_eq!(herdr.secondary.asked.get(), 0);
    }

    #[test]
    fn the_secondary_answers_when_the_primary_cannot() {
        let herdr = Fallback {
            primary: FakeTransport::failing("socket down"),
            secondary: FakeTransport::replying(ANOTHER_PANE),
        };

        assert_eq!(first_pane_id(&herdr), "p2");
    }

    #[test]
    fn the_secondary_answers_when_the_primary_replies_nonsense() {
        let herdr = Fallback {
            primary: FakeTransport::replying(r#"{"unexpected":true}"#),
            secondary: FakeTransport::replying(ANOTHER_PANE),
        };

        assert_eq!(first_pane_id(&herdr), "p2");
    }

    #[test]
    fn with_no_socket_the_secondary_answers() {
        let herdr = Fallback {
            primary: None::<FakeTransport>,
            secondary: FakeTransport::replying(ONE_PANE),
        };

        assert_eq!(first_pane_id(&herdr), "p1");
    }

    #[test]
    fn when_both_fail_the_secondarys_error_is_reported() {
        let herdr = Fallback {
            primary: FakeTransport::failing("socket down"),
            secondary: FakeTransport::failing("binary missing"),
        };

        let error = herdr.panes().unwrap_err();

        assert!(matches!(error, HerdrError::Rejected { reason, .. } if reason == "binary missing"));
    }

    #[test]
    fn focusing_succeeds_when_either_transport_accepts() {
        let herdr = Fallback {
            primary: FakeTransport::failing("socket down"),
            secondary: FakeTransport::replying("{}"),
        };

        assert!(herdr.focus("p1").is_ok());
    }
}
