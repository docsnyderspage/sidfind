//! The results view: the scanned file's name, then one row per match with a bar showing how
//! much of the tune was found, the percentage, the tune's path and its score.

use std::cell::RefCell;
use std::rc::Rc;

use sidfind_core::Identification;
use wxdragon::prelude::*;

use crate::widgets::{Bar, GAP, PAD, file_name, fit};

/// How many matches can be shown (the scan settings' `top` must not exceed this).
const MAX_ROWS: usize = 10;
const BAR_WIDTH: i32 = 96;
const BAR_HEIGHT: i32 = 10;
/// Space under the file name, and between the rows.
const AFTER_HEADER: i32 = 14;
const BETWEEN_ROWS: i32 = 10;
/// The widest text of the fixed columns, used to size them.
const WIDEST_PERCENT: &str = "100.0%";
const WIDEST_SCORE: &str = "score 99999.9";

#[derive(Clone)]
struct Row {
    panel: Panel,
    bar: Bar,
    percent: StaticText,
    name: StaticText,
    score: StaticText,
}

/// What is shown right now, kept so it can be fitted again when the window is resized.
#[derive(Default)]
struct Content {
    /// Full tune paths of the visible rows.
    names: Vec<String>,
    /// A message instead of rows ("Cancelled.", an error, "No match ...").
    note: String,
}

#[derive(Clone)]
pub struct ResultsView {
    pub panel: Panel,
    header: StaticText,
    note: StaticText,
    rows: Vec<Row>,
    content: Rc<RefCell<Content>>,
}

impl ResultsView {
    pub fn new(parent: &Panel) -> ResultsView {
        let panel = Panel::builder(parent).build(); // no fixed height: the rows decide
        let root = BoxSizer::builder(Orientation::Vertical).build();

        let header = StaticText::builder(&panel).with_label(" ").build();
        if let Some(mut font) = header.get_font() {
            font.make_bold();
            header.set_font(&font);
        }
        root.add(&header, 0, SizerFlag::Bottom, AFTER_HEADER);
        let note = StaticText::builder(&panel).with_label(" ").build();
        root.add(&note, 0, SizerFlag::Expand, 0);

        let percent_width = header.get_text_extent(WIDEST_PERCENT).width;
        let score_width = header.get_text_extent(WIDEST_SCORE).width;
        let rows = (0..MAX_ROWS)
            .map(|_| {
                // each row is a panel of its own, so hiding it also removes its spacing
                let row = Panel::builder(&panel).build();
                let bar = Bar::new(&row, Size::new(BAR_WIDTH, BAR_HEIGHT));
                let percent = StaticText::builder(&row)
                    .with_label(" ")
                    .with_style(StaticTextStyle::AlignRight)
                    .build();
                percent.set_min_size(Size::new(percent_width, -1));
                let name = StaticText::builder(&row).with_label(" ").build();
                let score = StaticText::builder(&row)
                    .with_label(" ")
                    .with_style(StaticTextStyle::AlignRight)
                    .build();
                score.set_min_size(Size::new(score_width, -1));

                let line = BoxSizer::builder(Orientation::Horizontal).build();
                line.add(&bar.panel, 0, SizerFlag::AlignCenterVertical, 0);
                line.add(
                    &percent,
                    0,
                    SizerFlag::AlignCenterVertical | SizerFlag::Left,
                    GAP,
                );
                line.add(
                    &name,
                    1,
                    SizerFlag::AlignCenterVertical | SizerFlag::Left,
                    GAP,
                );
                line.add(
                    &score,
                    0,
                    SizerFlag::AlignCenterVertical | SizerFlag::Left,
                    GAP,
                );
                row.set_sizer(line, true);
                row.show(false);
                root.add(&row, 0, SizerFlag::Expand | SizerFlag::Bottom, BETWEEN_ROWS);
                Row {
                    panel: row,
                    bar,
                    percent,
                    name,
                    score,
                }
            })
            .collect();
        panel.set_sizer(root, true);
        ResultsView {
            panel,
            header,
            note,
            rows,
            content: Rc::new(RefCell::new(Content::default())),
        }
    }

    /// Empties the view (a scan is starting).
    pub fn clear(&self) {
        self.header.set_label(" ");
        self.note.show(false);
        for row in &self.rows {
            row.panel.show(false);
        }
        *self.content.borrow_mut() = Content::default();
    }

    /// The matches of one file; `width` is the width available to the view.
    pub fn show_found(&self, found: &Identification, width: i32) {
        self.clear();
        self.header.set_label(&file_name(&found.path));
        if found.matches.is_empty() {
            self.set_note(
                &format!(
                    "No match ({} distinct windows checked; {}). The tune may be packed, relocated or not in HVSC.",
                    found.windows, found.emulation
                ),
                width,
            );
            return;
        }
        let mut content = self.content.borrow_mut();
        for (row, m) in self.rows.iter().zip(&found.matches) {
            row.bar
                .set_value((m.share * 100.0).round().clamp(0.0, 100.0) as i32);
            row.percent.set_label(&format!("{:.1}%", 100.0 * m.share));
            row.score.set_label(&format!("score {:.1}", m.score));
            row.panel.show(true);
            content.names.push(format!("/{}", m.name));
        }
        drop(content);
        self.layout(width);
    }

    /// A short message with a title: "Cancelled", an error.
    pub fn show_message(&self, title: &str, text: &str, width: i32) {
        self.clear();
        self.header.set_label(title);
        self.set_note(text, width);
    }

    fn set_note(&self, text: &str, width: i32) {
        self.content.borrow_mut().note = text.to_string();
        self.note.show(!text.is_empty());
        self.layout(width);
    }

    /// Fits the text to `width`: tune paths are cut in the middle, a message wraps.
    pub fn layout(&self, width: i32) {
        let content = self.content.borrow();
        if !content.note.is_empty() {
            self.note.set_label(&content.note);
            self.note.wrap((width - PAD).max(120));
        }
        // what the fixed columns and gaps leave for the tune's path
        let fixed = BAR_WIDTH
            + self.rows[0].percent.get_size().width
            + self.rows[0].score.get_size().width
            + 3 * GAP
            + PAD;
        let available = (width - fixed).max(80);
        for (row, name) in self.rows.iter().zip(&content.names) {
            row.name.set_label(&fit(&row.name, name, available));
            row.name.set_tooltip(name);
        }
        self.panel.layout();
    }
}
