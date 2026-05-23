mod actions;
mod chat;
mod i18n;
pub mod interaction;
#[cfg(target_os = "macos")]
mod macos_bundle;
mod model;
mod runtime;
#[cfg(test)]
mod test_support;
mod view;

pub(crate) use actions::{APP_KEY_CONTEXT, SaveSettings, ToggleJsoncEditor, ValidateSettings};
pub(crate) use i18n::{AppI18nCatalog, AppTextKey};
pub use interaction::{
    AppInteractionEndpoint, AppInteractionHttpClient, AppInteractionStatus, AppInteractionWsClient,
    initialize_interaction_status,
};
pub use model::{AppError, AppLaunchOptions};
pub(crate) use model::{AppRoute, AppStatus, AppViewModel, ChatEmptyState};
pub use runtime::run_app;
