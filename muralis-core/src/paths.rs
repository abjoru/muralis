use std::path::PathBuf;

use crate::error::{IoAt as _, MuralisError, Result};

#[derive(Debug, Clone)]
pub struct MuralisPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl MuralisPaths {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| MuralisError::Config("cannot resolve XDG config dir".into()))?
            .join("muralis");

        let data_dir = dirs::data_dir()
            .ok_or_else(|| MuralisError::Config("cannot resolve XDG data dir".into()))?
            .join("muralis");

        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| MuralisError::Config("cannot resolve XDG cache dir".into()))?
            .join("muralis");

        Ok(Self {
            config_dir,
            data_dir,
            cache_dir,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("muralis.db")
    }

    pub fn wallpapers_dir(&self) -> PathBuf {
        self.data_dir.join("wallpapers")
    }

    pub fn thumbnails_dir(&self) -> PathBuf {
        self.cache_dir.join("thumbnails")
    }

    pub fn socket_path() -> PathBuf {
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/muralis-{uid}.sock"))
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            &self.config_dir,
            &self.data_dir,
            &self.cache_dir,
            &self.wallpapers_dir(),
            &self.thumbnails_dir(),
        ] {
            std::fs::create_dir_all(dir).at("create directory", dir)?;
        }
        Ok(())
    }

    /// Install app icon to XDG icon dir if missing/outdated.
    pub fn install_icon(&self) -> Result<()> {
        let icon_dir = self
            .data_dir
            .parent()
            .unwrap_or(&self.data_dir)
            .join("icons/hicolor/scalable/apps");
        std::fs::create_dir_all(&icon_dir).at("create directory", &icon_dir)?;
        let dest = icon_dir.join("muralis.svg");
        let svg = include_bytes!("../../assets/muralis.svg");
        if dest.exists() {
            if let Ok(existing) = std::fs::read(&dest) {
                if existing == svg.as_slice() {
                    return Ok(());
                }
            }
        }
        std::fs::write(&dest, svg).at("write", &dest)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway XDG root with nothing under it: the machine muralis has
    /// never run on.
    fn bare_root() -> (MuralisPaths, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = MuralisPaths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        (paths, tmp)
    }

    /// The CLI and the daemon may start at once, and neither may fail
    /// because the other got there first — nor may a second run of either.
    #[test]
    fn ensuring_concurrently_from_a_clean_root_leaves_every_caller_succeeding() {
        let (paths, tmp) = bare_root();

        let outcomes: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let paths = paths.clone();
                    scope.spawn(move || paths.ensure_dirs())
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        for outcome in &outcomes {
            assert!(
                outcome.is_ok(),
                "losing the race is not a failure: {outcome:?}"
            );
        }
        for dir in [
            &paths.config_dir,
            &paths.data_dir,
            &paths.cache_dir,
            &paths.wallpapers_dir(),
            &paths.thumbnails_dir(),
        ] {
            assert!(dir.is_dir(), "{} was not created", dir.display());
        }

        // Nothing muralis does not own: the three XDG roots, and nothing
        // beside them.
        let mut created: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        created.sort();
        assert_eq!(created, ["cache", "config", "data"]);
    }

    /// A read-only parent is the real-world case, but a plain file as the
    /// parent reproduces the same class of failure for any user, root
    /// included — and root is how this runs in a container.
    #[test]
    fn a_directory_that_cannot_be_created_names_itself_and_why() {
        let tmp = tempfile::tempdir().unwrap();
        let blocked = tmp.path().join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        let paths = MuralisPaths {
            config_dir: blocked.join("config"),
            data_dir: blocked.join("data"),
            cache_dir: blocked.join("cache"),
        };

        let err = paths
            .ensure_dirs()
            .expect_err("nothing can be created under a file");

        let message = err.to_string();
        assert!(
            message.contains(&blocked.display().to_string()),
            "the path it could not create has to be in the message: {message}"
        );
        assert!(
            message.contains("Not a directory"),
            "and why it could not: {message}"
        );
    }
}
