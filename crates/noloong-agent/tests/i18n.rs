use noloong_agent::{Catalog, HostEnvironment, Locale, MessageKey};

#[test]
fn i18n_catalog_has_all_english_keys() {
    Catalog::assert_complete(Locale::En);
}

#[test]
fn i18n_catalog_has_all_chinese_keys() {
    Catalog::assert_complete(Locale::Zh);
}

#[test]
fn renders_host_environment_context() {
    let environment = HostEnvironment::detect(Some(Locale::En));
    let rendered = Catalog::new(Locale::En).render_host_environment(&environment);

    assert!(
        rendered.contains(Catalog::new(Locale::En).message(MessageKey::HostEnvironmentContext))
    );
    assert!(rendered.contains(&environment.os));
    assert!(rendered.contains(&environment.default_shell));
}
