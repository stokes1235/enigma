use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-env-changed=R1999_EMBED_ELEVATION");
    println!("cargo:rerun-if-env-changed=R1999_RC");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    if env::var_os("R1999_EMBED_ELEVATION").is_none() { return; }
    let rc = env::var_os("R1999_RC").expect("R1999_RC must point to the Windows SDK resource compiler");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("launcher.res");
    let status = Command::new(rc).arg("/nologo").arg(format!("/fo{}", out.display())).arg("app.rc").status().expect("start rc.exe");
    assert!(status.success(), "resource compiler failed: {status}");
    println!("cargo:rustc-link-arg={}", out.display());
    println!("cargo:rustc-link-arg=/MANIFEST:NO");
}
