fn main() {
    println!("cargo:rerun-if-changed=src/hvf_probe.c");
    println!("cargo:rerun-if-changed=src/gamepad.m");
    println!("cargo:rerun-if-changed=src/notifications.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/gamepad.m")
            .file("src/notifications.m")
            .flag("-fobjc-arc")
            .warnings_into_errors(true)
            .compile("glidepod_gamepad");
        println!("cargo:rustc-link-lib=framework=GameController");
        println!("cargo:rustc-link-lib=framework=Foundation");
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
