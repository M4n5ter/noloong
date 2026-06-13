use image::GenericImageView;
use noloong_app::{
    AppInteractionEndpoint, AppInteractionStatus, AppLaunchOptions, AppRuntimeControlEndpoint,
    interaction::InteractionProfileDescriptor,
};
use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn tauri_shell_declares_noloong_app_identity() {
    let config = tauri_config();

    assert_eq!(config["productName"], "Noloong");
    assert_eq!(config["identifier"], "com.noloong.desktop");
    assert_eq!(
        config["build"]["beforeDevCommand"],
        "bun --cwd ../apps/desktop dev"
    );
    assert_eq!(
        config["build"]["beforeBuildCommand"],
        "bun --cwd ../apps/desktop build"
    );
    assert_eq!(config["build"]["devUrl"], "http://127.0.0.1:5173");
    assert_eq!(config["build"]["frontendDist"], "../../apps/desktop/dist");

    let window = config["app"]["windows"]
        .as_array()
        .and_then(|windows| windows.first())
        .expect("main window config");

    assert_eq!(window["title"], "Noloong");
    assert_eq!(window["backgroundThrottling"], "disabled");
    assert_eq!(window["backgroundColor"], "#F7F3EA");
    assert_eq!(window["decorations"], true);
    assert!(window["titleBarStyle"].is_null());
    assert!(window["hiddenTitle"].is_null());
    assert!(window["trafficLightPosition"].is_null());
    assert_eq!(window["closable"], true);
    assert_eq!(window["minimizable"], true);
    assert_eq!(window["maximizable"], true);
    assert_eq!(window["resizable"], true);
}

#[test]
fn window_capabilities_stay_scoped_to_their_surfaces() {
    let capability = read_json(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capabilities")
            .join("main.json"),
    );

    assert_eq!(capability["identifier"], "main-window");
    assert_eq!(capability["windows"][0], "main");
    let permissions = capability["permissions"]
        .as_array()
        .expect("main window capability permissions");
    assert!(permissions.iter().any(|value| value == "core:default"));
    assert!(
        permissions
            .iter()
            .any(|value| value == "core:window:allow-start-dragging"),
        "Tauri title bar drag regions require the window start_dragging permission",
    );
    assert!(
        permissions.iter().any(|value| value == "dialog:allow-open"),
        "File attachments require only the Tauri dialog open permission",
    );
    assert!(
        !permissions.iter().any(|value| value == "dialog:default"),
        "File attachments should not grant broader save/message dialog permissions",
    );
    assert!(
        !permissions
            .iter()
            .any(|value| value == "core:window:allow-set-title"),
        "Only the settings window should be able to set its title",
    );

    let settings_capability = read_json(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capabilities")
            .join("settings.json"),
    );
    assert_eq!(settings_capability["identifier"], "settings-window");
    assert_eq!(settings_capability["windows"][0], "settings");
    let settings_permissions = settings_capability["permissions"]
        .as_array()
        .expect("settings window capability permissions");
    assert!(
        settings_permissions
            .iter()
            .any(|value| value == "core:window:allow-set-title"),
        "Settings pane changes update the native settings window title",
    );
    assert!(
        settings_permissions
            .iter()
            .any(|value| value == "core:event:allow-emit-to"),
        "Settings save/apply must notify the main chat window about runtime restarts",
    );
    assert!(
        !settings_permissions
            .iter()
            .any(|value| value == "dialog:allow-open"),
        "Settings does not need file attachment dialog permissions",
    );
}

#[test]
fn bun_workspace_declares_desktop_frontend_entrypoints() {
    let root = repository_root();
    let package = read_json(root.join("package.json"));
    let desktop = read_json(root.join("apps/desktop/package.json"));

    assert_eq!(package["private"], true);
    assert_eq!(
        package["scripts"]["desktop:dev"],
        "bun --cwd apps/desktop dev"
    );
    assert_eq!(
        package["scripts"]["desktop:build"],
        "bun --cwd apps/desktop build"
    );
    assert_eq!(
        package["scripts"]["app:dev"],
        "cd crates/noloong-app && ../../node_modules/.bin/tauri dev"
    );
    assert_eq!(
        package["scripts"]["app:bundle"],
        "cd crates/noloong-app && ../../node_modules/.bin/tauri build --bundles app --no-sign"
    );
    assert_eq!(package["devDependencies"]["@tauri-apps/cli"], "^2.11.2");

    assert_eq!(desktop["name"], "@noloong/desktop");
    assert_eq!(desktop["private"], true);
    assert!(desktop["dependencies"]["@vitejs/plugin-react"].is_null());
    assert_eq!(desktop["dependencies"]["@tauri-apps/api"], "^2.11.0");
    assert_eq!(
        desktop["dependencies"]["@tauri-apps/plugin-dialog"],
        "2.7.1"
    );
    assert_eq!(
        desktop["dependencies"]["@fontsource-variable/inter-tight"],
        "5.2.7"
    );
    assert_eq!(desktop["dependencies"]["lucide-react"], "1.17.0");
    assert_eq!(desktop["dependencies"]["react"], "^19.2.0");
    assert_eq!(desktop["dependencies"]["react-dom"], "^19.2.0");
    assert_eq!(desktop["devDependencies"]["vite"], "^8.0.0");
    assert_eq!(desktop["devDependencies"]["typescript"], "^6.0.0");
}

#[test]
fn app_crate_uses_tauri_shell_without_gpui_runtime() {
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).unwrap_or_else(|error| {
        panic!("{} should exist: {error}", manifest_path.display());
    });

    assert!(manifest.contains("tauri.workspace = true"));
    assert!(manifest.contains("tauri-build.workspace = true"));
    assert!(!manifest.contains("gpui.workspace = true"));
    assert!(!manifest.contains("gpui_platform.workspace = true"));
    assert!(!manifest.contains("gpui-component.workspace = true"));
}

#[test]
fn app_crate_declares_noloong_macos_binary_target() {
    let metadata = cargo_metadata();
    let package = metadata["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|package| package["name"] == "noloong-app")
        })
        .expect("noloong-app package should be present in cargo metadata");
    let bin_targets: Vec<&Value> = package["targets"]
        .as_array()
        .expect("package should declare targets")
        .iter()
        .filter(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .collect();

    assert_eq!(
        bin_targets.len(),
        1,
        "noloong-app should expose one app binary target",
    );
    assert_eq!(bin_targets[0]["name"], "Noloong");
    assert!(
        bin_targets[0]["src_path"]
            .as_str()
            .is_some_and(|path| path.ends_with("/crates/noloong-app/src/main.rs")),
        "Noloong binary target should use src/main.rs",
    );
}

#[test]
fn macos_conversation_menu_declares_discoverable_commands() {
    let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("runtime.rs");
    let runtime = fs::read_to_string(&runtime_path).unwrap_or_else(|error| {
        panic!("{} should exist: {error}", runtime_path.display());
    });

    for required in [
        r#"const FOCUS_COMPOSER_MENU_ID: &str = "focus-composer";"#,
        r#"const SEND_MESSAGE_MENU_ID: &str = "send-message";"#,
        r#"const STOP_RESPONSE_MENU_ID: &str = "stop-response";"#,
        r#"const CONVERSATION_MENU_COMMAND_EVENT: &str = "noloong-conversation-menu-command";"#,
        r#".accelerator("CmdOrCtrl+L")"#,
        r#".accelerator("CmdOrCtrl+Enter")"#,
        r#".accelerator("Esc")"#,
        r#".enabled(false)"#,
        "app_update_conversation_menu_state",
        "set_menu_item_enabled(app, SEND_MESSAGE_MENU_ID, state.can_send_message)",
        "set_menu_item_enabled(app, STOP_RESPONSE_MENU_ID, state.can_stop_response)",
    ] {
        assert!(
            runtime.contains(required),
            "runtime.rs should keep the macOS Conversation menu contract: {required}",
        );
    }

    assert!(
        runtime
            .find("&view_submenu")
            .expect("View menu should exist")
            < runtime
                .find("&conversation_submenu")
                .expect("Conversation menu should exist"),
        "Conversation menu should be after View",
    );
    assert!(
        runtime
            .find("&conversation_submenu")
            .expect("Conversation menu should exist")
            < runtime
                .find("&window_submenu")
                .expect("Window menu should exist"),
        "Conversation menu should be before Window",
    );
}

#[test]
fn workspace_no_longer_declares_gpui_dependencies_or_dev_profile_overrides() {
    let manifest_path = repository_root().join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).unwrap_or_else(|error| {
        panic!("{} should exist: {error}", manifest_path.display());
    });

    for stale_entry in [
        "gpui =",
        "gpui_platform =",
        "gpui-component =",
        "gpui_macros",
        "gpui-component-macros",
    ] {
        assert!(
            !manifest.contains(stale_entry),
            "workspace Cargo.toml should not contain stale GPUI entry {stale_entry:?}",
        );
    }
}

#[test]
fn stale_gpui_helpers_and_assets_are_removed() {
    let app_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    for stale_path in [
        "src/macos_bundle.rs",
        "src/view/mod.rs",
        "src/view/chat.rs",
        "src/view/settings.rs",
        "assets/toolbar-chat.svg",
        "assets/toolbar-profile.svg",
        "assets/title-save.svg",
        "assets/title-validate.svg",
    ] {
        assert!(
            !app_root.join(stale_path).exists(),
            "stale GPUI app path should be removed: {stale_path}",
        );
    }

    assert!(app_root.join("assets/noloong-logo.png").exists());
    assert!(app_root.join("assets/noloong-logo.icns").exists());
}

#[test]
fn app_icon_uses_rounded_transparent_corners() {
    let app_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let icon_path = app_root.join("assets/noloong-logo.png");
    let icon = image::open(&icon_path).unwrap_or_else(|error| {
        panic!("{} should be a readable PNG: {error}", icon_path.display())
    });

    assert_eq!(icon.dimensions(), (1024, 1024));
    for (x, y) in [(0, 0), (1023, 0), (0, 1023), (1023, 1023)] {
        assert_eq!(
            icon.get_pixel(x, y).0[3],
            0,
            "app icon corner ({x}, {y}) should be transparent",
        );
    }
    assert_eq!(
        icon.get_pixel(512, 512).0[3],
        255,
        "app icon center should be opaque",
    );
}

#[test]
fn launch_options_serialize_as_frontend_bootstrap_contract() {
    let value = serde_json::to_value(AppLaunchOptions {
        app_version: "0.0.0-test".into(),
        profile_config_path: Some("/tmp/profile.jsonc".into()),
        locale: None,
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:49152/jsonrpc/ws".into(),
            bearer_token: Some("token".into()),
        }),
        interaction_status: Some(AppInteractionStatus::Ready {
            server_name: "noloong-interaction".into(),
            protocol_version: "2026-01-01".into(),
            profiles: vec![InteractionProfileDescriptor {
                profile_id: "chatgpt".into(),
                display_name: "ChatGPT".into(),
                description: None,
                default_manifest_patches: Vec::new(),
                metadata: Default::default(),
            }],
        }),
        runtime_control_endpoint: Some(AppRuntimeControlEndpoint {
            http_url: "http://127.0.0.1:49153".into(),
            bearer_token: Some("control-token".into()),
        }),
    })
    .expect("launch options should serialize");

    assert_eq!(value["appVersion"], "0.0.0-test");
    assert_eq!(value["profileConfigPath"], "/tmp/profile.jsonc");
    assert_eq!(
        value["interactionEndpoint"]["wsUrl"],
        "ws://127.0.0.1:49152/jsonrpc/ws"
    );
    assert_eq!(value["interactionEndpoint"]["bearerToken"], "token");
    assert_eq!(value["interactionStatus"]["status"], "ready");
    assert_eq!(
        value["interactionStatus"]["serverName"],
        "noloong-interaction"
    );
    assert_eq!(
        value["interactionStatus"]["profiles"][0]["profileId"],
        "chatgpt"
    );
    assert_eq!(
        value["runtimeControlEndpoint"]["httpUrl"],
        "http://127.0.0.1:49153"
    );
    assert_eq!(
        value["runtimeControlEndpoint"]["bearerToken"],
        "control-token"
    );
}

fn tauri_config() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    read_json(path)
}

fn read_json(path: impl AsRef<std::path::Path>) -> Value {
    let path = path.as_ref();
    let text = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("{} should exist: {error}", path.display());
    });
    serde_json::from_str(&text).unwrap_or_else(|error| {
        panic!("{} should be valid JSON: {error}", path.display());
    })
}

fn cargo_metadata() -> Value {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(repository_root())
        .output()
        .expect("cargo metadata should run");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata should emit JSON")
}

fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .to_path_buf()
}
