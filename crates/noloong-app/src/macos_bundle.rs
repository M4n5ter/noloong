use crate::{AppError, AppLaunchOptions};
use std::{
    env, fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
};

const CHILD_ENV: &str = "NOLOONG_APP_BUNDLE_CHILD";
const BUNDLE_IDENTIFIER: &str = "dev.noloong.Noloong";
const APP_ICON_FILE: &str = "noloong-logo.icns";
const APP_ICON_BYTES: &[u8] = include_bytes!("../assets/noloong-logo.icns");

pub(crate) fn should_relaunch_in_bundle() -> bool {
    env::var_os(CHILD_ENV).is_none()
}

pub(crate) fn launch_in_bundle(options: AppLaunchOptions) -> Result<(), AppError> {
    let bundle = ensure_bundle(&env::current_exe()?)?;
    let mut command = Command::new("open");
    command.arg("-n").arg(&bundle);
    let args = bundle_args(&options);
    if !args.is_empty() {
        command.arg("--args").args(args);
    }
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Launch(format!(
            "`open {}` exited with {status}",
            bundle.display()
        )))
    }
}

fn ensure_bundle(executable: &Path) -> Result<PathBuf, AppError> {
    let bundle = bundle_path()?;
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;
    fs::write(contents.join("Info.plist"), info_plist())?;
    fs::write(resources.join(APP_ICON_FILE), APP_ICON_BYTES)?;

    let launcher = macos.join("noloong-app");
    fs::write(&launcher, launcher_script())?;
    let mut permissions = fs::metadata(&launcher)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&launcher, permissions)?;

    let linked_binary = macos.join("Noloong");
    remove_existing_path(&linked_binary)?;
    symlink(executable, linked_binary)?;
    remove_existing_path(&macos.join("noloong-bin"))?;
    Ok(bundle)
}

fn bundle_path() -> Result<PathBuf, AppError> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(AppError::MissingHome)?;
    Ok(home
        .join("Library")
        .join("Application Support")
        .join("Noloong")
        .join("Noloong.app"))
}

fn remove_existing_path(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::remove_dir_all(path)?;
            Ok(())
        }
        Ok(_) => {
            fs::remove_file(path)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn bundle_args(options: &AppLaunchOptions) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(path) = options.profile_config_path.as_deref() {
        args.push("--profile-config".to_string());
        args.push(path.to_string());
    }
    if let Some(locale) = options.locale {
        args.push("--locale".to_string());
        args.push(locale.code().to_string());
    }
    args
}

fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>noloong-app</string>
  <key>CFBundleIdentifier</key>
  <string>{BUNDLE_IDENTIFIER}</string>
  <key>CFBundleIconFile</key>
  <string>{APP_ICON_FILE}</string>
  <key>CFBundleName</key>
  <string>Noloong</string>
  <key>CFBundleDisplayName</key>
  <string>Noloong</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>{}</string>
  <key>CFBundleVersion</key>
  <string>{}</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>LSUIElement</key>
  <false/>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
"#,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION")
    )
}

fn launcher_script() -> &'static str {
    r#"#!/bin/sh
set -eu
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export NOLOONG_APP_BUNDLE_CHILD=1
exec "$script_dir/Noloong" app "$@"
"#
}

#[cfg(test)]
mod tests {
    use super::bundle_args;
    use crate::AppLaunchOptions;
    use noloong_agent::Locale;

    #[test]
    fn bundle_args_preserve_profile_config_and_locale() {
        assert_eq!(
            bundle_args(&AppLaunchOptions {
                profile_config_path: Some("/tmp/profile config.jsonc".to_string()),
                locale: Some(Locale::Zh),
            }),
            vec![
                "--profile-config",
                "/tmp/profile config.jsonc",
                "--locale",
                "zh",
            ]
        );
    }
}
