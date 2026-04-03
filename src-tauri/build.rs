//! Build script: injects `GIT_HASH` env var and runs `tauri_build`.

use std::process::Command;

fn main() {
    // Re-run when the git HEAD changes (new commits, branch switches).
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/");

    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| String::from("unknown"), |s| s.trim().to_owned());

    println!("cargo:rustc-env=GIT_HASH={git_hash}");

    tauri_build::build();
}
