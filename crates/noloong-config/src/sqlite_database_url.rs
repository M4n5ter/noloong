use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqliteDatabaseLocation {
    Memory,
    File(PathBuf),
}

impl SqliteDatabaseLocation {
    pub fn parse(database_url: impl AsRef<str>) -> Result<Self, SqliteDatabaseUrlError> {
        let database_url = database_url.as_ref();
        match database_url {
            "" => Err(SqliteDatabaseUrlError::EmptyUrl),
            ":memory:" => Ok(Self::Memory),
            url if url.starts_with("sqlite://") => {
                parse_sqlite_path(url.strip_prefix("sqlite://").unwrap_or_default())
            }
            url if url.starts_with("sqlite:") => {
                parse_sqlite_path(url.strip_prefix("sqlite:").unwrap_or_default())
            }
            url if url.contains("://") => Err(SqliteDatabaseUrlError::UnsupportedScheme(
                database_url.to_owned(),
            )),
            path => Ok(Self::File(PathBuf::from(path))),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Memory => None,
            Self::File(path) => Some(path.as_path()),
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SqliteDatabaseUrlError {
    #[error("sqlite database url is empty")]
    EmptyUrl,
    #[error("sqlite database path is empty")]
    EmptyPath,
    #[error("sqlite database URL must use sqlite, got: {0}")]
    UnsupportedScheme(String),
}

fn parse_sqlite_path(path: &str) -> Result<SqliteDatabaseLocation, SqliteDatabaseUrlError> {
    match path {
        "" => Err(SqliteDatabaseUrlError::EmptyPath),
        ":memory:" | "memory" => Ok(SqliteDatabaseLocation::Memory),
        path => Ok(SqliteDatabaseLocation::File(PathBuf::from(path))),
    }
}

#[cfg(test)]
mod tests {
    use super::{SqliteDatabaseLocation, SqliteDatabaseUrlError};
    use std::path::PathBuf;

    #[test]
    fn sqlite_database_url_parses_supported_locations() {
        for database_url in [":memory:", "sqlite::memory:", "sqlite://memory"] {
            assert_eq!(
                SqliteDatabaseLocation::parse(database_url).unwrap(),
                SqliteDatabaseLocation::Memory
            );
        }

        for database_url in [
            "sqlite:/tmp/noloong.sqlite",
            "sqlite:///tmp/noloong.sqlite",
            "/tmp/noloong.sqlite",
        ] {
            assert_eq!(
                SqliteDatabaseLocation::parse(database_url).unwrap(),
                SqliteDatabaseLocation::File(PathBuf::from("/tmp/noloong.sqlite"))
            );
        }

        assert_eq!(
            SqliteDatabaseLocation::parse("sqlite:relative/state.sqlite").unwrap(),
            SqliteDatabaseLocation::File(PathBuf::from("relative/state.sqlite"))
        );
    }

    #[test]
    fn sqlite_database_url_rejects_invalid_locations() {
        assert_eq!(
            SqliteDatabaseLocation::parse("").unwrap_err(),
            SqliteDatabaseUrlError::EmptyUrl
        );
        assert_eq!(
            SqliteDatabaseLocation::parse("sqlite:").unwrap_err(),
            SqliteDatabaseUrlError::EmptyPath
        );
        assert!(matches!(
            SqliteDatabaseLocation::parse("postgres://localhost/db").unwrap_err(),
            SqliteDatabaseUrlError::UnsupportedScheme(_)
        ));
    }
}
