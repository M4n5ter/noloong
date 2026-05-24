use crate::{APP_ID, APP_NAME, AppError, AppInteractionEndpoint, AppLaunchOptions};
use noloong_config::Locale;
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

const APP_ICON_FILE: &str = "noloong-logo.icns";
const APP_ICON_BYTES: &[u8] = include_bytes!("../assets/noloong-logo.icns");

pub(crate) fn should_relaunch_in_bundle() -> bool {
    !is_running_from_bundle()
}

pub fn take_bundle_launch_options() -> Result<Option<AppLaunchOptions>, AppError> {
    if !is_running_from_bundle() {
        return Ok(None);
    }
    read_launch_options().map(|options| Some(options.unwrap_or_default()))
}

pub(crate) fn launch_in_bundle(options: AppLaunchOptions) -> Result<(), AppError> {
    let bundle = ensure_bundle(&env::current_exe()?)?;
    if is_bundle_running(&bundle) {
        clear_launch_options();
        activate_bundle()?;
        return Ok(());
    }
    write_launch_options(&options)?;
    let status = Command::new("open").arg("-W").arg(&bundle).status()?;
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

    let linked_binary = macos.join("Noloong");
    remove_existing_path(&linked_binary)?;
    fs::copy(executable, &linked_binary)?;
    let mut permissions = fs::metadata(&linked_binary)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&linked_binary, permissions)?;
    remove_existing_path(&macos.join("noloong-app"))?;
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
    if options.profile_config_path.is_none()
        && options.locale.is_none()
        && options.interaction_endpoint.is_none()
    {
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
    let interaction_ws_url = options
        .interaction_endpoint
        .as_ref()
        .map(|endpoint| endpoint.ws_url.as_str())
        .unwrap_or_default();
    let interaction_token = options
        .interaction_endpoint
        .as_ref()
        .and_then(|endpoint| endpoint.bearer_token.as_deref())
        .unwrap_or_default();
    format!("{profile_config}\n{locale}\n{interaction_ws_url}\n{interaction_token}\n")
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
  <string>{APP_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>{APP_ID}</string>
  <key>CFBundleIconFile</key>
  <string>{APP_ICON_FILE}</string>
  <key>CFBundleName</key>
  <string>{APP_NAME}</string>
  <key>CFBundleDisplayName</key>
  <string>{APP_NAME}</string>
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

fn is_bundle_running(bundle: &Path) -> bool {
    let pattern = bundle_process_pattern(bundle);
    Command::new("pgrep")
        .args(["-f", pattern.as_str()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn bundle_process_pattern(bundle: &Path) -> String {
    let executable = bundle_executable(bundle).display().to_string();
    format!("^{}( app)?( |$)", regex_escape_literal(&executable))
}

fn is_running_from_bundle() -> bool {
    env::current_exe()
        .ok()
        .as_deref()
        .is_some_and(is_path_inside_app_bundle)
        || env::args_os()
            .next()
            .map(PathBuf::from)
            .as_deref()
            .is_some_and(is_path_inside_app_bundle)
}

fn is_path_inside_app_bundle(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        ancestor
            .extension()
            .is_some_and(|extension| extension == "app")
    })
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

fn read_launch_options() -> Result<Option<AppLaunchOptions>, AppError> {
    let path = launch_options_path()?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let _ = fs::remove_file(path);
    Ok(Some(parse_launch_options_text(&text)))
}

fn parse_launch_options_text(text: &str) -> AppLaunchOptions {
    let mut lines = text.lines();
    let profile_config_path = non_empty_line(lines.next());
    let locale = non_empty_line(lines.next()).and_then(|value| Locale::parse(&value));
    let interaction_ws_url = non_empty_line(lines.next());
    let interaction_token = non_empty_line(lines.next());
    AppLaunchOptions {
        profile_config_path,
        locale,
        interaction_endpoint: interaction_ws_url.map(|ws_url| AppInteractionEndpoint {
            ws_url,
            bearer_token: interaction_token,
        }),
        interaction_status: None,
    }
}

fn non_empty_line(line: Option<&str>) -> Option<String> {
    line.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn activate_bundle() -> Result<(), AppError> {
    let script = format!("tell application id \"{APP_ID}\" to activate");
    let status = Command::new("osascript")
        .args(["-e", script.as_str()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Launch(format!(
            "failed to activate {APP_ID}: {status}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        absolute_profile_config_arg, bundle_process_pattern, info_plist, launch_options_text,
        parse_launch_options_text,
    };
    use crate::{AppInteractionEndpoint, AppLaunchOptions};
    use noloong_config::Locale;
    use std::path::Path;

    #[test]
    fn launch_options_preserve_profile_config_and_locale() {
        let text = launch_options_text(&AppLaunchOptions {
            profile_config_path: Some("/tmp/profile config.jsonc".to_string()),
            locale: Some(Locale::Zh),
            interaction_endpoint: None,
            interaction_status: None,
        });

        assert_eq!(text, "/tmp/profile config.jsonc\nzh\n\n\n");
    }

    #[test]
    fn bundle_plist_uses_the_real_app_binary_as_executable() {
        let plist = info_plist();

        assert!(plist.contains("<key>CFBundleExecutable</key>\n  <string>Noloong</string>"));
        assert!(!plist.contains("<string>noloong-app</string>"));
    }

    #[test]
    fn bundle_process_pattern_accepts_direct_bundle_binary() {
        let pattern = bundle_process_pattern(Path::new(
            "/Users/example/Library/Application Support/Noloong/Noloong.app",
        ));

        assert!(pattern.contains("Contents/MacOS/Noloong"));
        assert!(pattern.contains("( app)?"));
    }

    #[test]
    fn launch_options_preserve_interaction_endpoint() {
        let text = launch_options_text(&AppLaunchOptions {
            profile_config_path: Some("/tmp/profile.jsonc".to_string()),
            locale: Some(Locale::En),
            interaction_endpoint: Some(AppInteractionEndpoint {
                ws_url: "ws://127.0.0.1:9876/jsonrpc/ws".into(),
                bearer_token: Some("secret token".into()),
            }),
            interaction_status: None,
        });

        assert_eq!(
            text,
            "/tmp/profile.jsonc\nen\nws://127.0.0.1:9876/jsonrpc/ws\nsecret token\n"
        );
    }

    #[test]
    fn launch_options_parse_back_into_app_options() {
        let options = parse_launch_options_text(
            "/tmp/profile.jsonc\nzh\nws://127.0.0.1:9876/jsonrpc/ws\nsecret token\n",
        );

        assert_eq!(
            options.profile_config_path.as_deref(),
            Some("/tmp/profile.jsonc")
        );
        assert_eq!(options.locale, Some(Locale::Zh));
        assert_eq!(
            options
                .interaction_endpoint
                .as_ref()
                .map(|endpoint| endpoint.ws_url.as_str()),
            Some("ws://127.0.0.1:9876/jsonrpc/ws")
        );
        assert_eq!(
            options
                .interaction_endpoint
                .as_ref()
                .and_then(|endpoint| endpoint.bearer_token.as_deref()),
            Some("secret token")
        );
    }

    #[test]
    fn bundle_args_absolutize_relative_profile_config() {
        let path = absolute_profile_config_arg("examples/profile.jsonc");

        assert!(Path::new(&path).is_absolute(), "{path}");
        assert!(path.ends_with("examples/profile.jsonc"), "{path}");
    }
}
