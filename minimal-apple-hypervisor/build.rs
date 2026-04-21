use std::{path::PathBuf, process::Command};

fn get_sdk_path() -> String {
    let output = Command::new("xcrun")
        .arg("-sdk")
        .arg("macosx")
        .arg("--show-sdk-path")
        .output()
        .expect("failed to run xcrun -sdk macosx --show-sdk-path");

    if !output.status.success() {
        panic!(
            "failed to get sdk path: {}",
            String::from_utf8_lossy(output.stderr.as_ref())
        );
    }

    let sdk_path: String = String::from_utf8_lossy(output.stdout.as_ref()).into();

    sdk_path.trim().into()
}

fn main() {
    let binding_rs_path =
        PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR env var not present"))
            .join("bindings.rs");

    let sdk_path = get_sdk_path();

    bindgen::builder()
        .header("wrapper.h")
        .clang_arg(format!("-F{}/System/Library/Frameworks", sdk_path))
        .derive_debug(true)
        .derive_default(true)
        .generate()
        .expect("failed to generate bindings")
        .write_to_file(binding_rs_path)
        .expect("failed to write bindings.rs");

    println!("cargo:rustc-link-lib=framework=Hypervisor");
    ()
}
