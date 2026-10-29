use std::path::PathBuf;

use crate::environment::Environment;
use crate::lock::Lock;
use crate::state_file::StateFile;

pub struct Connection {
    state_dir: PathBuf,
    socket_path: Option<String>,
}

impl Connection {
    pub fn new(state_dir: PathBuf, socket_path: Option<String>) -> Self {
        Self {
            state_dir,
            socket_path,
        }
    }

    pub fn from_environment(environment: &Environment) -> Self {
        Self::new(
            environment.state_dir.clone(),
            environment.socket_path.clone(),
        )
    }

    // Pane ids mean nothing across Herdr instances, so each keeps its own state.
    pub fn state_file(&self, stem: &str) -> StateFile {
        StateFile::at(self.path_of(stem, "json"))
    }

    pub fn lock(&self, stem: &str) -> Lock {
        Lock::at(self.path_of(stem, "lock"))
    }

    fn path_of(&self, stem: &str, extension: &str) -> PathBuf {
        let file_name = match &self.socket_path {
            Some(socket) => format!("{stem}-{:016x}.{extension}", fnv1a(socket)),
            None => format!("{stem}.{extension}"),
        };
        self.state_dir.join(file_name)
    }
}

// Not DefaultHasher: every process and every build must agree on the file name.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn connection(dir: &TempDir, socket: &str) -> Connection {
        Connection::new(dir.path().to_path_buf(), Some(socket.to_string()))
    }

    #[test]
    fn connections_to_different_sockets_do_not_share_state() {
        let dir = TempDir::new().unwrap();
        let a = connection(&dir, "/run/a.sock").state_file("history");
        let b = connection(&dir, "/run/b.sock").state_file("history");
        a.write("from a").unwrap();

        let seen_by_b = b.read();

        assert_eq!(seen_by_b, None);
    }

    #[test]
    fn the_same_socket_always_reaches_the_same_state() {
        let dir = TempDir::new().unwrap();
        connection(&dir, "/run/a.sock")
            .state_file("history")
            .write("saved")
            .unwrap();

        let again = connection(&dir, "/run/a.sock").state_file("history");

        let contents = again.read();

        assert_eq!(contents.as_deref(), Some("saved"));
    }

    #[test]
    fn different_kinds_of_state_do_not_collide() {
        let dir = TempDir::new().unwrap();
        let connection = connection(&dir, "/run/a.sock");
        connection.state_file("history").write("history").unwrap();

        let cycle_state = connection.state_file("cycle").read();

        assert_eq!(cycle_state, None);
    }

    #[test]
    fn a_connection_without_a_socket_still_has_state() {
        let dir = TempDir::new().unwrap();
        let connection = Connection::new(dir.path().to_path_buf(), None);

        connection.state_file("history").write("saved").unwrap();

        let contents = connection.state_file("history").read();

        assert_eq!(contents.as_deref(), Some("saved"));
    }
}
