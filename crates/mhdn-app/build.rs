fn main() {
    #[cfg(windows)]
    embed_windows_icon();
}

#[cfg(windows)]
fn embed_windows_icon() {
    use std::path::PathBuf;

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let icon = manifest_dir.join("../../assets/mhdn.ico");
    if !icon.is_file() {
        panic!("missing Windows icon at {}", icon.display());
    }

    winresource::WindowsResource::new()
        .set_icon(icon.to_str().expect("icon path is UTF-8"))
        .compile()
        .expect("embed Windows application icon");
}
