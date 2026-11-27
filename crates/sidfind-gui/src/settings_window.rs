//! The settings window: HVSC folder, index file and building an index. It is modal, so the
//! main window waits; changes are saved as they are made and shared through `Prefs`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use sidfind_core::api::CANCELLED;
use wxdragon::prelude::*;

use crate::job::{Job, Outcome};
use crate::prefs::Prefs;
use crate::widgets::{Bar, GAP, PAD, PathSetting, TITLE, grow, headline, hyperlink};

const TICK_MS: i32 = 100;
/// Where to get the HVSC collection.
const HVSC_DOWNLOAD_URL: &str = "https://hvsc.c64.org/downloads";
const WIDTH: i32 = 520;
/// Space between the sections (HVSC folder, index file, building) and before the footer.
const SECTION_GAP: i32 = 2 * PAD;

#[derive(Clone)]
struct Win {
    dialog: Dialog,
    panel: Panel,
    hvsc: PathSetting,
    index: PathSetting,
    build: Button,
    hint: StaticText,
    gauge: Bar,
    status: StaticText,
    timer: Rc<Timer<Dialog>>,
    prefs: Rc<RefCell<Prefs>>,
    /// The index build in progress, if any.
    job: Rc<RefCell<Option<Job>>>,
    gauge_shown: Rc<RefCell<bool>>,
}

/// Shows the settings window and returns when it is closed. A build still running at that
/// point is cancelled.
pub fn show(parent: &Frame, prefs: &Rc<RefCell<Prefs>>) {
    let dialog = Dialog::builder(parent, "Settings")
        .with_size(WIDTH, 300)
        .build();
    let panel = Panel::builder(&dialog).build();
    let root = BoxSizer::builder(Orientation::Vertical).build();

    root.add(
        &headline(&panel, "Settings"),
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        PAD,
    );
    root.add_spacer(SECTION_GAP);
    let (hvsc, hvsc_cell) = PathSetting::new(
        &panel,
        "HVSC folder (required)",
        "",
        Some(("Download the HVSC collection", HVSC_DOWNLOAD_URL)),
        "Clear HVSC folder",
    );
    let (index, index_cell) = PathSetting::new(
        &panel,
        "Index file",
        "Allows for faster searches. Build it once from the HVSC folder.",
        None,
        "Clear index file",
    );
    root.add_sizer(
        &hvsc_cell,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        PAD,
    );
    root.add_spacer(SECTION_GAP);
    root.add_sizer(
        &index_cell,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        PAD,
    );
    root.add_spacer(SECTION_GAP);

    // building an index: the button, and why it is unavailable when it is
    let build_row = BoxSizer::builder(Orientation::Horizontal).build();
    let build = Button::builder(&panel).with_label("Build index...").build();
    grow(&build);
    let hint = StaticText::builder(&panel).with_label(" ").build();
    build_row.add(&build, 0, SizerFlag::AlignCenterVertical, 0);
    build_row.add(
        &hint,
        1,
        SizerFlag::AlignCenterVertical | SizerFlag::Left,
        PAD,
    );
    root.add_sizer(&build_row, 0, SizerFlag::Expand | SizerFlag::All, PAD);

    // progress and the outcome stay hidden until a build starts
    let gauge = Bar::new(&panel, Size::new(-1, 18));
    let status = StaticText::builder(&panel).with_label(" ").build();
    root.add(
        &gauge.panel,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        PAD,
    );
    root.add(
        &status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        PAD,
    );
    gauge.show(false);
    status.show(false);

    root.add_spacer(SECTION_GAP); // air before the footer; the stretch keeps it at the bottom if the window grows
    root.add_stretch_spacer(1);
    let close = Button::builder(&panel).with_label("Close").build();
    grow(&close);
    // the project page: opens in the default browser (the URL comes from Cargo.toml)
    let link = hyperlink(&panel, "SidFind on GitHub", env!("CARGO_PKG_REPOSITORY"));
    let close_row = BoxSizer::builder(Orientation::Horizontal).build();
    close_row.add(&link, 0, SizerFlag::AlignCenterVertical, 0);
    close_row.add_stretch_spacer(1);
    close_row.add(&close, 0, SizerFlag::empty(), 0);
    root.add_sizer(&close_row, 0, SizerFlag::Expand | SizerFlag::All, PAD);

    let win = Win {
        dialog,
        panel,
        hvsc,
        index,
        build,
        hint,
        gauge,
        status,
        timer: Rc::new(Timer::new(&dialog)),
        prefs: prefs.clone(),
        job: Rc::new(RefCell::new(None)),
        gauge_shown: Rc::new(RefCell::new(false)),
    };
    win.connect(close);
    win.sync();

    panel.set_sizer(root, true);
    let dialog_sizer = BoxSizer::builder(Orientation::Vertical).build();
    dialog_sizer.add(&panel, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(dialog_sizer, true);
    dialog.fit();
    let best = dialog.get_size();
    dialog.set_size(Size::new(WIDTH, best.height));
    dialog.centre();
    win.sync(); // captions need the final width

    dialog.set_escape_id(ID_CANCEL);
    dialog.show_modal();

    // closing mid-build cancels it; the result would have nowhere to go
    win.timer.stop();
    if let Some(job) = win.job.borrow().as_ref() {
        job.cancel();
    }
    dialog.destroy();
}

impl Win {
    fn connect(&self, close: Button) {
        let win = self.clone();
        self.timer.on_tick(move |_| win.tick());

        let win = self.clone();
        self.panel.on_size(move |event| {
            win.sync();
            event.skip(true);
        });

        let win = self.clone();
        close.on_click(move |_| win.dialog.end_modal(ID_OK));

        let win = self.clone();
        self.hvsc.button.on_click(move |_| {
            let dialog = DirDialog::builder(&win.dialog, "Select the HVSC folder", "").build();
            if dialog.show_modal() == ID_OK {
                win.prefs.borrow_mut().hvsc = dialog.get_path().map(PathBuf::from);
                win.changed();
            }
        });
        let win = self.clone();
        self.hvsc.clear.on_click(move || {
            win.prefs.borrow_mut().hvsc = None;
            win.changed();
        });

        let win = self.clone();
        self.index.button.on_click(move |_| {
            let dialog = FileDialog::builder(&win.dialog)
                .with_message("Select the index file")
                .with_wildcard("Index files|*.idx|All files|*")
                .build();
            if dialog.show_modal() == ID_OK {
                win.prefs.borrow_mut().index = dialog.get_path().map(PathBuf::from);
                win.changed();
            }
        });
        let win = self.clone();
        self.index.clear.on_click(move || {
            win.prefs.borrow_mut().index = None;
            win.changed();
        });

        let win = self.clone();
        self.build.on_click(move |_| win.on_build());
    }

    /// A setting changed: remember it and redraw.
    fn changed(&self) {
        self.prefs.borrow().save();
        self.sync();
    }

    fn building(&self) -> bool {
        self.job.borrow().is_some()
    }

    fn sync(&self) {
        let (hvsc, index) = {
            let p = self.prefs.borrow();
            (p.hvsc.clone(), p.index.clone())
        };
        let idle = !self.building();
        self.hvsc.show(hvsc.as_deref(), idle);
        self.index.show(index.as_deref(), idle);
        self.hvsc.button.enable(idle);
        self.index.button.enable(idle);

        self.build
            .set_label(if idle { "Build index..." } else { "Cancel" });
        self.build.enable(!idle || hvsc.is_some());
        self.hint.set_label(if idle && hvsc.is_none() {
            "Select the HVSC folder first"
        } else {
            " "
        });

        let width = self.panel.get_client_size().width - 2 * PAD;
        self.hvsc.set_caption(hvsc.as_deref(), width);
        self.index.set_caption(index.as_deref(), width);
        self.panel.layout();
    }

    fn on_build(&self) {
        if let Some(job) = self.job.borrow().as_ref() {
            job.cancel();
            return;
        }
        let Some(hvsc) = self.prefs.borrow().hvsc.clone() else {
            return;
        };
        // default: the per-user data folder, so a new HVSC release can't wipe the index
        let dir = StandardPaths::user_data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let chooser = FileDialog::builder(&self.dialog)
            .with_message("Save the index as")
            .with_default_dir(&dir)
            .with_default_file("hvsc.idx")
            .with_wildcard("Index files|*.idx|All files|*")
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .build();
        if chooser.show_modal() != ID_OK {
            return;
        }
        let Some(path) = chooser.get_path().map(PathBuf::from) else {
            return;
        };

        self.reveal();
        self.gauge.set_value(0);
        self.status.set_label("Building the index...");
        *self.job.borrow_mut() = Some(Job::build(hvsc, path));
        self.timer.start(TICK_MS, false);
        self.sync();
    }

    /// Shows the progress bar and status line the first time a build starts.
    fn reveal(&self) {
        if !self.gauge_shown.replace(true) {
            self.gauge.show(true);
            self.status.show(true);
            self.panel.layout();
            let size = self.dialog.get_size();
            self.dialog
                .set_size(Size::new(size.width, size.height + 2 * GAP + 36 + 20));
        }
    }

    /// Called by the timer while the build runs: moves the bar and picks up the result.
    fn tick(&self) {
        let (percent, result) = match self.job.borrow().as_ref() {
            Some(job) => (job.percent(), job.take_result()),
            None => return,
        };
        self.gauge.set_value(percent);
        let Some(result) = result else { return };

        self.timer.stop();
        *self.job.borrow_mut() = None;
        self.gauge.set_value(if result.is_ok() { 100 } else { 0 });
        let text = match result {
            Ok(Outcome::Built { path, summary }) => {
                self.prefs.borrow_mut().index = Some(path.clone());
                self.prefs.borrow().save();
                format!(
                    "Index built: {} tunes, {:.0} MB. It is now the index file.",
                    summary.tunes,
                    summary.bytes as f64 / 1e6
                )
            }
            Ok(Outcome::Scanned(_)) => String::new(), // not started from here
            Err(e) if e == CANCELLED => "Cancelled.".to_string(),
            Err(e) => {
                let dialog = MessageDialog::builder(&self.dialog, &e, TITLE)
                    .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconError)
                    .build();
                dialog.show_modal();
                format!("Error: {e}")
            }
        };
        self.status.set_label(&text);
        self.sync();
    }
}
