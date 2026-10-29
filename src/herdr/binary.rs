use std::process::Command;

use serde_json::Value;

use super::{CheckFailed, HerdrError, LiveConfig, Request, Transport};

pub struct HerdrBinary {
    path: String,
}

impl HerdrBinary {
    pub(super) fn at(path: String) -> Self {
        Self { path }
    }

    fn run(&self, args: &[&str]) -> Result<String, HerdrError> {
        let output = Command::new(&self.path).args(args).output()?;
        if !output.status.success() {
            return Err(HerdrError::Rejected {
                request: format!("herdr {}", args.join(" ")),
                reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Transport for HerdrBinary {
    fn send(&self, request: &Request) -> Result<Value, HerdrError> {
        let args: Vec<&str> = match request {
            Request::ListPanes => vec!["pane", "list"],
            Request::FocusPane(pane_id) => vec!["agent", "focus", pane_id],
        };
        let mut reply: Value = serde_json::from_str(&self.run(&args)?)?;
        reply
            .get_mut("result")
            .map(Value::take)
            .ok_or_else(|| HerdrError::MissingResult(format!("herdr {}", args.join(" "))))
    }
}

impl LiveConfig for HerdrBinary {
    fn check(&self) -> Result<(), CheckFailed> {
        self.run(&["config", "check"])
            .map(drop)
            .map_err(|_| CheckFailed)
    }

    fn reload(&self) {
        // Without a running Herdr the keys are live from its next start.
        let _ = self.run(&["server", "reload-config"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn herdr_running(dir: &TempDir, script: &str) -> HerdrBinary {
        let path = dir.path().join("herdr");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        wait_until_executable(&path);
        HerdrBinary::at(path.to_str().unwrap().to_string())
    }

    // A test thread forking while the script is still open for writing makes
    // exec fail with ETXTBSY until that child execs, so exec it until it takes.
    fn wait_until_executable(path: &std::path::Path) {
        const TEXT_FILE_BUSY: i32 = 26;
        for _ in 0..200 {
            match Command::new(path).output() {
                Err(error) if error.raw_os_error() == Some(TEXT_FILE_BUSY) => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                _ => return,
            }
        }
    }

    #[test]
    fn the_result_of_a_pane_list_is_returned() {
        let dir = TempDir::new().unwrap();
        let herdr = herdr_running(&dir, r#"echo '{"id":"x","result":{"panes":[]}}'"#);

        let result = herdr.send(&Request::ListPanes).unwrap();

        assert_eq!(result, serde_json::json!({"panes": []}));
    }

    #[test]
    fn a_failing_command_is_a_rejection() {
        let dir = TempDir::new().unwrap();
        let herdr = herdr_running(&dir, "echo no such pane >&2; exit 1");

        let error = herdr.send(&Request::FocusPane("gone")).unwrap_err();

        assert!(matches!(error, HerdrError::Rejected { .. }));
    }

    #[test]
    fn output_without_a_result_is_an_error() {
        let dir = TempDir::new().unwrap();
        let herdr = herdr_running(&dir, "echo '{}'");

        let error = herdr.send(&Request::ListPanes).unwrap_err();

        assert!(matches!(error, HerdrError::MissingResult(_)), "{error:?}");
    }

    #[test]
    fn output_that_is_not_json_is_an_error() {
        let dir = TempDir::new().unwrap();
        let herdr = herdr_running(&dir, "echo nonsense");

        let error = herdr.send(&Request::ListPanes).unwrap_err();

        assert!(matches!(error, HerdrError::Malformed(_)));
    }

    #[test]
    fn a_missing_binary_is_an_io_error() {
        let herdr = HerdrBinary::at("/nonexistent/herdr".to_string());

        let error = herdr.send(&Request::ListPanes).unwrap_err();

        assert!(matches!(error, HerdrError::Io(_)));
    }

    #[test]
    fn a_config_check_passes_when_herdr_exits_cleanly() {
        let dir = TempDir::new().unwrap();
        let herdr = herdr_running(&dir, "exit 0");

        assert!(herdr.check().is_ok());
    }

    #[test]
    fn a_config_check_fails_when_herdr_exits_with_an_error() {
        let dir = TempDir::new().unwrap();
        let herdr = herdr_running(&dir, "echo bad config >&2; exit 1");

        assert!(herdr.check().is_err());
    }

    #[test]
    fn a_reload_is_asked_for_and_a_failed_one_is_ignored() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("args");
        let herdr = herdr_running(&dir, &format!("echo \"$@\" > {}; exit 1", log.display()));

        herdr.reload();

        assert_eq!(
            std::fs::read_to_string(log).unwrap().trim(),
            "server reload-config"
        );
    }
}
