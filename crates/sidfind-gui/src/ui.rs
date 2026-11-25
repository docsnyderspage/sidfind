//! The main window. All state lives on the UI thread; background work goes through `job`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sidfind_core::api::CANCELLED;
use sidfind_core::{Identification, ScanSettings, Source};
use wxdragon::prelude::*;

use crate::job::{Job, Kind, Outcome};
use crate::theme::{self, Applied, Theme};

const PAD: i32 = 12;
/// Indent of the path captions: lines them up with the text inside the field and buttons.
const CAPTION_INSET: i32 = 6;
const TITLE: &str = "SidFind";
/// Poll interval for the running job, in milliseconds.
const TICK_MS: i32 = 100;
// ids of the entries of the theme menu
/// The wordmark with its text converted to outlines (assets/logo-outlined.svg), so it needs
/// no font; the original assets/logo.svg uses live text.
const LOGO_SVG: &str = include_str!("../../../assets/logo-outlined.svg");
/// Logical size in the header, keeping the aspect ratio of the SVG (163.65 x 31.81).
const LOGO_HEIGHT: i32 = 40;
const LOGO_WIDTH: i32 = LOGO_HEIGHT * 16365 / 3181;
const ID_THEME_SYSTEM: i32 = 7001;
const ID_THEME_LIGHT: i32 = 7002;
const ID_THEME_DARK: i32 = 7003;

struct State {
    /// The chosen PRG/VSF. The field only shows its file name, so the full path lives here.
    source: Option<PathBuf>,
    hvsc: Option<PathBuf>,
    index: Option<PathBuf>,
    /// Scan mode: true = on-the-fly (read HVSC), false = through the index.
    on_the_fly: bool,
    /// Held in the state so a later options window can edit it; v1 always uses the defaults.
    settings: ScanSettings,
    job: Option<Job>,
    gauge_shown: bool,
    results_shown: bool,
    theme: Theme,
    /// Colour the logo was last drawn in, so it is only rendered again when it changes.
    logo_colour: String,
}

/// A secondary path setting: a Select/Change button, a trash icon and a caption with the path.
#[derive(Clone, Copy)]
struct Setting {
    button: Button,
    clear: BitmapButton,
    caption: StaticText,
}

impl Setting {
    fn show(&self, path: Option<&Path>, enabled: bool) {
        self.button.set_label(if path.is_some() { "Change..." } else { "Select..." });
        self.button.set_tooltip(&path.map(|p| p.display().to_string()).unwrap_or_default());
        self.clear.enable(path.is_some() && enabled);
    }
}

#[derive(Clone)]
struct Ui {
    frame: Frame,
    panel: Panel,
    theme_button: BitmapButton,
    logo: StaticBitmap,
    source: TextCtrl,
    source_browse: Button,
    source_clear: BitmapButton,
    source_caption: StaticText,
    hvsc: Setting,
    index: Setting,
    radio_index: RadioButton,
    radio_fly: RadioButton,
    hint: StaticText,
    build: Button,
    scan: Button,
    gauge: Gauge,
    results: TextCtrl,
    timer: Rc<Timer<Frame>>,
    state: Rc<RefCell<State>>,
}

pub fn build(theme: Theme) {
    let frame = Frame::builder().with_title(TITLE).with_size(Size::new(640, 640)).build();
    let panel = Panel::builder(&frame).build();
    let root = BoxSizer::builder(Orientation::Vertical).build();

    // header: the wordmark over the subtitle. The logo is drawn in the text colour of the
    // current theme (see `refresh_logo`), so it is visible on light and dark backgrounds.
    let head = BoxSizer::builder(Orientation::Horizontal).build();
    // StaticBitmap cannot exist without a bitmap: start with an empty one, `refresh_logo` draws the real logo
    let logo = StaticBitmap::builder(&panel).with_bitmap(Bitmap::from_rgba(&[0, 0, 0, 0], 1, 1)).build();
    let titles = BoxSizer::builder(Orientation::Vertical).build();
    let subtitle = StaticText::builder(&panel).with_label("By Doc Snyder").build();
    titles.add_spacer(12); // air above the logo
    titles.add(&logo, 0, SizerFlag::Bottom, 8);
    titles.add(&subtitle, 0, SizerFlag::empty(), 0);
    head.add_sizer(&titles, 1, SizerFlag::AlignCenterVertical, 0);
    // theme menu button, top right
    let theme_icon = BitmapBundle::from_svg_text(THEME_SVG, Size::new(20, 20)).and_then(|b| b.get_bitmap_for(&panel));
    let theme_button = BitmapButton::builder(&panel).with_bitmap(theme_icon).build();
    theme_button.set_tooltip("Appearance");
    head.add(&theme_button, 0, SizerFlag::empty(), 0);
    root.add_sizer(&head, 0, SizerFlag::Expand | SizerFlag::All, PAD);

    // The trash icon is an inline SVG: crisp at any scale, no font or image files. It is drawn
    // in a BitmapButton because a plain Button with an empty label ignores its bitmap on macOS.
    let trash = BitmapBundle::from_svg_text(&trash_svg(1.0), Size::new(16, 16));
    let trash_dim = BitmapBundle::from_svg_text(&trash_svg(0.3), Size::new(16, 16));
    let icon_on = trash.as_ref().and_then(|b| b.get_bitmap_for(&panel));
    let icon_off = trash_dim.as_ref().and_then(|b| b.get_bitmap_for(&panel));
    let trash_button = |tip: &str| {
        let b = BitmapButton::builder(&panel).with_bitmap(icon_on.clone()).with_bitmap_disabled(icon_off.clone()).build();
        b.set_tooltip(tip);
        b.enable(false);
        b
    };

    // primary action: the source file
    let source_label = StaticText::builder(&panel).with_label("Source file").build();
    if let Some(mut font) = source_label.get_font() {
        font.make_bold();
        source_label.set_font(&font);
    }
    // display-only: shows the file name, the full path is the tooltip; set through Browse
    let source = TextCtrl::builder(&panel).with_style(TextCtrlStyle::ReadOnly).build();
    let source_browse = Button::builder(&panel).with_label("Browse...").build();
    let source_clear = trash_button("Clear source file");
    let source_row = BoxSizer::builder(Orientation::Horizontal).build();
    source_row.add(&source, 1, SizerFlag::Expand | SizerFlag::AlignCenterVertical, 0);
    source_row.add(&source_browse, 0, SizerFlag::AlignCenterVertical | SizerFlag::Left, 8);
    source_row.add(&source_clear, 0, SizerFlag::AlignCenterVertical | SizerFlag::Left, 8);
    root.add(&source_label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, PAD);
    root.add_spacer(6);
    root.add_sizer(&source_row, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, PAD);
    // full path under the field, like the captions of the HVSC and index settings
    let source_caption = StaticText::builder(&panel).with_label(" ").build();
    source_caption.set_foreground_color(SystemSettings::get_colour(SystemColour::GrayText));
    root.add_spacer(4);
    root.add_sizer(&indented(&source_caption), 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, PAD);
    root.add_spacer(PAD);

    // secondary settings share one row; no text fields, the button says Select.../Change...
    let (hvsc, hvsc_cell) = setting_cell(&panel, "HVSC folder", &trash_button("Clear HVSC folder"));
    let (index, index_cell) = setting_cell(&panel, "Index file", &trash_button("Clear index file"));
    let settings_row = BoxSizer::builder(Orientation::Horizontal).build();
    settings_row.add_sizer(&hvsc_cell, 1, SizerFlag::Expand | SizerFlag::Right, PAD);
    settings_row.add_sizer(&index_cell, 1, SizerFlag::Expand, 0);
    root.add_sizer(&settings_row, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, PAD);

    // scan mode
    let mode = BoxSizer::builder(Orientation::Horizontal).build();
    let mode_label = StaticText::builder(&panel).with_label("Scan mode:").build();
    let radio_index = RadioButton::builder(&panel).with_label("Indexed (fast)").first_in_group().build();
    let radio_fly = RadioButton::builder(&panel).with_label("On-the-fly (scan HVSC)").build();
    mode.add(&mode_label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 16);
    mode.add(&radio_index, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 16);
    mode.add(&radio_fly, 0, SizerFlag::AlignCenterVertical, 0);
    root.add_sizer(&mode, 0, SizerFlag::Expand | SizerFlag::All, PAD);

    // actions: Build index left, a hint on why Scan is disabled in the middle, Scan right
    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    let build = Button::builder(&panel).with_label("Build index...").build();
    let hint = StaticText::builder(&panel).with_label(" ").build();
    // normal text colour, not the grey of the captions: the hint explains why Scan is disabled
    // and must be noticed
    let scan = Button::builder(&panel).with_label("Scan").build();
    actions.add(&build, 0, SizerFlag::AlignCenterVertical, 0);
    actions.add_stretch_spacer(1);
    actions.add(&hint, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, PAD);
    actions.add(&scan, 0, SizerFlag::AlignCenterVertical, 0);
    root.add_sizer(&actions, 0, SizerFlag::Expand | SizerFlag::All, PAD);

    // progress and results stay hidden (no empty space) until first needed
    let gauge = Gauge::builder(&panel).with_range(100).build();
    root.add(&gauge, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, PAD);
    let results = TextCtrl::builder(&panel)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
        .with_size(Size::new(-1, 160))
        .build();
    root.add(&results, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, PAD);
    gauge.show(false);
    results.show(false);

    let env = |name| std::env::var_os(name).map(PathBuf::from);
    let (env_hvsc, env_index) = (env("SIDFIND_HVSC"), env("SIDFIND_INDEX"));
    let state = State {
        source: None,
        on_the_fly: env_index.is_none() && env_hvsc.is_some(),
        hvsc: env_hvsc,
        index: env_index,
        settings: ScanSettings::default(),
        job: None,
        gauge_shown: false,
        results_shown: false,
        theme,
        logo_colour: String::new(),
    };
    radio_index.set_value(!state.on_the_fly);
    radio_fly.set_value(state.on_the_fly);

    let ui = Ui {
        frame,
        panel,
        theme_button,
        logo,
        source,
        source_browse,
        source_clear,
        source_caption,
        hvsc,
        index,
        radio_index,
        radio_fly,
        hint,
        build,
        scan,
        gauge,
        results,
        timer: Rc::new(Timer::new(&frame)),
        state: Rc::new(RefCell::new(state)),
    };
    ui.connect();
    ui.sync(); // also draws the logo

    panel.set_sizer(root, true);
    frame.fit(); // size to content: the hidden gauge and results take no space
    let best = frame.get_size();
    frame.set_size(Size::new(640, best.height));
    frame.set_min_size(Size::new(520, best.height));
    frame.show(true);
    frame.centre();
}

impl Ui {
    fn connect(&self) {
        let ui = self.clone();
        self.timer.on_tick(move |_| ui.tick());

        // the captions are cut to the available width, so redo them when the window is resized
        let ui = self.clone();
        self.panel.on_size(move |event| {
            ui.refresh_captions();
            event.skip(true); // the panel's own layout must still run
        });
        let ui = self.clone();
        self.source_clear.on_click(move |_| {
            ui.state.borrow_mut().source = None;
            ui.sync();
        });
        let ui = self.clone();
        self.source_browse.on_click(move |_| {
            let dialog = FileDialog::builder(&ui.frame)
                .with_message("Select a PRG file or VICE snapshot")
                .with_wildcard("C64 programs|*.prg;*.PRG;*.vsf;*.VSF|All files|*")
                .build();
            if dialog.show_modal() == ID_OK {
                if let Some(path) = dialog.get_path() {
                    ui.state.borrow_mut().source = Some(PathBuf::from(path));
                    ui.sync();
                }
            }
        });

        let ui = self.clone();
        self.hvsc.button.on_click(move |_| {
            let dialog = DirDialog::builder(&ui.frame, "Select the HVSC folder", "").build();
            if dialog.show_modal() == ID_OK {
                ui.state.borrow_mut().hvsc = dialog.get_path().map(PathBuf::from);
                ui.sync();
            }
        });
        let ui = self.clone();
        self.hvsc.clear.on_click(move |_| {
            ui.state.borrow_mut().hvsc = None;
            ui.sync();
        });

        let ui = self.clone();
        self.index.button.on_click(move |_| {
            let dialog = FileDialog::builder(&ui.frame)
                .with_message("Select the index file")
                .with_wildcard("Index files|*.idx|All files|*")
                .build();
            if dialog.show_modal() == ID_OK {
                ui.state.borrow_mut().index = dialog.get_path().map(PathBuf::from);
                ui.sync();
            }
        });
        let ui = self.clone();
        self.index.clear.on_click(move |_| {
            ui.state.borrow_mut().index = None;
            ui.sync();
        });

        let ui = self.clone();
        self.radio_index.on_selected(move |_| {
            ui.state.borrow_mut().on_the_fly = false;
            ui.sync();
        });
        let ui = self.clone();
        self.radio_fly.on_selected(move |_| {
            ui.state.borrow_mut().on_the_fly = true;
            ui.sync();
        });

        let ui = self.clone();
        self.frame.on_activate(move |event| {
            ui.sync(); // the system theme may have changed while the window was in the background
            event.skip(true);
        });
        let ui = self.clone();
        self.theme_button.on_click(move |_| ui.show_theme_menu());
        let ui = self.clone();
        self.frame.on_menu_selected(move |event| ui.on_theme_chosen(event.get_id()));

        let ui = self.clone();
        self.build.on_click(move |_| ui.on_build());
        let ui = self.clone();
        self.scan.on_click(move |_| ui.on_scan());
    }

    /// Brings every widget in line with the state. The only place that enables or disables.
    fn sync(&self) {
        let (source, hvsc, index, on_the_fly, running) = {
            let st = self.state.borrow();
            (st.source.clone(), st.hvsc.clone(), st.index.clone(), st.on_the_fly, st.job.as_ref().map(|j| j.kind))
        };
        let idle = running.is_none();
        let have_source = source.is_some();
        let shown = source.as_deref().map(file_name).unwrap_or_default();
        if self.source.get_value() != shown {
            self.source.set_value(&shown);
        }
        self.source.set_tooltip(&source.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
        let have_tunes = if on_the_fly { hvsc.is_some() } else { index.is_some() };

        self.hvsc.show(hvsc.as_deref(), idle);
        self.index.show(index.as_deref(), idle);
        self.source_clear.enable(idle && have_source);
        for control in [&self.source as &dyn WxWidget, &self.source_browse, &self.hvsc.button, &self.index.button] {
            control.enable(idle);
        }
        self.radio_index.enable(idle);
        self.radio_fly.enable(idle);

        self.build.set_label(if running == Some(Kind::Build) { "Cancel" } else { "Build index..." });
        self.build.enable(running == Some(Kind::Build) || (idle && hvsc.is_some()));
        self.scan.set_label(if running == Some(Kind::Scan) { "Cancel" } else { "Scan" });
        self.scan.enable(running == Some(Kind::Scan) || (idle && have_source && have_tunes));

        let hint = match (idle, have_source, have_tunes, on_the_fly) {
            (false, ..) => "",
            (_, false, ..) => "Choose a source file",
            (_, _, false, true) => "Select the HVSC folder",
            (_, _, false, false) => "Select an index, or build one",
            _ => "",
        };
        self.hint.set_label(if hint.is_empty() { " " } else { hint });
        self.refresh_captions();
        self.refresh_logo();
        self.panel.layout();
    }

    /// Draws the logo in the current text colour, when that changed since the last time.
    fn refresh_logo(&self) {
        let c = SystemSettings::get_colour(SystemColour::WindowText);
        let colour = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
        if self.state.borrow().logo_colour == colour {
            return;
        }
        // the SVG has no fill of its own (black by default); give its path one
        let svg = LOGO_SVG.replacen("<path", &format!("<path fill=\"{colour}\""), 1);
        let size = Size::new(LOGO_WIDTH, LOGO_HEIGHT);
        if let Some(bitmap) = BitmapBundle::from_svg_text(&svg, size).and_then(|b| b.get_bitmap_for(&self.panel)) {
            self.logo.set_bitmap(&bitmap);
            self.state.borrow_mut().logo_colour = colour;
        }
    }

    /// Sets the path captions, each cut in the middle to fit the width it has.
    fn refresh_captions(&self) {
        let (source, hvsc, index) = {
            let st = self.state.borrow();
            (st.source.clone(), st.hvsc.clone(), st.index.clone())
        };
        // Each caption is at most as wide as the control above it: the field, or the button
        // (without its trash icon). The panel size event fires before the sizer has laid out
        // the children, so work from the panel width and the controls that have a fixed width.
        const GAP: i32 = 8; // between a field/button and the icons beside it
        let panel = self.panel.get_client_size().width;
        let field = panel - 2 * PAD - self.source_browse.get_size().width - self.source_clear.get_size().width - 2 * GAP;
        let button = (panel - 3 * PAD) / 2 - self.hvsc.clear.get_size().width - GAP;
        let full = (field - CAPTION_INSET).max(60);
        let half = (button - CAPTION_INSET).max(60);
        let text = |p: &Option<PathBuf>| p.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let grey = SystemSettings::get_colour(SystemColour::GrayText);
        for caption in [&self.source_caption, &self.hvsc.caption, &self.index.caption] {
            caption.set_foreground_color(grey);
        }
        self.source_caption.set_label(&fit(&self.source_caption, &text(&source), full));
        self.hvsc.caption.set_label(&fit(&self.hvsc.caption, &text(&hvsc), half));
        self.index.caption.set_label(&fit(&self.index.caption, &text(&index), half));
    }

    fn show_theme_menu(&self) {
        let current = match self.state.borrow().theme {
            Theme::System => ID_THEME_SYSTEM,
            Theme::Light => ID_THEME_LIGHT,
            Theme::Dark => ID_THEME_DARK,
        };
        let mut menu = Menu::builder()
            .append_radio_item(ID_THEME_SYSTEM, "Follow system", "")
            .append_radio_item(ID_THEME_LIGHT, "Light", "")
            .append_radio_item(ID_THEME_DARK, "Dark", "")
            .build();
        menu.check_item(current, true);
        self.theme_button.popup_menu(&mut menu, None);
    }

    fn on_theme_chosen(&self, id: i32) {
        let chosen = match id {
            ID_THEME_SYSTEM => Theme::System,
            ID_THEME_LIGHT => Theme::Light,
            ID_THEME_DARK => Theme::Dark,
            _ => return,
        };
        self.state.borrow_mut().theme = chosen;
        theme::save(chosen);
        let note = match theme::apply_now(chosen) {
            Applied::Now => None,
            Applied::OnRestart => Some("The theme changes the next time SidFind starts."),
            Applied::Unsupported => Some("This system does not allow changing the theme from SidFind."),
        };
        if let Some(note) = note {
            let dialog = MessageDialog::builder(&self.frame, note, TITLE)
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
            dialog.show_modal();
        }
        self.sync(); // refreshes the caption colours, which are fixed at the time they are set
    }

    fn on_build(&self) {
        if let Some(job) = &self.state.borrow().job {
            job.cancel();
            return;
        }
        let Some(hvsc) = self.state.borrow().hvsc.clone() else { return };
        // default: the per-user data folder, so a new HVSC release can't wipe the index
        let dir = StandardPaths::user_data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let dialog = FileDialog::builder(&self.frame)
            .with_message("Save the index as")
            .with_default_dir(&dir)
            .with_default_file("hvsc.idx")
            .with_wildcard("Index files|*.idx|All files|*")
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(path) = dialog.get_path().map(PathBuf::from) else { return };
        self.start(Job::build(hvsc, path), true);
    }

    fn on_scan(&self) {
        if let Some(job) = &self.state.borrow().job {
            job.cancel();
            return;
        }
        let (file, source, settings, on_the_fly) = {
            let st = self.state.borrow();
            let source = if st.on_the_fly { st.hvsc.clone().map(Source::Hvsc) } else { st.index.clone().map(Source::Index) };
            (st.source.clone(), source, st.settings.clone(), st.on_the_fly)
        };
        let (Some(file), Some(source)) = (file, source) else { return };
        self.start(Job::scan(file, source, settings), on_the_fly);
    }

    /// `show_progress`: the job reports progress (an index lookup is too quick for a bar).
    fn start(&self, job: Job, show_progress: bool) {
        if show_progress && !self.state.borrow().gauge_shown {
            self.state.borrow_mut().gauge_shown = true;
            self.gauge.show(true);
            self.grow(36);
        }
        self.gauge.set_value(0);
        self.results.set_value("");
        self.state.borrow_mut().job = Some(job);
        self.timer.start(TICK_MS, false);
        self.sync();
    }

    /// Called by the timer while a job runs: moves the bar and picks up the result.
    fn tick(&self) {
        let (percent, result) = match &self.state.borrow().job {
            Some(job) => (job.percent(), job.take_result()),
            None => return,
        };
        self.gauge.set_value(percent);
        let Some(result) = result else { return };

        self.timer.stop();
        self.state.borrow_mut().job = None;
        self.gauge.set_value(if result.is_ok() { 100 } else { 0 });
        let text = match result {
            Ok(Outcome::Built { path, summary }) => {
                self.state.borrow_mut().index = Some(path.clone());
                format!(
                    "Index built: {} tunes, {:.0} MB\n{}\n\nIt is selected as the index file; use Indexed mode to scan with it.",
                    summary.tunes,
                    summary.bytes as f64 / 1e6,
                    path.display()
                )
            }
            Ok(Outcome::Scanned(found)) => found.iter().map(describe).collect::<Vec<_>>().join("\n"),
            Err(e) if e == CANCELLED => "Cancelled.".to_string(),
            Err(e) => {
                let dialog = MessageDialog::builder(&self.frame, &e, TITLE)
                    .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconError)
                    .build();
                dialog.show_modal();
                format!("Error: {e}")
            }
        };
        if !self.state.borrow().results_shown {
            self.state.borrow_mut().results_shown = true;
            self.results.show(true);
            self.grow(176);
        }
        self.results.set_value(&text);
        self.sync();
    }

    /// Makes the window taller by `dy` when something that was hidden first appears.
    fn grow(&self, dy: i32) {
        self.panel.layout();
        let size = self.frame.get_size();
        self.frame.set_size(Size::new(size.width, size.height + dy));
    }
}

/// A caption with the standard inset on its left.
fn indented(caption: &StaticText) -> BoxSizer {
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    row.add_spacer(CAPTION_INSET);
    row.add(caption, 1, SizerFlag::Expand, 0);
    row
}

/// A label above a Select/Change button + trash icon + a caption for the chosen path.
fn setting_cell(panel: &Panel, label: &str, clear: &BitmapButton) -> (Setting, BoxSizer) {
    let label = StaticText::builder(panel).with_label(label).build();
    let button = Button::builder(panel).with_label("Select...").build();
    let caption = StaticText::builder(panel).with_label(" ").build();
    caption.set_foreground_color(SystemSettings::get_colour(SystemColour::GrayText));
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    row.add(&button, 1, SizerFlag::Expand | SizerFlag::AlignCenterVertical, 0);
    row.add(clear, 0, SizerFlag::AlignCenterVertical | SizerFlag::Left, 8);
    let cell = BoxSizer::builder(Orientation::Vertical).build();
    cell.add(&label, 0, SizerFlag::Bottom, 6);
    cell.add_sizer(&row, 0, SizerFlag::Expand, 0);
    cell.add_sizer(&indented(&caption), 0, SizerFlag::Expand | SizerFlag::Top, 4);
    (Setting { button, clear: *clear, caption }, cell)
}

/// One result block, in the same wording as the command line tool.
fn describe(found: &Identification) -> String {
    let name = file_name(&found.path);
    if found.matches.is_empty() {
        return format!(
            "{name}\n   No match ({} distinct windows checked; {}).\n   The tune may be packed, relocated or not in HVSC.",
            found.windows, found.emulation
        );
    }
    let mut out = name;
    for m in &found.matches {
        out.push_str(&format!("\n   {:>5.1}% of tune data found, score {:>7.1}  /{}", 100.0 * m.share, m.score, m.name));
    }
    out
}

/// The last component of a path, or the whole path if it has none.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// `text` cut in the middle so it is at most `max_px` wide in the font of `widget`; the
/// full text when it fits, a blank when empty (keeps the line's height).
fn fit(widget: &StaticText, text: &str, max_px: i32) -> String {
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
fn shorten(path: &str, max: usize) -> String {
    let c: Vec<char> = path.chars().collect();
    if c.len() <= max {
        return path.to_string();
    }
    let keep = max.saturating_sub(3);
    let (head, tail) = (keep / 3, keep - keep / 3);
    format!("{}...{}", c[..head].iter().collect::<String>(), c[c.len() - tail..].iter().collect::<String>())
}

/// Half-filled circle ("appearance"), same grey as the trash icon.
const THEME_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#8e8e93" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 3a9 9 0 0 1 0 18z" fill="#8e8e93"/></svg>"##;

/// Trash-can icon as inline SVG. Mid-grey reads on both light and dark backgrounds.
fn trash_svg(opacity: f32) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#8e8e93" stroke-opacity="{opacity}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14M10 10v6M14 10v6"/></svg>"##
    )
}
