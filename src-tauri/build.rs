fn main() {
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        include_str!("../VERSION").trim(),
        "Native and Relay versions differ"
    );
    // Issue 20261123-13: without tauri's bundled-asset feature a Release build writes this Mac's
    // home folder into the binary, and RUSTFLAGS cannot rewrite that string — a build script
    // generates it. Fail at the boundary instead of producing it.
    if std::env::var("PROFILE").as_deref() == Ok("release")
        && std::env::var_os("CARGO_FEATURE_BUNDLED").is_none()
    {
        panic!(
            "refusing a Release build that would embed this Mac's home folder: assets are bundled \
             only with the `bundled` feature. Build with `npx @tauri-apps/cli@2 build --bundles app \
             -- --features bundled`, or add `--features bundled`."
        );
    }
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
