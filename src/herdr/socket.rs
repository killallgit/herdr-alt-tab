use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::json;

use super::{HerdrError, Request, Transport};

// Callers hold the cycle lock while waiting, so a stalled Herdr must not hold it forever.
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

pub struct Socket {
    path: String,
    reply_timeout: Duration,
}

impl Socket {
    pub fn at(path: &str) -> Self {
        Self {
            path: path.to_string(),
            reply_timeout: REPLY_TIMEOUT,
        }
    }
}

impl Transport for Socket {
    fn send(&self, request: &Request) -> Result<serde_json::Value, HerdrError> {
        let (method, params) = match request {
            Request::ListPanes => ("pane.list", json!({})),
            Request::FocusPane(pane_id) => ("pane.focus", json!({"pane_id": pane_id})),
        };
        let mut stream = UnixStream::connect(&self.path)?;
        stream.set_read_timeout(Some(self.reply_timeout))?;
        let call = json!({"jsonrpc": "2.0", "id": "1", "method": method, "params": params});
        writeln!(stream, "{call}")?;

        let mut reply = String::new();
        BufReader::new(&stream).read_line(&mut reply)?;
        let mut reply: serde_json::Value = serde_json::from_str(&reply)?;
        if let Some(error) = reply.get("error") {
            return Err(HerdrError::Rejected {
                request: method.to_string(),
                reason: error.to_string(),
            });
        }
        reply
            .get_mut("result")
            .map(serde_json::Value::take)
            .ok_or_else(|| HerdrError::MissingResult(method.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use tempfile::TempDir;

    fn herdr_replying(dir: &TempDir, reply: &'static str) -> Socket {
        let path = dir.path().join("herdr.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut call = String::new();
                BufReader::new(&stream).read_line(&mut call).unwrap();
                writeln!(&stream, "{reply}").unwrap();
            }
        });
        Socket::at(path.to_str().unwrap())
    }

    #[test]
    fn the_result_of_a_pane_list_is_returned() {
        let dir = TempDir::new().unwrap();
        let socket = herdr_replying(&dir, r#"{"result":{"panes":[]}}"#);

        let result = socket.send(&Request::ListPanes).unwrap();

        assert_eq!(result, json!({"panes": []}));
    }

    #[test]
    fn an_error_reply_is_a_rejection() {
        let dir = TempDir::new().unwrap();
        let socket = herdr_replying(&dir, r#"{"error":{"code":"pane_not_found"}}"#);

        let error = socket.send(&Request::FocusPane("gone")).unwrap_err();

        assert!(matches!(error, HerdrError::Rejected { .. }));
    }

    #[test]
    fn a_reply_without_a_result_is_an_error() {
        let dir = TempDir::new().unwrap();
        let socket = herdr_replying(&dir, r#"{"id":"1"}"#);

        let error = socket.send(&Request::ListPanes).unwrap_err();

        assert!(matches!(error, HerdrError::MissingResult(_)));
    }

    #[test]
    fn a_reply_that_is_not_json_is_an_error() {
        let dir = TempDir::new().unwrap();
        let socket = herdr_replying(&dir, "not json");

        let error = socket.send(&Request::ListPanes).unwrap_err();

        assert!(matches!(error, HerdrError::Malformed(_)));
    }

    #[test]
    fn a_herdr_that_never_replies_is_an_io_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("herdr.sock");
        let _listener = UnixListener::bind(&path).unwrap();
        let socket = Socket {
            reply_timeout: Duration::from_millis(50),
            ..Socket::at(path.to_str().unwrap())
        };

        let error = socket.send(&Request::ListPanes).unwrap_err();

        assert!(matches!(error, HerdrError::Io(_)));
    }

    #[test]
    fn an_unreachable_socket_is_an_io_error() {
        let dir = TempDir::new().unwrap();
        let socket = Socket::at(dir.path().join("nobody-home.sock").to_str().unwrap());

        let error = socket.send(&Request::ListPanes).unwrap_err();

        assert!(matches!(error, HerdrError::Io(_)));
    }
}
