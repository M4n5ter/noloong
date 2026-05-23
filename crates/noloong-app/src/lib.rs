mod actions;
mod i18n;
#[cfg(target_os = "macos")]
mod macos_bundle;
mod model;
mod runtime;
#[cfg(test)]
mod test_support;
mod view;

pub(crate) use actions::{APP_KEY_CONTEXT, SaveSettings, ToggleJsoncEditor, ValidateSettings};
pub(crate) use i18n::{AppI18nCatalog, AppTextKey};
pub use model::{AppError, AppInteractionEndpoint, AppLaunchOptions};
pub(crate) use model::{AppRoute, AppStatus, AppViewModel};
pub use runtime::run_app;
