#[path = "src/git_sha.rs"]
mod git_sha;

fn main() {
    let sha = git_short_sha();
    println!("cargo:rustc-env=MHDN_GIT_SHA={sha}");
    if let Some(head) = git_head_path() {
        println!("cargo:rerun-if-changed={head}");
    }

    #[cfg(windows)]
    embed_windows_icon();
}

fn git_short_sha() -> String {
    match std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        Ok(output) => git_sha::normalize(&output.stdout, output.status.success()),
        Err(_) => "unknown".to_string(),
    }
}

fn git_head_path() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let dir = String::from_utf8(output.stdout).ok()?;
    let dir = dir.trim();
    if dir.is_empty() {
        None
    } else {
        Some(format!("{dir}/HEAD"))
    }
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
