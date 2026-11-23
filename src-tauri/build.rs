fn main() {
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        include_str!("../VERSION").trim(),
        "Native and Relay versions differ"
    );
    // Sparkle shim (INTEGRATION.md §3A): libkhsparkle.a is static, but Sparkle.framework's install
    // name is @rpath/Sparkle.framework/Versions/B/Sparkle — the bundle needs Contents/Frameworks and
    // this rpath, or the app links and dies at launch.
    let kh = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/khsparkle");
    println!("cargo:rustc-link-search=native={}", kh.display());
    println!("cargo:rustc-link-lib=static=khsparkle");
    println!("cargo:rustc-link-search=framework={}", kh.display());
    println!("cargo:rustc-link-lib=framework=Sparkle");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    println!("cargo:rerun-if-changed=vendor/khsparkle/libkhsparkle.a");
    tauri_build::build();
}
