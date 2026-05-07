use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn write_temp_file(name: &str, extension: &str, text: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "noloong-{name}-{}-{timestamp}.{extension}",
        std::process::id()
    ));
    fs::write(&path, text).unwrap();
    path
}

pub fn remove_temp_file(path: impl AsRef<Path>) {
    fs::remove_file(path).ok();
}
