fn main() {
    if let Err(error) = klypse_app::cli::parse_from(std::env::args_os()) {
        let exit_code = error.exit_code();
        let _ = error.print();
        std::process::exit(exit_code);
    }

    let language = klypse_app::settings::AppSettings::new()
        .ok()
        .map(|settings| settings.language());
    let localization = klypse_app::i18n::init(language.as_deref());
    klypse_app::logging::init();
    if let Err(error) = localization {
        tracing::warn!(%error, "localization initialization failed");
    }
    std::process::exit(klypse_app::application::run().into());
}
