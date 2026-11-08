fn main() {
    println!("cargo:rerun-if-changed=src/hvf_probe.c");
    println!("cargo:rerun-if-changed=src/gamepad.m");
    println!("cargo:rerun-if-changed=src/notifications.m");
    println!("cargo:rerun-if-changed=src/network.m");
    println!("cargo:rerun-if-changed=src/location.m");
    println!("cargo:rerun-if-changed=src/download.m");
    println!("cargo:rerun-if-changed=src/files.m");
    println!("cargo:rerun-if-changed=src/gestures.m");
    println!("cargo:rerun-if-changed=src/launchers.m");
    println!("cargo:rerun-if-changed=src/tray");
    println!("cargo:rerun-if-changed=src/process_usage.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/gamepad.m")
            .file("src/notifications.m")
            .file("src/network.m")
            .file("src/location.m")
            .file("src/download.m")
            .file("src/files.m")
            .file("src/gestures.m")
            .file("src/launchers.m")
            .flag("-fobjc-arc")
            .warnings_into_errors(true)
            .compile("glidepod_gamepad");
        build_swift_tray();
        cc::Build::new()
            .file("src/process_usage.c")
            .warnings_into_errors(true)
            .compile("glidepod_process_usage");
        println!("cargo:rustc-link-lib=framework=GameController");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=CoreServices");
        println!("cargo:rustc-link-lib=framework=UniformTypeIdentifiers");
        println!("cargo:rustc-link-lib=framework=ImageIO");
        println!("cargo:rustc-link-lib=framework=CFNetwork");
        println!("cargo:rustc-link-lib=framework=vmnet");
        println!("cargo:rustc-link-lib=framework=CoreLocation");
        println!("cargo:rustc-link-lib=framework=UserNotifications");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64")
    {
        cc::Build::new()
            .file("src/hvf_probe.c")
            .flag("-std=c11")
            .warnings_into_errors(true)
            .compile("glidepod_hvf_probe");
        println!("cargo:rustc-link-lib=framework=Hypervisor");
    }
}

fn build_swift_tray() {
    use std::{path::PathBuf, process::Command};
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-env-changed=SDKROOT");
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("Unsupported macOS Swift architecture: {other}"),
    };
    let version = std::env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "15.0".into());
    let target = format!("{arch}-apple-macosx{version}");
    let sdk = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .expect("Xcode is required to compile the SwiftUI tray");
    assert!(sdk.status.success(), "Could not locate the macOS SDK");
    let sdk = String::from_utf8(sdk.stdout).unwrap();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let status = Command::new("xcrun")
        .args([
            "--sdk",
            "macosx",
            "swiftc",
            "-emit-library",
            "-static",
            "-parse-as-library",
            "-module-name",
            "GlidePodTray",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-O",
            "-target",
            &target,
            "-sdk",
            sdk.trim(),
            "src/tray/TrayModel.swift",
            "src/tray/TrayPanel.swift",
            "src/tray/Tray.swift",
            "-o",
        ])
        .arg(out.join("libglidepod_tray.a"))
        .status()
        .expect("Could not run the Xcode Swift compiler");
    assert!(status.success(), "SwiftUI tray compilation failed");
    // Swift emits autolink directives into its objects. Make both SDK runtime
    // stubs and the selected toolchain's compatibility archives visible to ld.
    let compiler = Command::new("xcrun")
        .args(["--sdk", "macosx", "--find", "swiftc"])
        .output()
        .expect("Could not locate swiftc");
    assert!(compiler.status.success(), "Could not locate swiftc");
    let compiler = PathBuf::from(String::from_utf8(compiler.stdout).unwrap().trim());
    let libraries = compiler
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("lib/swift/macosx");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-search=native={}", libraries.display());
    println!(
        "cargo:rustc-link-search=native={}/usr/lib/swift",
        sdk.trim()
    );
    println!("cargo:rustc-link-lib=static=glidepod_tray");
    println!("cargo:rustc-link-lib=framework=SwiftUI");
}
