//! The About window: logo, author, version and build (the git commit the program was built
//! from). Modal, opened from the info icon in the main window.

use wxdragon::prelude::*;

use crate::ui::{logo_bitmap, logo_ink};
use crate::widgets::{GAP, PAD, grow};

/// The release version, from Cargo.toml.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The git commit the build was made from (see build.rs).
const BUILD: &str = env!("SIDFIND_GIT_HASH");
const WIDTH: i32 = 320;

/// Shows the About window and returns when it is closed.
pub fn show(parent: &Frame) {
    let dialog = Dialog::builder(parent, "About SidFind").build();
    let panel = Panel::builder(&dialog).build();
    let root = BoxSizer::builder(Orientation::Vertical).build();

    let logo = StaticBitmap::builder(&panel)
        .with_bitmap(
            logo_bitmap(&panel, &logo_ink()).or_else(|| Bitmap::from_rgba(&[0, 0, 0, 0], 1, 1)),
        )
        .build();
    root.add(
        &logo,
        0,
        SizerFlag::AlignCenterHorizontal | SizerFlag::Top,
        PAD,
    );
    let author = StaticText::builder(&panel)
        .with_label("By Doc Snyder")
        .build();
    root.add(
        &author,
        0,
        SizerFlag::AlignCenterHorizontal | SizerFlag::Top,
        GAP,
    );
    root.add_spacer(2 * PAD);
    for line in [format!("Version {VERSION}"), format!("Build {BUILD}")] {
        let text = StaticText::builder(&panel).with_label(&line).build();
        root.add(
            &text,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::Top,
            GAP,
        );
    }
    root.add_stretch_spacer(1);
    let close = Button::builder(&panel).with_label("Close").build();
    grow(&close);
    root.add(
        &close,
        0,
        SizerFlag::AlignCenterHorizontal | SizerFlag::All,
        PAD,
    );

    panel.set_sizer(root, true);
    let dialog_sizer = BoxSizer::builder(Orientation::Vertical).build();
    dialog_sizer.add(&panel, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(dialog_sizer, true);
    dialog.fit();
    dialog.set_size(Size::new(WIDTH, dialog.get_size().height));
    dialog.centre();

    let closer = dialog.clone();
    close.on_click(move |_| closer.end_modal(ID_OK));
    dialog.set_escape_id(ID_CANCEL);
    dialog.show_modal();
}
