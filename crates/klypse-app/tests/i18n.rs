use std::process::Command;

#[test]
fn explicit_french_language_loads_the_compiled_catalog_in_a_subprocess() {
    if std::env::var_os("KLYPSE_I18N_TEST_CHILD").is_some() {
        klypse_app::i18n::init(Some("fr")).unwrap();
        assert_eq!(
            klypse_app::i18n::gettext("Capture area"),
            "Capturer une zone"
        );
        return;
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "explicit_french_language_loads_the_compiled_catalog_in_a_subprocess",
            "--nocapture",
        ])
        .env("KLYPSE_I18N_TEST_CHILD", "1")
        .env("KLYPSE_LOCALE_DIR", env!("KLYPSE_BUILD_LOCALE_DIR"))
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .status()
        .unwrap();

    assert!(status.success());
}
