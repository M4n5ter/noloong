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
    if is_bundle_running(&bundle) {
        clear_launch_options();
        activate_bundle()?;
        return Ok(());
    }
    write_launch_options(&options)?;
    let status = Command::new("open").arg(&bundle).status()?;
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

fn bundle_executable(bundle: &Path) -> PathBuf {
    bundle.join("Contents").join("MacOS").join("Noloong")
}

fn bundle_path() -> Result<PathBuf, AppError> {
    Ok(app_support_dir()?.join("Noloong.app"))
}

fn app_support_dir() -> Result<PathBuf, AppError> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(AppError::MissingHome)?;
    Ok(home
        .join("Library")
        .join("Application Support")
        .join("Noloong"))
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

fn write_launch_options(options: &AppLaunchOptions) -> Result<(), AppError> {
    let path = launch_options_path()?;
    if options.profile_config_path.is_none() && options.locale.is_none() {
        let _ = fs::remove_file(path);
        return Ok(());
    }
    fs::write(path, launch_options_text(options))?;
    Ok(())
}

fn launch_options_text(options: &AppLaunchOptions) -> String {
    let profile_config = options
        .profile_config_path
        .as_deref()
        .map(absolute_profile_config_arg)
        .unwrap_or_default();
    let locale = options
        .locale
        .map(|locale| locale.code())
        .unwrap_or_default();
    format!("{profile_config}\n{locale}\n")
}

fn clear_launch_options() {
    if let Ok(path) = launch_options_path() {
        let _ = fs::remove_file(path);
    }
}

fn launch_options_path() -> Result<PathBuf, AppError> {
    Ok(app_support_dir()?.join("launch-options.txt"))
}

fn absolute_profile_config_arg(path: &str) -> String {
    let path = Path::new(path);
    if path.is_absolute() {
        return path.display().to_string();
    }
    env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
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
launch_options="${HOME:-}/Library/Application Support/Noloong/launch-options.txt"
if [ -n "${HOME:-}" ] && [ -f "$launch_options" ]; then
  profile_config=$(sed -n '1p' "$launch_options")
  locale=$(sed -n '2p' "$launch_options")
  rm -f "$launch_options"
  if [ -n "$profile_config" ]; then
    set -- --profile-config "$profile_config" "$@"
  fi
  if [ -n "$locale" ]; then
    set -- "$@" --locale "$locale"
  fi
fi
exec "$script_dir/Noloong" app "$@"
"#
}

fn is_bundle_running(bundle: &Path) -> bool {
    let executable = bundle_executable(bundle).display().to_string();
    let pattern = format!("^{} app( |$)", regex_escape_literal(&executable));
    Command::new("pgrep")
        .args(["-f", pattern.as_str()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn regex_escape_literal(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '\\' | '.' | '+' | '*' | '?' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' => {
                ['\\', ch]
            }
            _ => ['\0', ch],
        })
        .filter(|ch| *ch != '\0')
        .collect()
}

fn activate_bundle() -> Result<(), AppError> {
    let script = format!("tell application id \"{BUNDLE_IDENTIFIER}\" to activate");
    let status = Command::new("osascript")
        .args(["-e", script.as_str()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Launch(format!(
            "failed to activate {BUNDLE_IDENTIFIER}: {status}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{absolute_profile_config_arg, launch_options_text};
    use crate::AppLaunchOptions;
    use noloong_config::Locale;
    use std::path::Path;

    #[test]
    fn launch_options_preserve_profile_config_and_locale() {
        let text = launch_options_text(&AppLaunchOptions {
            profile_config_path: Some("/tmp/profile config.jsonc".to_string()),
            locale: Some(Locale::Zh),
            interaction_endpoint: None,
        });

        assert_eq!(text, "/tmp/profile config.jsonc\nzh\n");
    }

    #[test]
    fn bundle_args_absolutize_relative_profile_config() {
        let path = absolute_profile_config_arg("examples/profile.jsonc");

        assert!(Path::new(&path).is_absolute(), "{path}");
        assert!(path.ends_with("examples/profile.jsonc"), "{path}");
    }
}
