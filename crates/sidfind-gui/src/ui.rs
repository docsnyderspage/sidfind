//! The main window. All state lives on the UI thread; background work goes through `job`.
//! Everything that is set up once (HVSC folder, index file) lives in `settings_window`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use sidfind_core::api::CANCELLED;
use sidfind_core::{ScanSettings, Source};
use wxdragon::prelude::*;

use crate::job::{Job, Outcome};
use crate::prefs::{Missing, Prefs};
use crate::results::ResultsView;
use crate::settings_window;
use crate::theme::{self, Applied, Theme};
use crate::widgets::{
    self, GAP, GEAR_SVG, INFO_SVG, PAD, THEME_SVG, TITLE, TextField, TrashButton, file_name, fit,
    icon_button, indented, is_source_file, luma, mix, muted, square,
};

/// Label of the main button.
const FIND: &str = "Find SID";
/// Poll interval for the running job, in milliseconds.
const TICK_MS: i32 = 100;
/// The logo (assets/logo.svg): shapes only, no text, so it needs no font. It fills its
/// drawing area, so the bottom of the letters is the bottom edge of the image.
const LOGO_SVG: &str = include_str!("../../../assets/logo.svg");
/// The app icon master (assets/icons/app.svg), shown in the title bar of the windows.
const APP_ICON_SVG: &str = include_str!("../../../assets/icons/app.svg");
/// The drawing area of the logo (its viewBox), which fixes its proportions.
const LOGO_VIEW: (f32, f32) = (165.53185, 19.608799);
/// Width of the logo in the header, in logical pixels; the height follows from the proportions.
const LOGO_WIDTH: i32 = 236;
/// Width of the window at startup. Wide enough that a typical result line (match share, score
/// and the tune's path in HVSC) fits without wrapping; it can still be resized.
const WINDOW_WIDTH: i32 = 820;
/// Height of the window before it is fitted to its content.
const START_HEIGHT: i32 = 800;
/// Narrowest the window can be made.
const MIN_WIDTH: i32 = 520;
// ids of the entries of the theme menu
const ID_THEME_SYSTEM: i32 = 7001;
const ID_THEME_LIGHT: i32 = 7002;
const ID_THEME_DARK: i32 = 7003;

struct State {
    /// The chosen PRG/VSF. The field only shows its file name, so the full path lives here.
    source: Option<PathBuf>,
    /// HVSC folder, index file and scan mode: saved between runs, edited in the settings window.
    prefs: Rc<RefCell<Prefs>>,
    /// Held in the state so a later options window can edit it; v1 always uses the defaults.
    settings: ScanSettings,
    /// The scan in progress, if any.
    job: Option<Job>,
    gauge_shown: bool,
    results_shown: bool,
    theme: Theme,
    /// The source field's normal background, restored after a drop hover.
    field_colour: Colour,
    /// Colour the logo was last drawn in, so it is only rendered again when it changes.
    logo_colour: String,
}

#[derive(Clone)]
struct Ui {
    frame: Frame,
    panel: Panel,
    settings_button: StaticBitmap,
    theme_button: StaticBitmap,
    about_button: StaticBitmap,
    logo: StaticBitmap,
    source: TextField,
    source_browse: Button,
    source_clear: TrashButton,
    source_caption: StaticText,
    radio_index: RadioButton,
    radio_fly: RadioButton,
    info: StaticBitmap,
    status: StaticText,
    scan: Button,
    gauge: widgets::Bar,
    results: ResultsView,
    timer: Rc<Timer<Frame>>,
    state: Rc<RefCell<State>>,
}

pub fn build(theme: Theme) {
    // generous on purpose: GTK lays the window out at its first show, and a window that is too short
    // squeezes the buttons ("Negative content height"). fit_height sets the final height afterwards.
    let frame = Frame::builder()
        .with_title(TITLE)
        .with_size(Size::new(WINDOW_WIDTH, START_HEIGHT))
        .build();
    // without this the title bar shows the generic icon (Windows does not use the exe icon there)
    if let Some(icon) = BitmapBundle::from_svg_text(APP_ICON_SVG, Size::new(32, 32))
        .and_then(|b| b.get_bitmap_for(&frame))
    {
        frame.set_icon(&icon);
    }
    // macOS quits with Cmd-Q through the Quit item that wxWidgets moves into the application
    // menu from this File menu; without a menu bar there is no such item.
    #[cfg(target_os = "macos")]
    {
        // wxWidgets moves the About and Quit items into the application menu
        let file = Menu::builder()
            .append_item(ID_ABOUT, "About SidFind", "")
            .append_item(ID_EXIT, "Quit SidFind\tCtrl+Q", "")
            .build();
        frame.set_menu_bar(MenuBar::builder().append(file, "File").build());
    }
    let panel = Panel::builder(&frame).build();
    let root = BoxSizer::builder(Orientation::Vertical).build();

    // header: the logo, drawn in the text colour of the current theme (see `refresh_logo`). The
    // version, build and author are in the About window (the info icon).
    let head = BoxSizer::builder(Orientation::Horizontal).build();
    // StaticBitmap cannot exist without a bitmap: start with an empty one, `refresh_logo` draws the real logo
    let logo = StaticBitmap::builder(&panel)
        .with_bitmap(Bitmap::from_rgba(&[0, 0, 0, 0], 1, 1))
        .build();
    let titles = BoxSizer::builder(Orientation::Vertical).build();
    titles.add_spacer(12); // air above the logo
    titles.add(&logo, 0, SizerFlag::Bottom, 8);
    head.add_sizer(&titles, 1, SizerFlag::AlignCenterVertical, 0);
    // icon buttons, top right: settings, appearance, about
    let settings_button = icon_button(&panel, GEAR_SVG, 20, "Settings");
    let theme_button = icon_button(&panel, THEME_SVG, 20, "Appearance");
    let about_button = icon_button(&panel, INFO_SVG, 20, "About SidFind");
    head.add(
        &settings_button,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        GAP,
    );
    head.add(
        &theme_button,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        GAP,
    );
    head.add(&about_button, 0, SizerFlag::AlignCenterVertical, 0);
    root.add_sizer(&head, 0, SizerFlag::Expand | SizerFlag::All, PAD);

    // primary action: the source file
    let source_label = StaticText::builder(&panel)
        .with_label("Source file")
        .build();
    if let Some(mut font) = source_label.get_font() {
        font.make_bold();
        source_label.set_font(&font);
    }
    // display-only: shows the file name, the full path is the tooltip; set through Browse
    let source_browse = Button::builder(&panel).with_label("Browse...").build();
    widgets::grow(&source_browse);
    let source = TextField::new(&panel);
    // the field, the Browse button and the icon buttons share one height
    let side = widgets::tall(&source_browse);
    source.panel.set_min_size(Size::new(-1, side));
    let source_clear = TrashButton::new(&panel, "Clear source file");
    square(source_clear.widget(), side);
    let source_row = BoxSizer::builder(Orientation::Horizontal).build();
    source_row.add(
        &source.panel,
        1,
        SizerFlag::Expand | SizerFlag::AlignCenterVertical,
        0,
    );
    source_row.add(
        &source_browse,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Left,
        GAP,
    );
    source_row.add(
        source_clear.widget(),
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Left,
        GAP,
    );
    root.add(
        &source_label,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        PAD,
    );
    root.add_spacer(6);
    root.add_sizer(
        &source_row,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        PAD,
    );
    // full path under the field
    let source_caption = StaticText::builder(&panel).with_label(" ").build();
    root.add_spacer(4);
    root.add_sizer(
        &indented(&source_caption),
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        PAD,
    );
    root.add_spacer(PAD);

    // scan mode
    let mode = BoxSizer::builder(Orientation::Horizontal).build();
    let mode_label = StaticText::builder(&panel).with_label("Scan mode:").build();
    let radio_index = RadioButton::builder(&panel)
        .with_label("Indexed (fast)")
        .first_in_group()
        .build();
    let radio_fly = RadioButton::builder(&panel)
        .with_label("On-the-fly (scan HVSC)")
        .build();
    mode.add(
        &mode_label,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        16,
    );
    mode.add(
        &radio_index,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        16,
    );
    mode.add(&radio_fly, 0, SizerFlag::AlignCenterVertical, 0);
    root.add_sizer(&mode, 0, SizerFlag::Expand | SizerFlag::All, PAD);

    // actions: what a scan will use (or what is missing) on the left, Scan on the right
    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    // the info icon marks a hint (something is missing); it is hidden while the line is plain information
    let info_icon = BitmapBundle::from_svg_text(INFO_SVG, Size::new(16, 16))
        .and_then(|b| b.get_bitmap_for(&panel));
    let info = StaticBitmap::builder(&panel).with_bitmap(info_icon).build();
    let status = StaticText::builder(&panel).with_label(" ").build();
    // the main action: the default button, which the system draws emphasised, and a bit larger
    let scan = Button::builder(&panel)
        .with_label(FIND)
        .with_size(Size::new(110, -1))
        .build();
    widgets::grow(&scan);
    scan.set_default();
    actions.add(
        &info,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        6,
    );
    actions.add(
        &status,
        1,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        PAD,
    );
    actions.add(&scan, 0, SizerFlag::AlignCenterVertical, 0);
    root.add_sizer(&actions, 0, SizerFlag::Expand | SizerFlag::All, PAD);

    // progress and results stay hidden (no empty space) until first needed
    let gauge = widgets::Bar::new(&panel, Size::new(-1, 18));
    root.add(
        &gauge.panel,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        PAD,
    );
    let results = ResultsView::new(&panel);
    root.add(
        &results.panel,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        PAD,
    );
    gauge.show(false);
    results.panel.show(false);

    let prefs = Prefs::load();
    let first_run = prefs.needs_setup();
    radio_index.set_value(!prefs.on_the_fly);
    radio_fly.set_value(prefs.on_the_fly);
    let state = State {
        source: None,
        prefs: Rc::new(RefCell::new(prefs)),
        settings: ScanSettings::default(),
        job: None,
        gauge_shown: false,
        results_shown: false,
        theme,
        field_colour: field_background(),
        logo_colour: String::new(),
    };

    let ui = Ui {
        frame,
        panel,
        settings_button,
        theme_button,
        about_button,
        logo,
        source,
        source_browse,
        source_clear,
        source_caption,
        radio_index,
        radio_fly,
        info,
        status,
        scan,
        gauge,
        results,
        timer: Rc::new(Timer::new(&frame)),
        state: Rc::new(RefCell::new(state)),
    };
    ui.connect();
    ui.sync();

    panel.set_sizer(root, true);
    frame.show(true);
    // size to content only once the window is shown: the title bar height is only known then.
    // Sizing before that made the window too short, and the bottom row (status and Find SID)
    // was cut off ("Negative content height" in GTK).
    ui.fit_height();
    // GTK applies the window size asynchronously: the first fit can read the old size, so fit once more shortly after
    let refit = Timer::new(&frame);
    let again = ui.clone();
    refit.on_tick(move |_| again.fit_height());
    refit.start(100, true);
    std::mem::forget(refit);
    frame.set_min_size(Size::new(MIN_WIDTH, frame.get_size().height));
    frame.centre();

    if first_run {
        // nothing to scan with yet: open the settings once the window is up
        let opener = Timer::new(&frame);
        let ui = ui.clone();
        opener.on_tick(move |_| ui.open_settings());
        opener.start(300, true);
        std::mem::forget(opener); // Drop would destroy it before it fires; it lives as long as the app
    }
}

/// The text colour of the theme as `#rrggbb`: the logo is drawn in it.
pub fn logo_ink() -> String {
    let c = SystemSettings::get_colour(SystemColour::WindowText);
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// The background of the input fields. On a dark theme it is a step lighter than the window, so the
/// field stays visible (Windows dark mode paints the window and the field in the same colour).
fn field_background() -> Colour {
    let window = SystemSettings::get_colour(SystemColour::Window);
    let text = SystemSettings::get_colour(SystemColour::WindowText);
    if luma(text) > 128.0 {
        mix(window, text, 0.12)
    } else {
        window
    }
}

/// The logo drawn in `ink` (`#rrggbb`) for `panel`; None if the SVG cannot be rendered.
pub fn logo_bitmap(panel: &Panel, ink: &str) -> Option<Bitmap> {
    // the artwork has no fill of its own (black); give its path one
    let svg = LOGO_SVG.replacen("<path", &format!("<path fill=\"{ink}\""), 1);
    let height = (LOGO_WIDTH as f32 * LOGO_VIEW.1 / LOGO_VIEW.0).round() as i32;
    BitmapBundle::from_svg_text(&svg, Size::new(LOGO_WIDTH, height))
        .and_then(|b| b.get_bitmap_for(panel))
}

impl Ui {
    fn connect(&self) {
        let ui = self.clone();
        self.timer.on_tick(move |_| ui.tick());

        // the caption is cut to the available width, so redo it when the window is resized
        let ui = self.clone();
        self.panel.on_size(move |event| {
            ui.refresh_caption();
            ui.results.layout(ui.results_width());
            event.skip(true); // the panel's own layout must still run
        });
        let ui = self.clone();
        self.source_clear.on_click(move || {
            ui.state.borrow_mut().source = None;
            ui.sync();
        });
        let ui = self.clone();
        self.source_browse.on_click(move |_| {
            let dialog = FileDialog::builder(&ui.frame)
                .with_message("Select a PRG file or VICE snapshot")
                .with_wildcard("C64 programs|*.prg;*.PRG;*.vsf;*.VSF")
                .build();
            if dialog.show_modal() == ID_OK {
                if let Some(path) = dialog
                    .get_path()
                    .map(PathBuf::from)
                    .filter(|p| is_source_file(p))
                {
                    ui.state.borrow_mut().source = Some(path);
                    ui.sync();
                }
            }
        });

        let ui = self.clone();
        self.radio_index.on_selected(move |_| ui.set_mode(false));
        let ui = self.clone();
        self.radio_fly.on_selected(move |_| ui.set_mode(true));

        let ui = self.clone();
        self.frame.on_activate(move |event| {
            ui.sync(); // the system theme may have changed while the window was in the background
            event.skip(true);
        });
        let ui = self.clone();
        self.settings_button
            .on_mouse_left_up(move |_| ui.open_settings());
        let ui = self.clone();
        self.theme_button
            .on_mouse_left_up(move |_| ui.show_theme_menu());
        let ui = self.clone();
        self.about_button
            .on_mouse_left_up(move |_| crate::about_window::show(&ui.frame));
        let ui = self.clone();
        self.frame
            .on_menu_selected(move |event| ui.on_menu(event.get_id()));

        let ui = self.clone();
        self.scan.on_click(move |_| ui.on_scan());

        // files can be dropped anywhere in the window, also on the (read-only) source field
        attach_drop(self, &self.panel);
        attach_drop(self, &self.source.panel);
        attach_drop(self, &self.source.text);
    }

    fn idle(&self) -> bool {
        self.state.borrow().job.is_none()
    }

    /// Feedback while a file is dragged over the window: the caption says what a drop does.
    fn drop_hover(&self, over: bool) {
        if over {
            // unmistakable: the field takes a tint of the accent colour and the caption the accent colour itself
            let accent = SystemSettings::get_colour(SystemColour::Highlight);
            let normal = self.state.borrow().field_colour;
            self.source.set_background_color(mix(normal, accent, 0.35));
            self.source_caption
                .set_foreground_color(readable(accent, normal));
            self.source_caption
                .set_label("Drop to use this file as the source");
        } else {
            self.source
                .set_background_color(self.state.borrow().field_colour);
            self.refresh_caption();
        }
        self.source.refresh();
    }

    /// A drop: the first dropped file becomes the source. Folders and drops while a scan
    /// runs are refused.
    fn on_drop(&self, files: &[String]) -> bool {
        self.drop_hover(false);
        let Some(file) = files
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file() && is_source_file(p))
        else {
            return false;
        };
        if !self.idle() {
            return false;
        }
        self.state.borrow_mut().source = Some(file);
        self.sync();
        true
    }

    fn prefs(&self) -> Rc<RefCell<Prefs>> {
        self.state.borrow().prefs.clone()
    }

    fn set_mode(&self, on_the_fly: bool) {
        let prefs = self.prefs();
        prefs.borrow_mut().on_the_fly = on_the_fly;
        prefs.borrow().save();
        self.sync();
    }

    fn open_settings(&self) {
        settings_window::show(&self.frame, &self.prefs());
        self.sync(); // the paths may have changed
    }

    /// Brings every widget in line with the state. The only place that enables or disables.
    fn sync(&self) {
        let (source, prefs, running) = {
            let st = self.state.borrow();
            (
                st.source.clone(),
                st.prefs.borrow().clone(),
                st.job.is_some(),
            )
        };
        let idle = !running;
        let have_source = source.is_some();
        let shown = source.as_deref().map(file_name).unwrap_or_default();
        if self.source.get_value() != shown {
            self.source.set_value(&shown);
        }
        self.source.set_tooltip(
            &source
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        );
        // what a scan reads: the index in indexed mode, else the HVSC folder
        let tunes = if prefs.on_the_fly {
            prefs.hvsc.as_ref()
        } else {
            prefs.index.as_ref()
        };
        // the HVSC folder is required in both modes; indexed mode also needs the index
        let missing_setting = prefs.missing();
        let have_tunes = missing_setting.is_none();

        self.source_clear.enable(idle && have_source);
        for control in [
            &self.source.panel as &dyn WxWidget,
            &self.source_browse,
            &self.radio_index,
            &self.radio_fly,
            &self.settings_button,
        ] {
            control.enable(idle);
        }
        self.scan.set_label(if running { "Cancel" } else { FIND });
        self.scan.enable(running || (have_source && have_tunes));

        // what is missing in normal text, what a scan will use in grey
        let (text, missing) = match (running, missing_setting, have_source, prefs.on_the_fly) {
            (true, ..) => (String::new(), false),
            (_, Some(Missing::Hvsc), ..) => {
                ("Select the HVSC folder in Settings".to_string(), true)
            }
            (_, Some(Missing::Index), ..) => {
                ("Select or build an index in Settings".to_string(), true)
            }
            (_, None, false, _) => ("Choose or drop a source file".to_string(), true),
            (_, None, true, true) => (
                format!("Reading HVSC: {}", file_name(tunes.unwrap())),
                false,
            ),
            (_, None, true, false) => {
                (format!("Using index: {}", file_name(tunes.unwrap())), false)
            }
        };
        let colour = if missing {
            SystemSettings::get_colour(SystemColour::WindowText)
        } else {
            muted(&self.panel)
        };
        self.info.show(missing);
        self.status.set_foreground_color(colour);
        self.status.set_label(&if text.is_empty() {
            " ".to_string()
        } else {
            text
        });
        self.status
            .set_tooltip(&tunes.map(|p| p.display().to_string()).unwrap_or_default());
        // the field is coloured explicitly: on Windows in dark mode it would stay white
        let field = field_background();
        self.state.borrow_mut().field_colour = field;
        self.source.set_background_color(field);
        self.source
            .set_foreground_color(SystemSettings::get_colour(SystemColour::WindowText));
        self.refresh_caption();
        self.refresh_logo();
        self.panel.layout();
    }

    /// Draws the logo in the current text colour, when that changed since the last time.
    fn refresh_logo(&self) {
        let ink = logo_ink();
        if self.state.borrow().logo_colour == ink {
            return;
        }
        if let Some(bitmap) = logo_bitmap(&self.panel, &ink) {
            self.logo.set_bitmap(&bitmap);
            self.state.borrow_mut().logo_colour = ink;
        }
    }

    /// Sets the full-path caption under the source field, cut in the middle to the width of
    /// the field. The panel size event fires before the sizer has laid out the children, so
    /// this works from the panel width and the controls that have a fixed width.
    fn refresh_caption(&self) {
        let panel = self.panel.get_client_size().width;
        let field = panel
            - 2 * PAD
            - self.source_browse.get_size().width
            - self.source_clear.widget().get_size().width
            - 2 * GAP;
        let text = self
            .state
            .borrow()
            .source
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.source_caption.set_foreground_color(muted(&self.panel));
        self.source_caption.set_label(&fit(
            &self.source_caption,
            &text,
            (field - widgets::CAPTION_INSET).max(60),
        ));
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

    fn on_menu(&self, id: i32) {
        if id == ID_EXIT {
            self.frame.close(false);
        } else if id == ID_ABOUT {
            crate::about_window::show(&self.frame);
        } else {
            self.on_theme_chosen(id);
        }
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
            Applied::Unsupported => {
                Some("This system does not allow changing the theme from SidFind.")
            }
        };
        if let Some(note) = note {
            let dialog = MessageDialog::builder(&self.frame, note, TITLE)
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
            dialog.show_modal();
        }
        self.sync(); // refreshes the colours that are fixed at the time they are set
    }

    fn on_scan(&self) {
        if let Some(job) = &self.state.borrow().job {
            job.cancel();
            return;
        }
        let (file, source, settings, on_the_fly) = {
            let st = self.state.borrow();
            let prefs = st.prefs.borrow();
            if prefs.missing().is_some() {
                return; // the button is disabled then; this keeps the rule in one place too
            }
            let source = if prefs.on_the_fly {
                prefs.hvsc.clone().map(Source::Hvsc)
            } else {
                prefs.index.clone().map(Source::Index)
            };
            (
                st.source.clone(),
                source,
                st.settings.clone(),
                prefs.on_the_fly,
            )
        };
        let (Some(file), Some(source)) = (file, source) else {
            return;
        };
        // an index lookup is too quick for a progress bar; reading the whole HVSC is not
        if on_the_fly && !self.state.borrow().gauge_shown {
            self.state.borrow_mut().gauge_shown = true;
            self.gauge.show(true);
            self.fit_height();
        }
        self.gauge.set_value(0);
        self.results.clear();
        self.state.borrow_mut().job = Some(Job::scan(file, source, settings));
        self.timer.start(TICK_MS, false);
        self.sync();
    }

    /// Called by the timer while a scan runs: moves the bar and picks up the result.
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
        // the results area appears the first time there is something to show
        if !self.state.borrow().results_shown {
            self.state.borrow_mut().results_shown = true;
            self.results.panel.show(true);
        }
        let width = self.results_width();
        match result {
            Ok(Outcome::Scanned(found)) => {
                if let Some(first) = found.first() {
                    self.results.show_found(first, width);
                }
            }
            Ok(Outcome::Built { .. }) => {} // index builds run in the settings window
            Err(e) if e == CANCELLED => self.results.show_message("Cancelled", "", width),
            Err(e) => {
                let dialog = MessageDialog::builder(&self.frame, &e, TITLE)
                    .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconError)
                    .build();
                dialog.show_modal();
                self.results.show_message("Error", &e, width);
            }
        }
        self.fit_height(); // the window takes the height its content needs, no more
        self.sync();
    }

    /// The width the results view has inside the window.
    fn results_width(&self) -> i32 {
        self.panel.get_client_size().width - 2 * PAD
    }

    /// Sets the window's height to what its content needs (the width is left alone): it grows
    /// when something appears and shrinks again for a shorter result. Never below the minimum.
    fn fit_height(&self) {
        self.panel.layout();
        let width = self.frame.get_size().width;
        self.frame.fit();
        // GTK can size the window too short, and the bottom row (status, Find SID) then falls
        // off the bottom edge. The laid-out bottom edge of that row tells how much is missing.
        self.panel.layout();
        let bottom = |w: &dyn WxWidget| w.get_position().y + w.get_size().height;
        let missing = bottom(&self.scan).max(bottom(&self.status)) + PAD
            - self.panel.get_client_size().height;
        let height = self.frame.get_size().height + missing.max(0);
        self.frame.set_size(Size::new(width, height));
    }
}

/// `accent` as text colour on `background`: on a dark background the accent (a fairly deep
/// blue) is lightened toward white so it stays readable; on a light one it is used as it is.
fn readable(accent: Colour, background: Colour) -> Colour {
    if luma(background) < 128.0 {
        mix(accent, Colour::rgb(255, 255, 255), 0.55)
    } else {
        accent
    }
}

/// Makes `window` accept dropped files for `ui`.
fn attach_drop<W: WxWidget>(ui: &Ui, window: &W) {
    let (enter, over, leave, files) = (ui.clone(), ui.clone(), ui.clone(), ui.clone());
    let hover = move |ui: &Ui, default: DragResult| {
        // while a scan runs the cursor shows that a drop is not possible
        if ui.idle() {
            ui.drop_hover(true);
            default
        } else {
            DragResult::None
        }
    };
    FileDropTarget::builder(window)
        .with_on_enter(move |_, _, default| hover(&enter, default))
        .with_on_drag_over(move |_, _, default| {
            if over.idle() {
                default
            } else {
                DragResult::None
            }
        })
        .with_on_leave(move || leave.drop_hover(false))
        .with_on_drop_files(move |dropped, _, _| files.on_drop(&dropped))
        .build();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_blends_between_two_colours() {
        let (black, white) = (Colour::rgb(0, 0, 0), Colour::rgb(255, 255, 255));
        assert_eq!(
            (mix(black, white, 0.0).r, mix(black, white, 1.0).r),
            (0, 255)
        );
        assert_eq!(mix(black, white, 0.5).g, 128);
        assert_eq!(
            mix(Colour::rgb(10, 20, 30), Colour::rgb(10, 20, 30), 0.7).b,
            30
        );
    }

    #[test]
    fn accent_text_is_lightened_on_dark_backgrounds_only() {
        let accent = Colour::rgb(0, 90, 220);
        let (dark, light) = (Colour::rgb(30, 30, 30), Colour::rgb(250, 250, 250));
        assert!(
            luma(readable(accent, dark)) > luma(accent) + 40.0,
            "must get clearly brighter on dark"
        );
        assert_eq!(readable(accent, light).b, accent.b, "unchanged on light");
    }
}

#[cfg(test)]
mod logo_tests {
    use super::*;

    #[test]
    fn the_logo_constants_match_the_artwork() {
        let (w, h) = LOGO_VIEW;
        assert!(
            LOGO_SVG.contains(&format!("viewBox=\"0 0 {w} {h}\"")),
            "LOGO_VIEW is out of date"
        );
        assert!(!LOGO_SVG.contains("<text"), "the logo must be shapes only");
        assert!(
            LOGO_SVG.contains("<path"),
            "refresh_logo fills the first <path>"
        );
    }
}
