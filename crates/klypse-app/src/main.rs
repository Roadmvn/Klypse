fn main() {
    klypse_app::logging::init();
    if let Err(error) = klypse_app::i18n::init() {
        tracing::warn!(%error, "localization initialization failed");
    }
    std::process::exit(klypse_app::application::run().into());
}
