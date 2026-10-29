use std::fs;
use std::io;
use std::path::PathBuf;

pub struct StateFile {
    path: PathBuf,
}

impl StateFile {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn read(&self) -> Option<String> {
        fs::read_to_string(&self.path).ok()
    }

    pub fn write(&self, contents: &str) -> io::Result<()> {
        self.create_parent_dir()?;
        let unfinished = self.path.with_extension("tmp");
        fs::write(&unfinished, contents)?;
        if let Ok(existing) = fs::metadata(&self.path) {
            fs::set_permissions(&unfinished, existing.permissions())?;
        }
        fs::rename(&unfinished, &self.path)
    }

    pub fn remove(&self) {
        let _ = fs::remove_file(&self.path);
    }

    fn create_parent_dir(&self) -> io::Result<()> {
        match self.path.parent() {
            Some(parent) => fs::create_dir_all(parent),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn state_file_in(dir: &TempDir) -> StateFile {
        StateFile::at(dir.path().join("nested").join("state.json"))
    }

    #[test]
    fn written_contents_read_back() {
        let dir = TempDir::new().unwrap();
        let file = state_file_in(&dir);
        file.write("hello").unwrap();

        let contents = file.read();

        assert_eq!(contents.as_deref(), Some("hello"));
    }

    #[test]
    fn a_file_never_written_reads_as_nothing() {
        let dir = TempDir::new().unwrap();
        let file = state_file_in(&dir);

        let contents = file.read();

        assert_eq!(contents, None);
    }

    #[test]
    fn a_removed_file_reads_as_nothing() {
        let dir = TempDir::new().unwrap();
        let file = state_file_in(&dir);
        file.write("hello").unwrap();
        file.remove();

        let contents = file.read();

        assert_eq!(contents, None);
    }

    #[test]
    fn a_second_write_replaces_the_first() {
        let dir = TempDir::new().unwrap();
        let file = state_file_in(&dir);
        file.write("first").unwrap();
        file.write("second").unwrap();

        let contents = file.read();

        assert_eq!(contents.as_deref(), Some("second"));
    }

    #[test]
    fn rewriting_a_file_keeps_its_permissions() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("private.toml");
        fs::write(&path, "first").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        StateFile::at(path.clone()).write("second").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        assert_eq!(mode, 0o600);
    }
}
