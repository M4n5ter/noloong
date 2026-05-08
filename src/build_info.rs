use serde::Deserialize;
use std::{
    fs,
    io::{self, Cursor, Read},
    path::{Component, Path, PathBuf},
};
use thiserror::Error;

const BUILD_INFO_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/build-info.json"));
const SOURCE_ARCHIVE_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/source.tar.zst"));
type SourceArchiveReader =
    tar::Archive<zstd::stream::read::Decoder<'static, io::BufReader<Cursor<&'static [u8]>>>>;

pub fn manifest_json() -> &'static str {
    BUILD_INFO_JSON
}

pub fn build_command() -> Result<String, BuildInfoError> {
    Ok(manifest()?.build.command)
}

pub fn source_paths() -> Result<Vec<String>, BuildInfoError> {
    let mut paths = manifest()?
        .files
        .into_iter()
        .map(|file| validate_archive_path(Path::new(&file.path)))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

pub fn source_file(path: &str) -> Result<Vec<u8>, BuildInfoError> {
    let requested = validate_requested_path(path)?;
    let mut archive = source_archive()?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = validated_entry_path(&entry)?;
        if path == requested {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            return Ok(bytes);
        }
    }
    Err(BuildInfoError::SourceNotFound(requested))
}

pub fn write_archive(path: &Path) -> Result<(), BuildInfoError> {
    if path.is_dir() {
        return Err(BuildInfoError::OutputIsDirectory(path.into()));
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, SOURCE_ARCHIVE_BYTES)?;
    Ok(())
}

pub fn extract_source(output_dir: &Path, force: bool) -> Result<(), BuildInfoError> {
    if output_dir.exists() && !output_dir.is_dir() {
        return Err(BuildInfoError::OutputIsFile(output_dir.into()));
    }
    if output_dir.exists() && !force && directory_has_entries(output_dir)? {
        return Err(BuildInfoError::OutputDirectoryNotEmpty(output_dir.into()));
    }
    fs::create_dir_all(output_dir)?;

    let mut archive = source_archive()?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = validated_entry_path(&entry)?;
        let destination = output_dir.join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(destination)?;
        io::copy(&mut entry, &mut output)?;
    }
    Ok(())
}

fn manifest() -> Result<BuildInfoManifest, BuildInfoError> {
    serde_json::from_str(BUILD_INFO_JSON)
        .map_err(|error| BuildInfoError::ManifestInvalid(error.to_string()))
}

fn source_archive() -> Result<SourceArchiveReader, BuildInfoError> {
    let decoder = zstd::stream::read::Decoder::new(Cursor::new(SOURCE_ARCHIVE_BYTES))?;
    Ok(tar::Archive::new(decoder))
}

fn validated_entry_path<R: Read>(entry: &tar::Entry<'_, R>) -> Result<String, BuildInfoError> {
    let path = entry.path()?;
    if !entry.header().entry_type().is_file() {
        return Err(BuildInfoError::ArchiveEntry(format!(
            "non-file entry: {}",
            path.display()
        )));
    }
    validate_archive_path(path.as_ref())
}

fn validate_requested_path(path: &str) -> Result<String, BuildInfoError> {
    validate_relative_path(Path::new(path), "requested source path")
}

fn validate_archive_path(path: &Path) -> Result<String, BuildInfoError> {
    validate_relative_path(path, "archive source path")
}

fn validate_relative_path(path: &Path, label: &'static str) -> Result<String, BuildInfoError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(BuildInfoError::InvalidPath {
            label,
            path: path.into(),
        });
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| BuildInfoError::InvalidPath {
                    label,
                    path: path.into(),
                })?;
                parts.push(part);
            }
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => {
                return Err(BuildInfoError::InvalidPath {
                    label,
                    path: path.into(),
                });
            }
        }
    }
    if parts.is_empty() {
        return Err(BuildInfoError::InvalidPath {
            label,
            path: path.into(),
        });
    }
    Ok(parts.join("/"))
}

fn directory_has_entries(path: &Path) -> io::Result<bool> {
    if let Some(entry) = fs::read_dir(path)?.next() {
        entry?;
        return Ok(true);
    }
    Ok(false)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildInfoManifest {
    build: BuildManifest,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Deserialize)]
struct BuildManifest {
    command: String,
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    path: String,
}

#[derive(Debug, Error)]
pub enum BuildInfoError {
    #[error("build info manifest is invalid: {0}")]
    ManifestInvalid(String),
    #[error("{label} is invalid: {}", path.display())]
    InvalidPath { label: &'static str, path: PathBuf },
    #[error("source path not found in embedded snapshot: {0}")]
    SourceNotFound(String),
    #[error("source archive entry is invalid: {0}")]
    ArchiveEntry(String),
    #[error("output directory is not empty: {}", .0.display())]
    OutputDirectoryNotEmpty(PathBuf),
    #[error("output path is a directory: {}", .0.display())]
    OutputIsDirectory(PathBuf),
    #[error("output directory path is a file: {}", .0.display())]
    OutputIsFile(PathBuf),
    #[error("I/O failed: {0}")]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use super::{
        BuildInfoError, build_command, extract_source, manifest_json, source_file, source_paths,
        validate_archive_path, write_archive,
    };
    use crate::test_support::{remove_temp_dir, remove_temp_file, temp_dir};
    use std::path::Path;

    #[test]
    fn build_info_manifest_is_json_v1() {
        let value: serde_json::Value = serde_json::from_str(manifest_json()).unwrap();

        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["sourceArchive"]["format"], "tar.zst");
    }

    #[test]
    fn build_info_command_is_normalized_cargo_build() {
        let command = build_command().unwrap();

        assert!(command.starts_with("cargo build -p noloong --bin noloong"));
    }

    #[test]
    fn build_info_source_paths_include_repository_context() {
        let paths = source_paths().unwrap();

        assert!(paths.iter().any(|path| path == "Cargo.toml"));
        assert!(paths.iter().any(|path| path == ".github/workflows/ci.yml"));
        assert!(
            paths
                .iter()
                .any(|path| path == "crates/noloong-agent-core/src/lib.rs")
        );
    }

    #[test]
    fn build_info_source_paths_exclude_private_and_generated_paths() {
        let paths = source_paths().unwrap();

        assert!(
            !paths
                .iter()
                .any(|path| path == ".git" || path.starts_with(".git/"))
        );
        assert!(
            !paths
                .iter()
                .any(|path| path == "target" || path.starts_with("target/"))
        );
        assert!(
            !paths
                .iter()
                .any(|path| path == ".env" || path.starts_with(".env"))
        );
        assert!(!paths.iter().any(|path| {
            path.ends_with(".sqlite")
                || path.contains(".sqlite-")
                || path.ends_with(".db")
                || path.ends_with(".pem")
                || path.ends_with(".key")
                || path.ends_with(".log")
        }));
    }

    #[test]
    fn build_info_source_file_reads_embedded_file() {
        let bytes = source_file("Cargo.toml").unwrap();
        let text = String::from_utf8(bytes).unwrap();

        assert!(text.contains("[workspace]"));
    }

    #[test]
    fn build_info_source_file_rejects_unsafe_paths() {
        for path in ["", "/Cargo.toml", "../Cargo.toml", "src/../Cargo.toml"] {
            assert!(matches!(
                source_file(path),
                Err(BuildInfoError::InvalidPath { .. })
            ));
        }
    }

    #[test]
    fn build_info_source_file_reports_missing_paths() {
        let error = source_file("missing.txt").unwrap_err();

        assert!(matches!(error, BuildInfoError::SourceNotFound(_)));
    }

    #[test]
    fn build_info_archive_path_validator_rejects_traversal() {
        for path in ["/Cargo.toml", "../Cargo.toml", "src/../Cargo.toml"] {
            assert!(matches!(
                validate_archive_path(Path::new(path)),
                Err(BuildInfoError::InvalidPath { .. })
            ));
        }
    }

    #[test]
    fn build_info_extract_rejects_non_empty_directory_without_force() {
        let dir = temp_dir("build-info-extract-non-empty");
        std::fs::write(dir.join("existing.txt"), "existing").unwrap();

        let error = extract_source(&dir, false).unwrap_err();
        remove_temp_dir(dir);

        assert!(matches!(error, BuildInfoError::OutputDirectoryNotEmpty(_)));
    }

    #[test]
    fn build_info_extract_allows_non_empty_directory_with_force() {
        let dir = temp_dir("build-info-extract-force");
        std::fs::write(dir.join("existing.txt"), "existing").unwrap();

        extract_source(&dir, true).unwrap();
        let cargo_toml = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        remove_temp_dir(dir);

        assert!(cargo_toml.contains("[workspace]"));
    }

    #[test]
    fn build_info_extract_writes_snapshot_files() {
        let dir = temp_dir("build-info-extract");

        extract_source(&dir, false).unwrap();
        let cargo_toml = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        remove_temp_dir(dir);

        assert!(cargo_toml.contains("[workspace]"));
    }

    #[test]
    fn build_info_write_archive_rejects_directory_output() {
        let dir = temp_dir("build-info-archive-dir");

        let error = write_archive(&dir).unwrap_err();
        remove_temp_dir(dir);

        assert!(matches!(error, BuildInfoError::OutputIsDirectory(_)));
    }

    #[test]
    fn build_info_write_archive_writes_embedded_archive() {
        let dir = temp_dir("build-info-archive-output");
        let path = dir.join("source.tar.zst");

        write_archive(&path).unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        remove_temp_file(path);
        remove_temp_dir(dir);

        assert!(metadata.len() > 0);
    }
}
