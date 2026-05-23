use gpui::actions;

pub(crate) const APP_KEY_CONTEXT: &str = "NoloongApp";

actions!(
    noloong_app,
    [ToggleJsoncEditor, SaveSettings, ValidateSettings]
);
