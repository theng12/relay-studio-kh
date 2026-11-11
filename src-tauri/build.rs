fn main() {
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        include_str!("../VERSION").trim(),
        "Native and Relay versions differ"
    );
    tauri_build::build();
}
