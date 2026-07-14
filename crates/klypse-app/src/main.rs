fn main() {
    if let Err(error) = klypse_app::cli::parse_from(std::env::args_os()) {
        let exit_code = error.exit_code();
        let _ = error.print();
        std::process::exit(exit_code);
    }

    klypse_app::logging::init();
    if let Err(error) = klypse_app::i18n::init() {
        tracing::warn!(%error, "localization initialization failed");
    }
    std::process::exit(klypse_app::application::run().into());
}
