mod i18n;
#[cfg(target_os = "macos")]
mod macos_bundle;
mod model;
mod runtime;
#[cfg(test)]
mod test_support;
mod view;

pub(crate) use i18n::{AppI18nCatalog, AppTextKey};
pub use model::{AppError, AppLaunchOptions};
pub(crate) use model::{AppRoute, AppStatus, AppViewModel};
pub use runtime::run_app;
