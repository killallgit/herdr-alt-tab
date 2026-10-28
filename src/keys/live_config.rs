use crate::herdr::binary::HerdrBinary;

#[derive(Debug, thiserror::Error)]
#[error("`herdr config check` did not pass")]
pub struct CheckFailed;

/// The config as the running Herdr sees it.
pub trait LiveConfig {
    fn check(&self) -> Result<(), CheckFailed>;
    fn reload(&self);
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
