//! Small widgets and helpers shared by the main window and the settings window.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use wxdragon::prelude::*;

pub const PAD: i32 = 12;
/// Gap between a field or button and the icons beside it.
pub const GAP: i32 = 8;
/// Indent of the path captions: lines them up with the text inside the field and buttons.
pub const CAPTION_INSET: i32 = 6;
pub const TITLE: &str = "SidFind";

/// The grey of the icon SVGs (the colour they draw in at rest). Hover swaps it for the text colour.
pub const ICON_GREY: &str = "#8e8e93";
/// Opacity of an icon that is disabled.
const DISABLED_OPACITY: f32 = 0.3;

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

/// The grey for secondary text on `window`: the text colour blended toward the background, so it
/// stays readable on both themes. The system grey (GrayText) is too faint on dark and too bright on light.
pub fn muted(window: &dyn WxWidget) -> Colour {
    let background = window.get_background_color();
    let text = SystemSettings::get_colour(SystemColour::WindowText);
    if luma(background) < 128.0 { mix(text, background, 0.3) } else { mix(text, background, 0.5) }
}

/// Vertical offset that puts the baseline of a line of text on the bottom edge of something
/// `above_height` tall: the text box is `text_height` high, of which `descent` is below its baseline.
pub fn baseline_offset(above_height: i32, text_height: i32, descent: i32) -> i32 {
    (above_height - (text_height - descent)).max(0)
}

/// Info icon ("i" in an outlined circle), same grey.
pub const INFO_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#8e8e93" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>"##;

/// Trash-can icon as inline SVG. Mid-grey reads on both light and dark backgrounds.
fn trash_svg(colour: &str, opacity: f32) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="{colour}" stroke-opacity="{opacity}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14M10 10v6M14 10v6"/></svg>"##
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

/// A trash-can icon as the clear control: dimmed while disabled, and while enabled it turns the
/// text colour on hover with a hand cursor, like the other icons. A StaticBitmap, because a
/// BitmapButton cannot change its bitmap after it is created.
#[derive(Clone)]
pub struct TrashButton {
    icon: StaticBitmap,
    enabled: Rc<Cell<bool>>,
}

/// The trash icon in `colour` (an SVG colour) at `opacity`.
fn trash_bitmap(window: &dyn WxWidget, colour: &str, opacity: f32) -> Option<Bitmap> {
    BitmapBundle::from_svg_text(&trash_svg(colour, opacity), Size::new(16, 16)).and_then(|b| b.get_bitmap_for(window))
}

impl TrashButton {
    /// Starts disabled.
    pub fn new(panel: &Panel, tip: &str) -> Self {
        let icon = StaticBitmap::builder(panel).with_bitmap(trash_bitmap(panel, ICON_GREY, DISABLED_OPACITY)).build();
        icon.set_tooltip(tip);
        let enabled = Rc::new(Cell::new(false));
        let on_enter = (icon, enabled.clone());
        icon.on_mouse_enter(move |_| {
            let (icon, enabled) = &on_enter;
            if enabled.get() {
                let ink = SystemSettings::get_colour(SystemColour::WindowText);
                let hex = format!("#{:02x}{:02x}{:02x}", ink.r, ink.g, ink.b);
                if let Some(hover) = trash_bitmap(icon, &hex, 1.0) {
                    icon.set_bitmap(&hover);
                }
                if let Some(hand) = Cursor::from_stock(StockCursor::Hand) {
                    icon.set_cursor(Some(&hand));
                }
            }
        });
        let on_leave = icon;
        icon.on_mouse_leave(move |_| {
            if let Some(grey) = trash_bitmap(&on_leave, ICON_GREY, 1.0) {
                on_leave.set_bitmap(&grey);
            }
            on_leave.set_cursor(None);
        });
        TrashButton { icon, enabled }
    }

    /// The control as a window, for sizers and sizing.
    pub fn widget(&self) -> &StaticBitmap {
        &self.icon
    }

    /// Enables or disables the control (disabled: dimmed, no hover, no clicks).
    pub fn enable(&self, on: bool) {
        self.enabled.set(on);
        let opacity = if on { 1.0 } else { DISABLED_OPACITY };
        if let Some(bitmap) = trash_bitmap(&self.icon, ICON_GREY, opacity) {
            self.icon.set_bitmap(&bitmap);
        }
    }

    /// Runs `f` on a click, but only while enabled.
    pub fn on_click(&self, f: impl Fn() + 'static) {
        let enabled = self.enabled.clone();
        self.icon.on_mouse_left_up(move |_| {
            if enabled.get() {
                f();
            }
        });
    }
}

/// Makes an icon button at least `side` pixels square, so it lines up with the fields and buttons
/// beside it (Windows sizes a bitmap button to its icon; macOS pads it, which stays as it is).
/// Whether `path` is a file the scan takes as a source: a PRG or a VICE snapshot (VSF), in any case.
pub fn is_source_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("prg") || e.eq_ignore_ascii_case("vsf"))
}

/// A read-only field showing one line of text, vertically centred. It is a bordered panel with a
/// static text inside: a native text control draws its text at the top of a taller box (macOS).
#[derive(Clone)]
pub struct TextField {
    pub panel: Panel,
    pub text: StaticText,
}

impl TextField {
    pub fn new(parent: &Panel) -> Self {
        let panel = Panel::builder(parent).with_style(PanelStyle::BorderNone).build();
        let text = StaticText::builder(&panel).with_label(" ").build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&text, 1, SizerFlag::AlignCenterVertical | SizerFlag::Left | SizerFlag::Right, GAP);
        panel.set_sizer(row, true);
        TextField { panel, text }
    }

    /// The shown text: cut to the field's width.
    pub fn set_value(&self, value: &str) {
        let width = self.panel.get_size().width - 2 * GAP;
        self.text.set_label(&fit(&self.text, value, width));
    }

    pub fn get_value(&self) -> String {
        self.text.get_label()
    }

    pub fn set_tooltip(&self, tip: &str) {
        self.panel.set_tooltip(tip);
        self.text.set_tooltip(tip);
    }

    pub fn set_background_color(&self, colour: Colour) {
        self.panel.set_background_color(colour);
        self.text.set_background_color(colour);
    }

    pub fn set_foreground_color(&self, colour: Colour) {
        self.text.set_foreground_color(colour);
    }

    pub fn refresh(&self) {
        self.panel.refresh(true, None);
        self.text.refresh(true, None);
    }
}

/// The height of input fields and buttons: 25% taller than the platform default.
pub fn tall(window: &dyn WxWidget) -> i32 {
    window.get_best_size().height * 5 / 4
}

/// Gives a field or button the height of [`tall`].
pub fn grow(window: &dyn WxWidget) {
    window.set_min_size(Size::new(-1, tall(window)));
}

pub fn square(window: &dyn WxWidget, side: i32) {
    window.set_min_size(Size::new(side, side));
}

/// Makes a label bold (the look of a section label).
/// Colour of the progress bars: the blue of the macOS default button (the "Find SID" look). The
/// system highlight colour is a pale selection blue on macOS, so it is not used here.
pub fn bar_colour() -> Colour {
    Colour::rgb(0x34, 0x78, 0xf6)
}

/// A progress bar drawn by the app. Windows draws its own gauges green and animates them, and it
/// ignores the colour set on them, so the bar is painted here: the accent colour on a track.
#[derive(Clone)]
pub struct Bar {
    pub panel: Panel,
    value: Rc<Cell<i32>>,
}

impl Bar {
    pub fn new(parent: &dyn WxWidget, size: Size) -> Self {
        let panel = Panel::builder(parent).with_size(size).build();
        panel.set_min_size(size);
        panel.set_background_style(BackgroundStyle::Paint);
        let value = Rc::new(Cell::new(0));
        let (painted, shown) = (panel.clone(), value.clone());
        panel.on_paint(move |_| {
            let dc = AutoBufferedPaintDC::new(&painted);
            let size = painted.get_size();
            let track = mix(SystemSettings::get_colour(SystemColour::WindowText), SystemSettings::get_colour(SystemColour::Window), 0.85);
            dc.set_pen(track, 1, PenStyle::Solid);
            dc.set_brush(track, BrushStyle::Solid);
            dc.draw_rectangle(0, 0, size.width, size.height);
            let filled = size.width * shown.get().clamp(0, 100) / 100;
            dc.set_pen(bar_colour(), 1, PenStyle::Solid);
            dc.set_brush(bar_colour(), BrushStyle::Solid);
            dc.draw_rectangle(0, 0, filled, size.height);
        });
        Bar { panel, value }
    }

    /// Sets the fill, 0 to 100 percent.
    pub fn set_value(&self, percent: i32) {
        self.value.set(percent);
        self.panel.refresh(false, None);
    }

    pub fn show(&self, on: bool) {
        self.panel.show(on);
    }
}

pub fn make_bold(text: &StaticText) {
    if let Some(mut font) = text.get_font() {
        font.make_bold();
        text.set_font(&font);
    }
}

/// A link that opens `url` in the browser: plain underlined text in the accent colour, with a hand
/// cursor. Not a button, so it has no border or focus frame and lines up with the text beside it.
pub fn hyperlink(panel: &Panel, text: &str, url: &str) -> StaticText {
    let link = StaticText::builder(panel).with_label(text).build();
    // the accent is pale on a light background (macOS selection blue), so links use a deeper blue there
    // dark mode is told by the system text colour (light text), not by the panel's own background
    let dark = luma(SystemSettings::get_colour(SystemColour::WindowText)) > 128.0;
    let colour = if dark { SystemSettings::get_colour(SystemColour::Highlight) } else { Colour::rgb(0, 84, 200) };
    link.set_foreground_color(colour);
    if let Some(mut font) = link.get_font() {
        font.set_underlined(true);
        link.set_font(&font);
    }
    if let Some(hand) = Cursor::from_stock(StockCursor::Hand) {
        link.set_cursor(Some(&hand));
    }
    link.set_tooltip(url);
    // hover: the text colour, as the icon buttons do
    let on_enter = link;
    link.on_mouse_enter(move |_| on_enter.set_foreground_color(SystemSettings::get_colour(SystemColour::WindowText)));
    let on_leave = link;
    link.on_mouse_leave(move |_| on_leave.set_foreground_color(colour));
    let url = url.to_string();
    link.on_mouse_left_up(move |_| {
        launch_default_browser(&url, BrowserLaunchFlags::Default);
    });
    link
}

/// A headline: bold and a few points larger than the text around it.
pub fn headline(panel: &Panel, text: &str) -> StaticText {
    let label = StaticText::builder(panel).with_label(text).build();
    if let Some(mut font) = label.get_font() {
        font.make_bold();
        font.set_point_size(font.get_point_size() + 4);
        label.set_font(&font);
    }
    label
}

/// A path setting: a Select/Change button, a trash icon and a grey caption with the path.
#[derive(Clone)]
pub struct PathSetting {
    pub button: Button,
    pub clear: TrashButton,
    caption: StaticText,
}

impl PathSetting {
    /// The setting with its label above; add the returned sizer to a layout. A non-empty
    /// `description` is shown in grey, and `link` (text, URL) as a link that opens in the
    /// default browser. The link sits at the right end of the label's line, the description
    /// on its own line below it; the button follows.
    pub fn new(panel: &Panel, label: &str, description: &str, link: Option<(&str, &str)>, clear_tip: &str) -> (PathSetting, BoxSizer) {
        let label = StaticText::builder(panel).with_label(label).build();
        make_bold(&label);
        let button = Button::builder(panel).with_label("Select...").build();
        grow(&button);
        let clear = TrashButton::new(panel, clear_tip);
        square(clear.widget(), tall(&button));
        let caption = StaticText::builder(panel).with_label(" ").build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&button, 1, SizerFlag::Expand | SizerFlag::AlignCenterVertical, 0);
        row.add(clear.widget(), 0, SizerFlag::AlignCenterVertical | SizerFlag::Left, GAP);
        let cell = BoxSizer::builder(Orientation::Vertical).build();
        // the label, with the link (if any) at the right end of the same line
        let header = BoxSizer::builder(Orientation::Horizontal).build();
        header.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
        header.add_stretch_spacer(1);
        if let Some((text, url)) = link {
            let link = hyperlink(panel, text, url);
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
        let button = row_width - self.clear.widget().get_size().width - GAP;
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
    fn only_prg_and_vsf_are_source_files_in_any_case() {
        assert!(is_source_file(Path::new("/a/demo.prg")));
        assert!(is_source_file(Path::new("/a/DEMO.PRG")));
        assert!(is_source_file(Path::new("/a/snap.Vsf")));
        assert!(!is_source_file(Path::new("/a/build.zip")));
        assert!(!is_source_file(Path::new("/a/song.sid")));
        assert!(!is_source_file(Path::new("/a/prg")), "no extension");
    }

    #[test]
    fn file_name_falls_back_to_the_whole_path() {
        assert_eq!(file_name(Path::new("/a/b/demo.prg")), "demo.prg");
        assert_eq!(file_name(Path::new("/")), "/");
    }
}
