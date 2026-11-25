//! Windows only: embeds an application manifest. Without it Windows runs the program with the
//! classic (pre-XP) look of the controls, no high-DPI support (blurry on scaled displays) and
//! the legacy ANSI code page. On other platforms this does nothing.

use embed_manifest::manifest::{ActiveCodePage, DpiAwareness, Setting, SupportedOS::*};
use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // the target, not the host: this also holds when cross-compiling
    if !std::env::var("TARGET").unwrap_or_default().contains("windows") {
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
}
