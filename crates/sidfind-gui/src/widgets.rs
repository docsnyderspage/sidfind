//! Small widgets and helpers shared by the main window and the settings window.

use std::path::Path;

use wxdragon::prelude::*;

pub const PAD: i32 = 12;
/// Gap between a field or button and the icons beside it.
pub const GAP: i32 = 8;
/// Indent of the path captions: lines them up with the text inside the field and buttons.
pub const CAPTION_INSET: i32 = 6;
pub const TITLE: &str = "SidFind";

/// The grey of the icon SVGs (the colour they draw in at rest). Hover swaps it for the text colour.
pub const ICON_GREY: &str = "#8e8e93";

/// Half-filled circle ("appearance"), same grey as the trash icon.
pub const THEME_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#8e8e93" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 3a9 9 0 0 1 0 18z" fill="#8e8e93"/></svg>"##;

/// Gear ("settings"), same grey.
pub const GEAR_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#8e8e93" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>"##;

/// Perceived brightness, 0 (black) to 255 (white).
pub fn luma(c: Colour) -> f32 {
    0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32
}

/// `a` blended towards `b` by `t` (0.0 = a, 1.0 = b).
pub fn mix(a: Colour, b: Colour, t: f32) -> Colour {
    let channel = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Colour::rgb(channel(a.r, b.r), channel(a.g, b.g), channel(a.b, b.b))
}

/// The grey for secondary text on `window`. On a dark background the system grey (a dark
/// grey on Windows) is too faint, so it is derived from the text colour instead.
pub fn muted(window: &dyn WxWidget) -> Colour {
    let background = window.get_background_color();
    if luma(background) < 128.0 {
        mix(SystemSettings::get_colour(SystemColour::WindowText), background, 0.3)
    } else {
        SystemSettings::get_colour(SystemColour::GrayText)
    }
}

/// Vertical offset that puts the baseline of a line of text on the bottom edge of something
/// `above_height` tall: the text box is `text_height` high, of which `descent` is below its baseline.
pub fn baseline_offset(above_height: i32, text_height: i32, descent: i32) -> i32 {
    (above_height - (text_height - descent)).max(0)
}

/// Info icon ("i" in an outlined circle), same grey.
pub const INFO_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#8e8e93" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>"##;

/// Trash-can icon as inline SVG. Mid-grey reads on both light and dark backgrounds.
fn trash_svg(opacity: f32) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#8e8e93" stroke-opacity="{opacity}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14M10 10v6M14 10v6"/></svg>"##
    )
}

/// An icon-only control drawn from an inline SVG (crisp at any scale, no image files). It is a
/// StaticBitmap, not a button: wxWidgets draws a bezel around a bitmap button on macOS whatever
/// the border style, and the click is bound by the caller (`on_mouse_left_up`).
/// On hover the icon takes the window text colour and the cursor becomes a hand, so it reads as
/// clickable. The SVG must draw in `ICON_GREY`, which is what gets swapped on hover.
pub fn icon_button(panel: &Panel, svg: &'static str, size: i32, tip: &str) -> StaticBitmap {
    let icon = StaticBitmap::builder(panel).with_bitmap(icon_bitmap(panel, svg, size)).build();
    icon.set_tooltip(tip);
    if let Some(hand) = Cursor::from_stock(StockCursor::Hand) {
        icon.set_cursor(Some(&hand));
    }
    let on_enter = icon;
    icon.on_mouse_enter(move |_| {
        let ink = SystemSettings::get_colour(SystemColour::WindowText);
        let hover = svg.replace(ICON_GREY, &format!("#{:02x}{:02x}{:02x}", ink.r, ink.g, ink.b));
        if let Some(bundle) = BitmapBundle::from_svg_text(&hover, Size::new(size, size)) {
            on_enter.set_bitmap_bundle(&bundle);
        }
    });
    let on_leave = icon;
    icon.on_mouse_leave(move |_| {
        if let Some(bundle) = BitmapBundle::from_svg_text(svg, Size::new(size, size)) {
            on_leave.set_bitmap_bundle(&bundle);
        }
    });
    icon
}

/// The bitmap of an inline SVG icon at `size`.
fn icon_bitmap(panel: &Panel, svg: &str, size: i32) -> Option<Bitmap> {
    BitmapBundle::from_svg_text(svg, Size::new(size, size)).and_then(|b| b.get_bitmap_for(panel))
}

/// The trash button: starts disabled, with a dimmed icon for that state.
pub fn trash_button(panel: &dyn WxWidget, tip: &str) -> BitmapButton {
    let bitmap = |opacity| BitmapBundle::from_svg_text(&trash_svg(opacity), Size::new(16, 16)).and_then(|b| b.get_bitmap_for(panel));
    let button = BitmapButton::builder(panel).with_bitmap(bitmap(1.0)).with_bitmap_disabled(bitmap(0.3)).build();
    button.set_tooltip(tip);
    button.enable(false);
    button
}

/// Makes an icon button at least `side` pixels square, so it lines up with the fields and buttons
/// beside it (Windows sizes a bitmap button to its icon; macOS pads it, which stays as it is).
pub fn square(window: &dyn WxWidget, side: i32) {
    window.set_min_size(Size::new(side, side));
}

/// A path setting: a Select/Change button, a trash icon and a grey caption with the path.
#[derive(Clone, Copy)]
pub struct PathSetting {
    pub button: Button,
    pub clear: BitmapButton,
    caption: StaticText,
}

impl PathSetting {
    /// The setting with its label above; add the returned sizer to a layout. A non-empty
    /// `description` is shown in grey, and `link` (text, URL) as a link that opens in the
    /// default browser. The link sits at the right end of the label's line, the description
    /// on its own line below it; the button follows.
    pub fn new(panel: &Panel, label: &str, description: &str, link: Option<(&str, &str)>, clear_tip: &str) -> (PathSetting, BoxSizer) {
        let label = StaticText::builder(panel).with_label(label).build();
        let button = Button::builder(panel).with_label("Select...").build();
        let clear = trash_button(panel, clear_tip);
        square(&clear, button.get_best_size().height);
        let caption = StaticText::builder(panel).with_label(" ").build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&button, 1, SizerFlag::Expand | SizerFlag::AlignCenterVertical, 0);
        row.add(&clear, 0, SizerFlag::AlignCenterVertical | SizerFlag::Left, GAP);
        let cell = BoxSizer::builder(Orientation::Vertical).build();
        // the label, with the link (if any) at the right end of the same line
        let header = BoxSizer::builder(Orientation::Horizontal).build();
        header.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
        header.add_stretch_spacer(1);
        if let Some((text, url)) = link {
            let link = HyperlinkCtrl::builder(panel).with_label(text).with_url(url).build();
            header.add(&link, 0, SizerFlag::AlignCenterVertical, 0);
        }
        let below_header = if description.is_empty() { 6 } else { 2 };
        cell.add_sizer(&header, 0, SizerFlag::Expand | SizerFlag::Bottom, below_header);
        if !description.is_empty() {
            let note = StaticText::builder(panel).with_label(description).build();
            note.set_foreground_color(muted(panel));
            cell.add(&note, 0, SizerFlag::Bottom, 6);
        }
        cell.add_sizer(&row, 0, SizerFlag::Expand, 0);
        cell.add_sizer(&indented(&caption), 0, SizerFlag::Expand | SizerFlag::Top, 4);
        (PathSetting { button, clear, caption }, cell)
    }

    /// Select... or Change... with the path as tooltip; the trash icon only when there is
    /// something to clear and `enabled`.
    pub fn show(&self, path: Option<&Path>, enabled: bool) {
        self.button.set_label(if path.is_some() { "Change..." } else { "Select..." });
        self.button.set_tooltip(&path.map(|p| p.display().to_string()).unwrap_or_default());
        self.clear.enable(path.is_some() && enabled);
    }

    /// The caption: the path, cut in the middle so it is no wider than the button above it.
    /// `row_width` is the width of the row the button and its trash icon share.
    pub fn set_caption(&self, path: Option<&Path>, row_width: i32) {
        let button = row_width - self.clear.get_size().width - GAP;
        self.caption.set_foreground_color(muted(&self.caption));
        let text = path.map(|p| p.display().to_string()).unwrap_or_default();
        self.caption.set_label(&fit(&self.caption, &text, (button - CAPTION_INSET).max(60)));
    }
}

/// A caption with the standard inset on its left.
pub fn indented(caption: &StaticText) -> BoxSizer {
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    row.add_spacer(CAPTION_INSET);
    row.add(caption, 1, SizerFlag::Expand, 0);
    row
}

/// The last component of a path, or the whole path if it has none.
pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// `text` cut in the middle so it is at most `max_px` wide in the font of `widget`; the
/// full text when it fits, a blank when empty (keeps the line's height).
pub fn fit(widget: &StaticText, text: &str, max_px: i32) -> String {
    if text.is_empty() {
        return " ".to_string();
    }
    let width = |t: &str| widget.get_text_extent(t).width;
    if width(text) <= max_px {
        return text.to_string();
    }
    // the most characters (counting the "...") that still fit
    let (mut lo, mut hi) = (1, text.chars().count());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if width(&shorten(text, mid)) <= max_px { lo = mid } else { hi = mid - 1 }
    }
    shorten(text, lo)
}

/// Shortens a long path in the middle: "/Users/me/.../C64Music" (char-safe).
pub fn shorten(path: &str, max: usize) -> String {
    let c: Vec<char> = path.chars().collect();
    if c.len() <= max {
        return path.to_string();
    }
    let keep = max.saturating_sub(3);
    let (head, tail) = (keep / 3, keep - keep / 3);
    format!("{}...{}", c[..head].iter().collect::<String>(), c[c.len() - tail..].iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_keeps_short_text_and_cuts_the_middle_of_long_text() {
        assert_eq!(shorten("/short", 20), "/short");
        let long = "/Users/someone/Music/HVSC/C64Music";
        let cut = shorten(long, 20);
        assert_eq!(cut.chars().count(), 20);
        assert_eq!(cut, "/User...VSC/C64Music"); // 20 characters: 5 kept at the start, 12 at the end
        assert_eq!(shorten("ääääääääää", 6).chars().count(), 6, "must not split a multi-byte character");
    }

    #[test]
    fn baseline_offset_puts_the_text_baseline_on_the_bottom_edge() {
        // a 30 px logo, 14 px tall text with 3 px below its baseline: the text box starts at
        // 30 - (14 - 3) = 19, so its baseline (box top + 11) lands on 30
        assert_eq!(baseline_offset(30, 14, 3), 19);
        assert_eq!(baseline_offset(30, 14, 0), 16);
        assert_eq!(baseline_offset(5, 14, 3), 0, "never negative");
    }

    #[test]
    fn file_name_falls_back_to_the_whole_path() {
        assert_eq!(file_name(Path::new("/a/b/demo.prg")), "demo.prg");
        assert_eq!(file_name(Path::new("/")), "/");
    }
}
