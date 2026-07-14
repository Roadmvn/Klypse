use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=resources/io.github.roadmvn.Klypse.gschema.xml");
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
}
