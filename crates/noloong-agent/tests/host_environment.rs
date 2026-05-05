use noloong_agent::{HostEnvironment, Locale, PathStyle};

#[test]
fn detects_host_environment_with_explicit_locale() {
    let environment = HostEnvironment::detect(Some(Locale::Zh));

    assert!(!environment.os.is_empty());
    assert!(!environment.arch.is_empty());
    assert!(!environment.default_shell.is_empty());
    assert!(!environment.available_shell_hints.is_empty());
    assert_eq!(environment.locale, Locale::Zh);
    assert_eq!(environment.path_style, PathStyle::detect());
}

#[test]
fn parses_supported_locale_values() {
    assert_eq!(Locale::parse("zh_CN.UTF-8"), Some(Locale::Zh));
    assert_eq!(Locale::parse("en_US.UTF-8"), Some(Locale::En));
    assert_eq!(Locale::parse("C"), Some(Locale::En));
    assert_eq!(Locale::parse("fr_FR.UTF-8"), None);
}
