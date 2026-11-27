//! Two jobs. Every platform: the git commit is compiled in (`SIDFIND_GIT_HASH`) so the window can
//! show which build it is. Windows only: embeds an application manifest. Without it Windows runs
//! the program with the classic (pre-XP) look of the controls, no high-DPI support (blurry on
//! scaled displays) and the legacy ANSI code page.

use embed_manifest::manifest::{ActiveCodePage, DpiAwareness, Setting, SupportedOS::*};
use embed_manifest::{embed_manifest, new_manifest};
use std::process::Command;

/// The short commit hash of HEAD, with `-dirty` when tracked files have uncommitted changes.
/// `unknown` when git is not available (e.g. a source archive).
fn git_hash() -> String {
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
    };
    let Some(hash) = run(&["rev-parse", "--short", "HEAD"]) else {
        return "unknown".to_string();
    };
    let hash = String::from_utf8_lossy(&hash.stdout).trim().to_string();
    let dirty = run(&["diff-index", "--quiet", "HEAD", "--"]).is_none();
    if dirty { format!("{hash}-dirty") } else { hash }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // the commit changes .git/HEAD (branch switch) or the branch file (new commit)
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
    println!("cargo:rustc-env=SIDFIND_GIT_HASH={}", git_hash());
    println!("cargo:rerun-if-changed=../../assets/icons/sidfind.ico");
    // the target, not the host: this also holds when cross-compiling
    if !std::env::var("TARGET")
        .unwrap_or_default()
        .contains("windows")
    {
        return;
    }
    let manifest = new_manifest("SidFind.Gui")
        // Common Controls v6: the current look of buttons, text fields, progress bars
        .supported_os(Windows7..=Windows10)
        // sharp rendering on displays with scaling, per monitor
        .dpi_awareness(DpiAwareness::PerMonitorV2)
        // UTF-8 as the process code page, for paths and names with non-ASCII characters
        .active_code_page(ActiveCodePage::Utf8)
        .long_path_aware(Setting::Enabled);
    if let Err(e) = embed_manifest(manifest) {
        // the program still works, only without the modern look
        println!("cargo:warning=could not embed the Windows manifest: {e}");
    }
    // the program icon in the exe (Explorer, taskbar, Alt-Tab)
    let mut resources = winres::WindowsResource::new();
    resources.set_icon("../../assets/icons/sidfind.ico");
    if let Err(e) = resources.compile() {
        println!("cargo:warning=could not embed the Windows icon: {e}");
    }
}
