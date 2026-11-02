//! Pro Internet Manager restyle: embeds assets\ProInternetManager.ico as the exe icon
//! with rc.exe from the Windows SDK (no extra crates, works with --offline).
use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/ProInternetManager.ico");
    println!("cargo:rerun-if-env-changed=RC_EXE");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let ico = root.join("assets").join("ProInternetManager.ico");
    let rc_file = out.join("pim.rc");
    let res_file = out.join("pim.res");
    let ico_text = ico.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc_file, format!("1 ICON \"{ico_text}\"\n")).expect("write pim.rc");
    let rc = env::var("RC_EXE").unwrap_or_else(|_| "rc.exe".to_string());
    let status = Command::new(&rc).arg("/nologo").arg("/fo").arg(&res_file).arg(&rc_file).status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", res_file.display()),
        _ => println!("cargo:warning=rc.exe not found or failed ({rc}); the exe icon was not embedded"),
    }
}
