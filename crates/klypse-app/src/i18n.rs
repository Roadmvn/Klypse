use std::{env, error::Error, ffi::OsString};

use gettextrs::{LocaleCategory, bind_textdomain_codeset, bindtextdomain, setlocale, textdomain};

pub const DOMAIN: &str = "klypse";

pub fn init() -> Result<(), Box<dyn Error>> {
    setlocale(LocaleCategory::LcAll, "");
    let locale_directory =
        env::var_os("KLYPSE_LOCALE_DIR").unwrap_or_else(|| OsString::from("/usr/share/locale"));
    bindtextdomain(DOMAIN, locale_directory)?;
    bind_textdomain_codeset(DOMAIN, "UTF-8")?;
    textdomain(DOMAIN)?;
    Ok(())
}
