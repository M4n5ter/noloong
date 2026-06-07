use image::GenericImageView;
use noloong_app::{
    AppInteractionEndpoint, AppInteractionStatus, AppLaunchOptions,
    interaction::InteractionProfileDescriptor,
};
use serde_json::Value;
use std::fs;

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
    assert_eq!(config["build"]["devUrl"], "http://localhost:5173");
    assert_eq!(config["build"]["frontendDist"], "../../apps/desktop/dist");

    let window = config["app"]["windows"]
        .as_array()
        .and_then(|windows| windows.first())
        .expect("main window config");

    assert_eq!(window["title"], "Noloong");
    assert_eq!(window["decorations"], true);
    assert_eq!(window["titleBarStyle"], "Overlay");
    assert_eq!(window["hiddenTitle"], true);
    assert_eq!(window["trafficLightPosition"]["x"], 18);
    assert_eq!(window["trafficLightPosition"]["y"], 19);
    assert_eq!(window["closable"], true);
    assert_eq!(window["minimizable"], true);
    assert_eq!(window["maximizable"], true);
    assert_eq!(window["resizable"], true);
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

fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .to_path_buf()
}
