use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=resources/io.github.roadmvn.Klypse.gschema.xml");
    println!("cargo:rerun-if-changed=resources/po/fr.po");
    let source = PathBuf::from("resources");
    let destination =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo")).join("schemas");
    fs::create_dir_all(&destination).expect("create the build schema directory");
    let status = Command::new("glib-compile-schemas")
        .arg("--targetdir")
        .arg(&destination)
        .arg(&source)
        .status()
        .expect("glib-compile-schemas is required to build Klypse");
    assert!(status.success(), "compile the Klypse GSettings schema");
    println!(
        "cargo:rustc-env=KLYPSE_BUILD_SCHEMA_DIR={}",
        destination.display()
    );

    let locale_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo")).join("locale");
    let messages = locale_directory.join("fr/LC_MESSAGES");
    fs::create_dir_all(&messages).expect("create the build locale directory");
    let status = Command::new("msgfmt")
        .args(["--check", "--check-format", "--output-file"])
        .arg(messages.join("klypse.mo"))
        .arg("resources/po/fr.po")
        .status()
        .expect("msgfmt is required to build Klypse");
    assert!(status.success(), "compile the Klypse French catalog");
    println!(
        "cargo:rustc-env=KLYPSE_BUILD_LOCALE_DIR={}",
        locale_directory.display()
    );
}
