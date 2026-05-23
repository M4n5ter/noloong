use ignore::{DirEntry, WalkBuilder};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

type BuildResult<T> = Result<T, Box<dyn std::error::Error>>;
const SOURCE_ARCHIVE_ZSTD_LEVEL: i32 = 3;
const EMBED_SOURCE_ARCHIVE_ENV: &str = "NOLOONG_EMBED_SOURCE_ARCHIVE";

#[derive(Debug)]
struct SourceFile {
    path: String,
    disk_path: PathBuf,
    size: u64,
    content_sha256: String,
    mode: u32,
}

#[derive(Debug)]
struct SourceArchive {
    compressed_bytes: Vec<u8>,
    uncompressed_bytes: usize,
    sha256: String,
    embedded: bool,
}

fn main() {
    if let Err(error) = run() {
        panic!("failed to generate build info: {error}");
    }
}

fn run() -> BuildResult<()> {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let embed_source_archive = should_embed_source_archive();

    emit_rerun_instructions(&root, embed_source_archive);

    let files = if embed_source_archive {
        collect_source_files(&root)?
    } else {
        Vec::new()
    };
    if embed_source_archive {
        emit_source_rerun_instructions(&files);
    }

    let source_archive = if embed_source_archive {
        build_source_archive(&files)?
    } else {
        SourceArchive::placeholder()
    };
    let manifest = build_manifest(&root, &files, &source_archive)?;

    fs::write(
        out_dir.join("source.tar.zst"),
        source_archive.compressed_bytes,
    )?;
    fs::write(out_dir.join("build-info.json"), manifest)?;

    Ok(())
}

fn should_embed_source_archive() -> bool {
    env::var_os("CARGO_FEATURE_EMBED_SOURCE_ARCHIVE").is_some()
        || env::var("PROFILE").is_ok_and(|profile| profile == "release")
        || env_flag(EMBED_SOURCE_ARCHIVE_ENV)
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn collect_source_files(root: &Path) -> BuildResult<Vec<SourceFile>> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false);

    let root_for_filter = root.to_path_buf();
    builder.filter_entry(move |entry| should_descend(entry, &root_for_filter));

    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = entry?;
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if relative.as_os_str().is_empty() || path_contains_git(relative) {
            continue;
        }
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_file() {
            continue;
        }
        let bytes = fs::read(path)?;
        let size = bytes.len() as u64;
        files.push(SourceFile {
            path: manifest_path(relative)?,
            disk_path: path.to_path_buf(),
            content_sha256: hash_bytes(&bytes),
            mode: file_mode(&metadata),
            size,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn should_descend(entry: &DirEntry, root: &Path) -> bool {
    entry
        .path()
        .strip_prefix(root)
        .map(|relative| !path_contains_git(relative))
        .unwrap_or(true)
}

fn path_contains_git(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::Normal(name) if name == ".git"))
}

fn manifest_path(path: &Path) -> BuildResult<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err(format!("invalid source path: {}", path.display()).into());
        };
        let part = part
            .to_str()
            .ok_or_else(|| format!("source path is not utf-8: {}", path.display()))?;
        parts.push(part);
    }
    if parts.is_empty() {
        return Err("empty source path".into());
    }
    Ok(parts.join("/"))
}

fn build_source_archive(files: &[SourceFile]) -> BuildResult<SourceArchive> {
    let encoder = zstd::Encoder::new(Vec::new(), SOURCE_ARCHIVE_ZSTD_LEVEL)?;
    let mut counting_writer = CountingWriter::new(encoder);
    {
        let mut builder = tar::Builder::new(&mut counting_writer);
        for file in files {
            let mut source = fs::File::open(&file.disk_path)?;
            let mut header = tar::Header::new_gnu();
            header.set_path(&file.path)?;
            header.set_size(file.size);
            header.set_mode(file.mode);
            header.set_mtime(0);
            header.set_uid(0);
            header.set_gid(0);
            header.set_cksum();
            builder.append(&header, &mut source)?;
        }
        builder.finish()?;
    }

    let uncompressed_bytes = counting_writer.bytes_written();
    let encoder = counting_writer.into_inner();
    let compressed_bytes = encoder.finish()?;
    let sha256 = hash_bytes(&compressed_bytes);

    Ok(SourceArchive {
        compressed_bytes,
        uncompressed_bytes,
        sha256,
        embedded: true,
    })
}

impl SourceArchive {
    fn placeholder() -> Self {
        let compressed_bytes = Vec::new();
        Self {
            sha256: hash_bytes(&compressed_bytes),
            compressed_bytes,
            uncompressed_bytes: 0,
            embedded: false,
        }
    }
}

fn build_manifest(
    root: &Path,
    files: &[SourceFile],
    source_archive: &SourceArchive,
) -> BuildResult<String> {
    let build = build_recipe();
    let package_name = env::var("CARGO_PKG_NAME")?;
    let manifest = json!({
        "schemaVersion": 1,
        "package": {
            "name": package_name.clone(),
            "version": env::var("CARGO_PKG_VERSION")?,
        },
        "workspace": {
            "rootPackage": package_name,
        },
        "git": git_info(root),
        "rust": {
            "rustc": command_output(env::var("RUSTC").unwrap_or_else(|_| "rustc".into()), ["--version"]),
        },
        "cargo": {
            "version": command_output(env::var("CARGO").unwrap_or_else(|_| "cargo".into()), ["--version"]),
        },
        "build": {
            "target": env::var("TARGET").ok(),
            "host": env::var("HOST").ok(),
            "profile": env::var("PROFILE").ok(),
            "features": enabled_features(),
            "command": build.command,
            "args": build.args,
        },
        "sourceArchive": {
            "format": "tar.zst",
            "embedded": source_archive.embedded,
            "sha256": source_archive.sha256,
            "compressedBytes": source_archive.compressed_bytes.len(),
            "uncompressedBytes": source_archive.uncompressed_bytes,
            "fileCount": files.len(),
        },
        "files": files.iter().map(|file| {
            json!({
                "path": file.path,
                "bytes": file.size,
                "sha256": file.content_sha256,
            })
        }).collect::<Vec<_>>(),
    });
    Ok(format!("{}\n", serde_json::to_string_pretty(&manifest)?))
}

struct BuildRecipe {
    command: String,
    args: Vec<String>,
}

fn build_recipe() -> BuildRecipe {
    let mut args = vec![
        "build".to_owned(),
        "-p".to_owned(),
        env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "noloong".into()),
        "--bin".to_owned(),
        env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "noloong".into()),
    ];

    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    match profile.as_str() {
        "debug" => {}
        "release" => args.push("--release".into()),
        other => {
            args.push("--profile".into());
            args.push(other.into());
        }
    }

    if let (Ok(target), Ok(host)) = (env::var("TARGET"), env::var("HOST"))
        && target != host
    {
        args.push("--target".into());
        args.push(target);
    }

    let features = enabled_features();
    if !features.is_empty() {
        args.push("--features".into());
        args.push(features.join(","));
    }

    let mut command_parts = vec!["cargo".to_owned()];
    command_parts.extend(args.clone());
    BuildRecipe {
        command: command_parts.join(" "),
        args,
    }
}

fn enabled_features() -> Vec<String> {
    let mut features = env::vars()
        .filter_map(|(key, _)| key.strip_prefix("CARGO_FEATURE_").map(normalize_feature))
        .collect::<Vec<_>>();
    features.sort();
    features
}

fn normalize_feature(raw: &str) -> String {
    raw.to_ascii_lowercase().replace('_', "-")
}

fn git_info(root: &Path) -> serde_json::Value {
    let commit = git_output(root, ["rev-parse", "HEAD"]);
    let status_output = git_output(root, ["status", "--porcelain", "--untracked-files=all"]);
    let dirty = status_output.as_ref().map(|status| !status.is_empty());
    let has_untracked = status_output
        .as_ref()
        .map(|status| status.lines().any(|line| line.starts_with("?? ")));
    let status = match dirty {
        Some(true) => "dirty",
        Some(false) => "clean",
        None => "unknown",
    };
    json!({
        "commit": commit,
        "dirty": dirty,
        "hasUntracked": has_untracked,
        "status": status,
    })
}

fn git_output<const N: usize>(root: &Path, args: [&str; N]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(stdout.trim().to_owned())
}

fn command_output<const N: usize>(command: String, args: [&str; N]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(stdout.trim().to_owned())
}

fn emit_rerun_instructions(root: &Path, embed_source_archive: bool) {
    println!("cargo:rerun-if-env-changed={EMBED_SOURCE_ARCHIVE_ENV}");
    println!("cargo:rerun-if-changed=.gitignore");
    println!("cargo:rerun-if-changed=.git/HEAD");
    if let Some(reference) = current_git_ref(root) {
        println!("cargo:rerun-if-changed={}", reference.display());
    }
    if !embed_source_archive {
        println!("cargo:rerun-if-changed=build.rs");
    }
}

fn emit_source_rerun_instructions(files: &[SourceFile]) {
    for file in files {
        println!("cargo:rerun-if-changed={}", file.path);
    }
}

fn current_git_ref(root: &Path) -> Option<PathBuf> {
    let head = fs::read_to_string(root.join(".git/HEAD")).ok()?;
    let reference = head.strip_prefix("ref: ")?.trim();
    Some(Path::new(".git").join(reference))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex(&hasher.finalize())
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn file_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            0o644
        } else {
            0o755
        }
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}

struct CountingWriter<W> {
    inner: W,
    bytes_written: usize,
}

impl<W> CountingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            bytes_written: 0,
        }
    }

    fn bytes_written(&self) -> usize {
        self.bytes_written
    }

    fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.bytes_written += written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
