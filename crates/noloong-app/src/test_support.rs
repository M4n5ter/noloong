use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn temp_dir(name: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("noloong-{name}-{}-{timestamp}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

pub fn remove_temp_dir(path: impl AsRef<Path>) {
    fs::remove_dir_all(path).ok();
}
