use std::{
    collections::HashMap,
    env,
    error::Error,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use gettextrs::{
    LocaleCategory, bind_textdomain_codeset, bindtextdomain, gettext as system_gettext, setlocale,
    textdomain,
};

pub const DOMAIN: &str = "klypse";

enum TranslationMode {
    System,
    English,
    French(HashMap<String, String>),
}

static TRANSLATIONS: OnceLock<TranslationMode> = OnceLock::new();

pub fn init(language: Option<&str>) -> Result<(), Box<dyn Error>> {
    setlocale(LocaleCategory::LcAll, "");
    let locale_directory = locale_directory();
    bindtextdomain(DOMAIN, &locale_directory)?;
    bind_textdomain_codeset(DOMAIN, "UTF-8")?;
    textdomain(DOMAIN)?;

    let mode = match language {
        Some("en") => TranslationMode::English,
        Some("fr") => TranslationMode::French(load_mo_catalog(
            &PathBuf::from(locale_directory)
                .join("fr/LC_MESSAGES")
                .join(format!("{DOMAIN}.mo")),
        )?),
        _ => TranslationMode::System,
    };
    let _ = TRANSLATIONS.set(mode);
    Ok(())
}

pub fn gettext(message: &str) -> String {
    match TRANSLATIONS.get() {
        Some(TranslationMode::English) => message.to_owned(),
        Some(TranslationMode::French(catalog)) => catalog
            .get(message)
            .cloned()
            .unwrap_or_else(|| message.to_owned()),
        Some(TranslationMode::System) | None => system_gettext(message),
    }
}

fn locale_directory() -> OsString {
    locale_directory_from(
        env::var_os("KLYPSE_LOCALE_DIR"),
        env::var_os("FLATPAK_ID").is_some(),
        cfg!(debug_assertions)
            .then(|| option_env!("KLYPSE_BUILD_LOCALE_DIR"))
            .flatten(),
    )
}

fn locale_directory_from(
    explicit: Option<OsString>,
    flatpak: bool,
    debug_directory: Option<&str>,
) -> OsString {
    explicit
        .or_else(|| flatpak.then(|| OsString::from("/app/share/locale")))
        .or_else(|| debug_directory.map(OsString::from))
        .unwrap_or_else(|| OsString::from("/usr/share/locale"))
}

fn load_mo_catalog(path: &Path) -> io::Result<HashMap<String, String>> {
    let bytes = fs::read(path)?;
    let little_endian = match bytes.get(..4) {
        Some([0xde, 0x12, 0x04, 0x95]) => true,
        Some([0x95, 0x04, 0x12, 0xde]) => false,
        _ => return Err(invalid_catalog("invalid gettext catalog magic")),
    };
    let string_count = read_u32(&bytes, 8, little_endian)? as usize;
    let originals = read_u32(&bytes, 12, little_endian)? as usize;
    let translations = read_u32(&bytes, 16, little_endian)? as usize;
    let mut catalog = HashMap::with_capacity(string_count);

    for index in 0..string_count {
        let original = read_string(&bytes, originals, index, little_endian)?;
        if original.is_empty() {
            continue;
        }
        let translation = read_string(&bytes, translations, index, little_endian)?;
        catalog.insert(
            original.split('\0').next().unwrap_or_default().to_owned(),
            translation
                .split('\0')
                .next()
                .unwrap_or_default()
                .to_owned(),
        );
    }
    Ok(catalog)
}

fn read_string(
    bytes: &[u8],
    table: usize,
    index: usize,
    little_endian: bool,
) -> io::Result<String> {
    let entry = table
        .checked_add(
            index
                .checked_mul(8)
                .ok_or_else(|| invalid_catalog("catalog table overflow"))?,
        )
        .ok_or_else(|| invalid_catalog("catalog table overflow"))?;
    let length = read_u32(bytes, entry, little_endian)? as usize;
    let offset = read_u32(bytes, entry + 4, little_endian)? as usize;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid_catalog("catalog string overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| invalid_catalog("catalog string is out of bounds"))?;
    std::str::from_utf8(value)
        .map(str::to_owned)
        .map_err(|_| invalid_catalog("catalog string is not UTF-8"))
}

fn read_u32(bytes: &[u8], offset: usize, little_endian: bool) -> io::Result<u32> {
    let raw: [u8; 4] = bytes
        .get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| invalid_catalog("truncated gettext catalog"))?;
    Ok(if little_endian {
        u32::from_le_bytes(raw)
    } else {
        u32::from_be_bytes(raw)
    })
}

fn invalid_catalog(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatpak_uses_the_application_locale_directory_even_in_debug_builds() {
        assert_eq!(
            locale_directory_from(None, true, Some("/tmp/build-locale")),
            OsString::from("/app/share/locale")
        );
    }

    #[test]
    fn an_explicit_locale_directory_has_the_highest_priority() {
        assert_eq!(
            locale_directory_from(Some(OsString::from("/tmp/locales")), true, None),
            OsString::from("/tmp/locales")
        );
    }
}
