use noloong_agent::Locale;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTextKey {
    AppTitle,
    ProfileSettingsTitle,
    Saved,
    Valid,
    Invalid,
    SaveFailed,
    Tools,
    Settings,
    JsoncButton,
    Identity,
    ProfileId,
    DisplayName,
    Locale,
    DefaultProfile,
    Provider,
    ProviderType,
    Model,
    Compaction,
    Storage,
    Plugins,
    ManifestPatches,
    Metadata,
    JsoncEditor,
    Format,
    JsoncInvalid,
    Copy,
    Close,
    ChatPlaceholder,
    ChatComposerPlaceholder,
    ChatTokenCounter,
    ChatDisabled,
    ToolsPlaceholder,
    SettingsPlaceholder,
}

impl AppTextKey {
    #[cfg(test)]
    pub const ALL: &'static [Self] = &[
        Self::AppTitle,
        Self::ProfileSettingsTitle,
        Self::Saved,
        Self::Valid,
        Self::Invalid,
        Self::SaveFailed,
        Self::Tools,
        Self::Settings,
        Self::JsoncButton,
        Self::Identity,
        Self::ProfileId,
        Self::DisplayName,
        Self::Locale,
        Self::DefaultProfile,
        Self::Provider,
        Self::ProviderType,
        Self::Model,
        Self::Compaction,
        Self::Storage,
        Self::Plugins,
        Self::ManifestPatches,
        Self::Metadata,
        Self::JsoncEditor,
        Self::Format,
        Self::JsoncInvalid,
        Self::Copy,
        Self::Close,
        Self::ChatPlaceholder,
        Self::ChatComposerPlaceholder,
        Self::ChatTokenCounter,
        Self::ChatDisabled,
        Self::ToolsPlaceholder,
        Self::SettingsPlaceholder,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppI18nCatalog {
    locale: Locale,
}

impl AppI18nCatalog {
    pub fn new(locale: Locale) -> Self {
        Self { locale }
    }

    pub fn text(self, key: AppTextKey) -> &'static str {
        match self.locale {
            Locale::Zh => zh_text(key),
            Locale::En => en_text(key),
        }
    }
}

fn zh_text(key: AppTextKey) -> &'static str {
    match key {
        AppTextKey::AppTitle => "Noloong",
        AppTextKey::ProfileSettingsTitle => "配置档设置",
        AppTextKey::Saved => "已保存",
        AppTextKey::Valid => "配置有效",
        AppTextKey::Invalid => "配置无效",
        AppTextKey::SaveFailed => "保存失败",
        AppTextKey::Tools => "工具",
        AppTextKey::Settings => "设置",
        AppTextKey::JsoncButton => "JSONC",
        AppTextKey::Identity => "身份",
        AppTextKey::ProfileId => "配置档 ID",
        AppTextKey::DisplayName => "显示名称",
        AppTextKey::Locale => "语言区域",
        AppTextKey::DefaultProfile => "默认配置档",
        AppTextKey::Provider => "提供商",
        AppTextKey::ProviderType => "提供商类型",
        AppTextKey::Model => "模型",
        AppTextKey::Compaction => "上下文压缩",
        AppTextKey::Storage => "存储",
        AppTextKey::Plugins => "插件",
        AppTextKey::ManifestPatches => "Manifest 补丁",
        AppTextKey::Metadata => "元数据",
        AppTextKey::JsoncEditor => "JSONC 编辑",
        AppTextKey::Format => "格式化",
        AppTextKey::JsoncInvalid => "JSONC 无效",
        AppTextKey::Copy => "复制",
        AppTextKey::Close => "关闭",
        AppTextKey::ChatPlaceholder => "对话客户端将在后续版本启用。",
        AppTextKey::ChatComposerPlaceholder => "输入消息...",
        AppTextKey::ChatTokenCounter => "0 个令牌",
        AppTextKey::ChatDisabled => "发送暂不可用",
        AppTextKey::ToolsPlaceholder => "工具状态将在接入 interaction 后显示。",
        AppTextKey::SettingsPlaceholder => "应用设置将在后续版本启用。",
    }
}

fn en_text(key: AppTextKey) -> &'static str {
    match key {
        AppTextKey::AppTitle => "Noloong",
        AppTextKey::ProfileSettingsTitle => "Profile Settings",
        AppTextKey::Saved => "Saved",
        AppTextKey::Valid => "Valid",
        AppTextKey::Invalid => "Invalid",
        AppTextKey::SaveFailed => "Save failed",
        AppTextKey::Tools => "Tools",
        AppTextKey::Settings => "Settings",
        AppTextKey::JsoncButton => "JSONC",
        AppTextKey::Identity => "Identity",
        AppTextKey::ProfileId => "Profile ID",
        AppTextKey::DisplayName => "Display Name",
        AppTextKey::Locale => "Locale",
        AppTextKey::DefaultProfile => "Default Profile",
        AppTextKey::Provider => "Provider",
        AppTextKey::ProviderType => "Provider Type",
        AppTextKey::Model => "Model",
        AppTextKey::Compaction => "Compaction",
        AppTextKey::Storage => "Storage",
        AppTextKey::Plugins => "Plugins",
        AppTextKey::ManifestPatches => "Manifest Patches",
        AppTextKey::Metadata => "Metadata",
        AppTextKey::JsoncEditor => "JSONC Editor",
        AppTextKey::Format => "Format",
        AppTextKey::JsoncInvalid => "Invalid JSONC",
        AppTextKey::Copy => "Copy",
        AppTextKey::Close => "Close",
        AppTextKey::ChatPlaceholder => "The interaction client will be enabled in a later version.",
        AppTextKey::ChatComposerPlaceholder => "Type a message...",
        AppTextKey::ChatTokenCounter => "0 tokens",
        AppTextKey::ChatDisabled => "Sending is not available yet",
        AppTextKey::ToolsPlaceholder => "Tool status will appear after interaction support lands.",
        AppTextKey::SettingsPlaceholder => "App settings will be enabled in a later version.",
    }
}

#[cfg(test)]
mod tests {
    use super::{AppI18nCatalog, AppTextKey};
    use noloong_agent::Locale;

    #[test]
    fn all_catalog_entries_are_present() {
        for locale in [Locale::Zh, Locale::En] {
            let catalog = AppI18nCatalog::new(locale);
            for key in AppTextKey::ALL {
                assert!(!catalog.text(*key).trim().is_empty(), "{locale:?} {key:?}");
            }
        }
    }

    #[test]
    fn locale_specific_labels_do_not_mix_languages() {
        let zh = AppI18nCatalog::new(Locale::Zh);
        let en = AppI18nCatalog::new(Locale::En);

        assert_eq!(zh.text(AppTextKey::Identity), "身份");
        assert_eq!(en.text(AppTextKey::Identity), "Identity");
        assert_eq!(zh.text(AppTextKey::Saved), "已保存");
        assert_eq!(en.text(AppTextKey::Saved), "Saved");
    }
}
