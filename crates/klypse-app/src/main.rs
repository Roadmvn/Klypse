fn main() {
    // Has to happen while the process is still single threaded: the settings
    // backend below pulls in GIO, which immediately spawns workers.
    klypse_app::i18n::init_locale();

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
