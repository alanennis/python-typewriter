//! the typewriter itself: the line being typed, margins, bells and
//! alignment. it knows nothing about the screen; the caller supplies the
//! bell sounds and the printer.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::time::Instant;

use crate::session::Session;

/// how the line being typed is laid out while an alignment mode is on
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Centre,
    Right,
}

/// anything that can print one line at a time. every carriage return
/// calls it, blank lines included, and the printer must feed a line for a
/// blank one just as for text
pub trait LinePrinter {
    fn print_line(&mut self, text: &str, spacing: f64, align: Option<Align>) -> io::Result<()>;
}

/// a sound the machine can make (a beep, the margin bell)
pub type Sound = Box<dyn FnMut()>;

/// how a tab stop is marked on the tab bar: a comma, low like the dots
/// so it doesn't stand out like the margins
pub const TAB_MARK: char = ',';

/// most finished lines that can be shown above the typing
pub const MAX_PREVIOUS: usize = 7;

/// line spacing choices: single, one and a half, double
pub const SPACING_CHOICES: [f64; 3] = [1.0, 1.5, 2.0];

/// settings for a new machine. the defaults match the python version
#[derive(Clone, Debug)]
pub struct Options {
    pub width: usize,
    pub spacing_index: usize,
    pub autoreturn: bool,
    pub margin_bell: usize,
    pub left_margin: usize,
    /// None means the full width
    pub right_margin: Option<usize>,
    pub use_file: bool,
    pub use_printer: bool,
    /// name of the chosen printer, shown on screen even if not found
    pub printer_name: String,
    /// false silences the bell and the margin bell
    pub bell_on: bool,
    /// columns the tab key jumps to
    pub tab_stops: Vec<usize>,
    /// words to write this session; 0 for none
    pub word_target: usize,
    /// show the session time on screen
    pub show_timer: bool,
    /// show the words per minute on screen
    pub show_wpm: bool,
    /// finished lines shown above the typing, 0 to MAX_PREVIOUS
    pub previous_lines: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 75,
            spacing_index: 1,
            autoreturn: false,
            margin_bell: 8,
            left_margin: 0,
            right_margin: None,
            use_file: true,
            use_printer: true,
            printer_name: "none".into(),
            bell_on: true,
            tab_stops: Vec::new(),
            word_target: 0,
            show_timer: true,
            show_wpm: true,
            previous_lines: 3,
        }
    }
}

pub struct Typewriter {
    pub width: usize,
    pub autoreturn: bool,
    pub spacing_index: usize,
    pub current_spacing: f64,
    pub buffer: Vec<char>,
    pub margin_bell: usize,
    pub right_margin: usize,
    pub left_margin: usize,
    pub margin_release: bool,
    /// the last MAX_PREVIOUS finished lines, oldest first
    pub previous: Vec<String>,
    /// how many of them are shown above the typing
    pub previous_lines: usize,
    pub file_name: String,
    pub use_file: bool,
    pub use_printer: bool,
    pub margin_hot_zone: bool,
    pub help_wanted: bool,
    /// set while aligning the line being typed
    pub align: Option<Align>,
    pub align_chars: Vec<char>,
    pub word_count: usize,
    pub line_count: usize,
    pub save_folder: String,
    /// column the next character will be typed at
    pub cursor: usize,
    pub printer: Option<Box<dyn LinePrinter>>,
    pub printer_found: bool,
    pub printer_name: String,
    pub bell_on: bool,
    /// columns the tab key jumps to, in order
    pub tab_stops: Vec<usize>,
    /// words to write this session; 0 for none
    pub word_target: usize,
    /// word_count when the session started (more than 0 when carrying on
    /// with an earlier file)
    pub start_words: usize,
    /// the margin bell has rung for reaching word_target
    pub target_reached: bool,
    /// time spent at the machine, for the words per minute
    pub session: Session,
    pub show_timer: bool,
    pub show_wpm: bool,
    bell_sound: Option<Sound>,
    ding_sound: Option<Sound>,
}

impl Typewriter {
    /// bell is called whenever the machine wants to beep (line full,
    /// cursor at the end of the line...). ding is the margin bell when you
    /// reach the hot zone; it uses the plain bell unless given
    pub fn new(
        opts: Options,
        printer: Option<Box<dyn LinePrinter>>,
        bell: Option<Sound>,
        ding: Option<Sound>,
    ) -> Self {
        let spacing_index = opts.spacing_index.min(SPACING_CHOICES.len() - 1);
        let right_margin = opts.right_margin.unwrap_or(opts.width).min(opts.width);
        let today = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
        let printer_found = printer.is_some();
        let mut tab_stops: Vec<usize> = opts.tab_stops.into_iter().filter(|&c| c < opts.width).collect();
        tab_stops.sort_unstable();
        tab_stops.dedup();
        Typewriter {
            width: opts.width,
            autoreturn: opts.autoreturn,
            spacing_index,
            current_spacing: SPACING_CHOICES[spacing_index],
            buffer: vec![' '; opts.left_margin],
            margin_bell: opts.margin_bell,
            right_margin,
            left_margin: opts.left_margin,
            margin_release: false,
            previous: Vec::new(),
            previous_lines: opts.previous_lines.min(MAX_PREVIOUS),
            file_name: format!("{today}.txt"),
            use_file: opts.use_file,
            use_printer: opts.use_printer,
            margin_hot_zone: false,
            help_wanted: false,
            align: None,
            align_chars: Vec::new(),
            word_count: 0,
            line_count: 0,
            save_folder: "./".into(),
            cursor: opts.left_margin,
            printer,
            printer_found,
            printer_name: opts.printer_name,
            bell_on: opts.bell_on,
            tab_stops,
            word_target: opts.word_target,
            start_words: 0,
            target_reached: false,
            session: Session::new(Instant::now()),
            show_timer: opts.show_timer,
            show_wpm: opts.show_wpm,
            bell_sound: bell,
            ding_sound: ding,
        }
    }

    pub fn give_buffer(&self) -> String {
        self.buffer.iter().collect()
    }

    /// beep for an error, unless the bell is switched off
    pub fn bell(&mut self) {
        if self.bell_on {
            if let Some(b) = self.bell_sound.as_mut() {
                b();
            }
        }
    }

    /// ring the margin bell, unless the bell is switched off
    pub fn ding(&mut self) {
        if !self.bell_on {
            return;
        }
        match self.ding_sound.as_mut() {
            Some(d) => d(),
            None => self.bell(),
        }
    }

    /// switch all bell sounds off or on
    pub fn toggle_bell(&mut self) {
        self.bell_on = !self.bell_on;
    }

    /// column where the margin bell rings and the hot zone begins. can be
    /// negative if the bell distance is bigger than the right margin
    pub fn hot_zone_start(&self) -> i64 {
        self.right_margin as i64 - self.margin_bell as i64
    }

    /// tab bar column used to show the right margin
    fn right_marker_index(&self) -> usize {
        self.right_margin.min(self.width.saturating_sub(1))
    }

    /// the margin line: '!' at each margin, ':' where the hot zone starts,
    /// TAB_MARK at each tab stop and '.' everywhere else
    pub fn give_tab_bar(&self) -> String {
        let mut bar = vec!['.'; self.width];
        let hot = self.hot_zone_start();
        if hot >= 0 && (hot as usize) < self.width {
            bar[hot as usize] = ':';
        }
        for &stop in &self.tab_stops {
            if stop < self.width {
                bar[stop] = TAB_MARK;
            }
        }
        if self.left_margin < self.width {
            bar[self.left_margin] = '!';
        }
        if self.width > 0 {
            let r = self.right_marker_index();
            bar[r] = '!';
        }
        bar.into_iter().collect()
    }

    pub fn toggle_line_space(&mut self) {
        self.spacing_index = (self.spacing_index + 1) % SPACING_CHOICES.len();
        self.current_spacing = SPACING_CHOICES[self.spacing_index];
    }

    pub fn toggle_autoreturn(&mut self) {
        self.autoreturn = !self.autoreturn;
    }

    /// add a character at the cursor, checking margins etc. only fails if
    /// an autoreturn can't write the line to the file
    pub fn char_add(&mut self, key: char) -> Result<(), String> {
        if self.align.is_some() {
            self.align_add(key);
            return Ok(());
        }
        let len = self.buffer.len();
        // at the margin bell point ring the bell and set the hot zone flag
        if len as i64 == self.hot_zone_start() {
            self.ding();
        }
        self.margin_hot_zone = len as i64 >= self.hot_zone_start();

        // add the character if it's inside the right margin (or the margin
        // is released) and not past the printable width
        let fits = (len < self.right_margin || self.margin_release)
            && len < self.width.saturating_sub(1);
        // typing in the middle of the line (after moving back with the
        // arrow keys) inserts, and must never trigger an autoreturn
        let at_end = self.cursor == len;
        if fits {
            self.buffer.insert(self.cursor, key);
            self.cursor += 1;
        }

        // space or hyphen in the hot zone, or a full line, gives an
        // autoreturn if it's on, otherwise a ding
        if (self.margin_hot_zone && (key == ' ' || key == '-')) || !fits {
            if self.autoreturn && at_end {
                self.carriage_return()?;
            } else {
                self.bell();
            }
        }
        Ok(())
    }

    /// move the cursor back one space and delete the char before it
    pub fn backspace(&mut self) {
        if self.align.is_some() {
            if self.align_chars.pop().is_some() {
                self.layout_align();
            }
            return;
        }
        if !self.buffer.is_empty() && self.cursor > 0 {
            self.buffer.remove(self.cursor - 1);
            self.cursor -= 1;
        }
    }

    /// index where the word before position end starts, skipping any
    /// spaces between it and end first. never goes below floor
    fn word_start(chars: &[char], end: usize, floor: usize) -> usize {
        let mut i = end;
        while i > floor && chars[i - 1] == ' ' {
            i -= 1;
        }
        while i > floor && chars[i - 1] != ' ' {
            i -= 1;
        }
        i
    }

    /// delete back to the start of the word before the cursor. stops at
    /// the left margin, unless the cursor is already at or left of it
    pub fn delete_word(&mut self) {
        if self.align.is_some() {
            let start = Self::word_start(&self.align_chars, self.align_chars.len(), 0);
            self.align_chars.truncate(start);
            self.layout_align();
            return;
        }
        let floor = if self.cursor > self.left_margin { self.left_margin } else { 0 };
        let start = Self::word_start(&self.buffer, self.cursor, floor);
        self.buffer.drain(start..self.cursor);
        self.cursor = start;
    }

    /// delete the character under the cursor
    pub fn delete(&mut self) {
        if self.align.is_none() && self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    /// move the cursor one place left, ding at the start of the line
    pub fn cursor_left(&mut self) {
        if self.align.is_some() || self.cursor == 0 {
            self.bell();
            return;
        }
        self.cursor -= 1;
    }

    /// move the cursor one place right, ding at the end of the text
    pub fn cursor_right(&mut self) {
        if self.align.is_some() || self.cursor >= self.buffer.len() {
            self.bell();
            return;
        }
        self.cursor += 1;
    }

    /// move the cursor to the left margin, or to the very start of the
    /// line if it is already at (or left of) the margin
    pub fn cursor_home(&mut self) {
        if self.align.is_some() {
            return;
        }
        let margin = self.left_margin.min(self.buffer.len());
        self.cursor = if self.cursor > margin { margin } else { 0 };
    }

    /// move the cursor to the end of the text
    pub fn cursor_end(&mut self) {
        if self.align.is_none() {
            self.cursor = self.buffer.len();
        }
    }

    /// set a tab stop at the cursor, or clear the one that's there
    pub fn toggle_tab_stop(&mut self) {
        if self.align.is_some() || self.cursor >= self.width {
            self.bell();
            return;
        }
        match self.tab_stops.binary_search(&self.cursor) {
            Ok(i) => {
                self.tab_stops.remove(i);
            }
            Err(i) => self.tab_stops.insert(i, self.cursor),
        }
    }

    /// clear every tab stop (the margins stay)
    pub fn clear_tab_stops(&mut self) {
        self.tab_stops.clear();
    }

    /// move to the next tab stop. in the text the cursor just moves; past
    /// the end of it the line is filled with spaces up to the stop, ringing
    /// the margin bell if that passes it. dings if there's no stop ahead
    /// that can be typed at
    pub fn tab(&mut self) {
        // the last column a character can go in
        let limit = if self.margin_release { self.width.saturating_sub(1) } else { self.right_margin };
        let next = self.tab_stops.iter().copied().find(|&s| s > self.cursor);
        let stop = match next {
            Some(s) if self.align.is_none() && s < limit.min(self.width.saturating_sub(1)) => s,
            _ => {
                self.bell();
                return;
            }
        };
        let len = self.buffer.len();
        if stop > len {
            let hot = self.hot_zone_start();
            if len as i64 <= hot && hot < stop as i64 {
                self.ding();
            }
            self.buffer.resize(stop, ' ');
            self.margin_hot_zone = stop as i64 >= hot;
        }
        self.cursor = stop;
    }

    /// words written this session
    pub fn session_words(&self) -> usize {
        self.word_count.saturating_sub(self.start_words)
    }

    /// carry on with an earlier file in save_folder: new lines are added
    /// to it, its last lines show above the typing and the line and
    /// word counts start from its totals
    pub fn carry_on(&mut self, file_name: &str) -> io::Result<()> {
        let text = std::fs::read_to_string(Path::new(&self.save_folder).join(file_name))?;
        let lines: Vec<&str> = text.lines().collect();
        let from = lines.len().saturating_sub(MAX_PREVIOUS);
        self.previous = lines[from..].iter().map(|l| l.to_string()).collect();
        self.line_count = lines.len();
        self.word_count = text.split_whitespace().count();
        self.start_words = self.word_count;
        self.file_name = file_name.to_string();
        Ok(())
    }

    /// keep the finished line, dropping the oldest past MAX_PREVIOUS
    fn buffer_ripple(&mut self) {
        self.previous.push(self.give_buffer());
        if self.previous.len() > MAX_PREVIOUS {
            self.previous.remove(0);
        }
    }

    /// the n-th line back (1 is the line just finished), "" if there
    /// isn't one
    pub fn previous_line(&self, n: usize) -> &str {
        self.previous.len().checked_sub(n).map_or("", |i| &self.previous[i])
    }

    /// the previous_lines lines to show above the typing, oldest (top)
    /// first, blank where there aren't enough yet
    pub fn shown_previous(&self) -> Vec<&str> {
        (1..=self.previous_lines).rev().map(|n| self.previous_line(n)).collect()
    }

    /// finish the line: count it, write it to the file, print it and start
    /// a new one at the left margin. fails only if the file can't be written
    pub fn carriage_return(&mut self) -> Result<(), String> {
        self.margin_hot_zone = false;
        let line = self.give_buffer();
        self.word_count += line.split_whitespace().count();
        self.line_count += 1;
        if self.word_target > 0 && !self.target_reached && self.session_words() >= self.word_target {
            self.target_reached = true;
            self.ding();
        }

        if self.use_file {
            let file_path = Path::new(&self.save_folder).join(&self.file_name);
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)
                .and_then(|mut f| writeln!(f, "{line}"))
                .map_err(|e| {
                    format!("problem writing to the file {}: {e}", file_path.display())
                })?;
        }

        if self.use_printer && self.printer_found {
            self.send_to_printer(&line);
        }

        self.buffer_ripple();
        self.buffer.clear();
        self.buffer.resize(self.left_margin, ' ');
        self.cursor = self.buffer.len();
        self.margin_release = false;
        self.align = None;
        self.align_chars.clear();
        Ok(())
    }

    /// on quitting: finish a line that has something typed on it, as enter
    /// would (to the file and the printer), so it isn't lost. a blank
    /// line is left alone so quitting doesn't add an empty line
    pub fn finish_line(&mut self) -> Result<(), String> {
        if self.give_buffer().trim().is_empty() {
            return Ok(());
        }
        self.carriage_return()
    }

    fn send_to_printer(&mut self, line: &str) {
        let (spacing, align) = (self.current_spacing, self.align);
        if let Some(p) = self.printer.as_mut() {
            if let Err(e) = p.print_line(line, spacing, align) {
                crate::log_warning(&format!("printing failed: {e}"));
            }
        }
    }

    /// set the cursor column as the left margin, unless it's at or past
    /// the right margin
    pub fn margin_set_left(&mut self) {
        if self.cursor >= self.right_margin {
            self.bell();
            return;
        }
        self.left_margin = self.cursor;
    }

    /// set the cursor column as the right margin, unless it's at or before
    /// the left margin or past the width
    pub fn margin_set_right(&mut self) {
        if self.cursor <= self.left_margin || self.cursor > self.width {
            self.bell();
            return;
        }
        self.right_margin = self.cursor;
    }

    /// allow typing past the right margin
    pub fn toggle_margin_release(&mut self) {
        self.margin_release = !self.margin_release;
    }

    pub fn centre_mode(&self) -> bool {
        self.align == Some(Align::Centre)
    }

    pub fn right_mode(&self) -> bool {
        self.align == Some(Align::Right)
    }

    /// toggle centre mode: typed text stays midway between the margins
    pub fn centre_text(&mut self) {
        self.toggle_align(Align::Centre);
    }

    /// toggle right justify: typed text stays flush with the right margin
    pub fn right_justify(&mut self) {
        self.toggle_align(Align::Right);
    }

    /// switch an alignment mode on or off.
    ///
    /// any text already typed on the line is picked up and aligned. while
    /// the mode is on every character typed re-aligns the line. turning it
    /// off moves the text back to the left margin so you can carry on
    /// typing normally. a carriage return also ends it, leaving the line
    /// aligned. switching straight from one mode to the other re-aligns
    /// the text.
    fn toggle_align(&mut self, mode: Align) {
        if self.align == Some(mode) {
            self.align = None;
            let text = std::mem::take(&mut self.align_chars);
            self.buffer.clear();
            self.buffer.resize(self.left_margin, ' ');
            self.buffer.extend(text);
            self.cursor = self.buffer.len();
            return;
        }
        if self.align.is_none() {
            self.align_chars = self.give_buffer().trim().chars().collect();
        }
        self.align = Some(mode);
        self.layout_align();
    }

    /// number of columns available between the margins
    fn align_span(&self) -> i64 {
        self.right_margin as i64 - self.left_margin as i64
    }

    /// add a character to the aligned text, ding if it will not fit
    fn align_add(&mut self, key: char) {
        if self.align_chars.len() as i64 >= self.align_span() {
            self.bell();
            return;
        }
        self.align_chars.push(key);
        self.layout_align();
    }

    /// rebuild the buffer so the text sits centred or against the right
    /// margin
    fn layout_align(&mut self) {
        let gap = self.align_span() - self.align_chars.len() as i64;
        let start = match self.align {
            Some(Align::Centre) => self.left_margin as i64 + gap.div_euclid(2),
            _ => self.left_margin as i64 + gap,
        };
        self.buffer.clear();
        self.buffer.resize(start.max(0) as usize, ' ');
        self.buffer.extend(self.align_chars.iter().copied());
        self.cursor = self.buffer.len();
    }

    /// pause or resume sending lines to the printer. lines typed while
    /// paused still go to the file but are never sent to the printer
    pub fn toggle_printer(&mut self) {
        self.use_printer = !self.use_printer;
    }

    /// show or hide the session time
    pub fn toggle_timer(&mut self) {
        self.show_timer = !self.show_timer;
    }

    /// show or hide the words per minute
    pub fn toggle_wpm(&mut self) {
        self.show_wpm = !self.show_wpm;
    }

    pub fn toggle_help(&mut self) {
        self.help_wanted = !self.help_wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// counts how often a sound was made
    #[derive(Clone, Default)]
    struct Bell(Rc<Cell<u32>>);

    impl Bell {
        fn sound(&self) -> Option<Sound> {
            let c = self.0.clone();
            Some(Box::new(move || c.set(c.get() + 1)))
        }
        fn count(&self) -> u32 {
            self.0.get()
        }
        fn reset(&self) {
            self.0.set(0)
        }
    }

    /// records what would have been printed
    type Printed = Vec<(String, f64, Option<Align>)>;

    #[derive(Clone, Default)]
    struct FakePrinter(Rc<RefCell<Printed>>);

    impl LinePrinter for FakePrinter {
        fn print_line(&mut self, text: &str, spacing: f64, align: Option<Align>) -> io::Result<()> {
            self.0.borrow_mut().push((text.into(), spacing, align));
            Ok(())
        }
    }

    impl FakePrinter {
        fn texts(&self) -> Vec<String> {
            self.0.borrow().iter().map(|l| l.0.clone()).collect()
        }
        fn lines(&self) -> Vec<(String, f64)> {
            self.0.borrow().iter().map(|l| (l.0.clone(), l.1)).collect()
        }
    }

    /// options with no file or printer output, like the python fixture
    fn opts() -> Options {
        Options { use_file: false, use_printer: false, ..Options::default() }
    }

    fn make(o: Options, bell: &Bell) -> Typewriter {
        Typewriter::new(o, None, bell.sound(), None)
    }

    fn quiet(o: Options) -> Typewriter {
        make(o, &Bell::default())
    }

    fn with_printer(o: Options, p: &FakePrinter) -> Typewriter {
        Typewriter::new(o, Some(Box::new(p.clone())), None, None)
    }

    fn type_text(t: &mut Typewriter, text: &str) {
        for ch in text.chars() {
            t.char_add(ch).unwrap();
        }
    }

    /// put the machine at a given line length with the cursor at the end
    fn fill_to(t: &mut Typewriter, length: usize) {
        t.buffer = vec!['x'; length];
        t.cursor = length;
    }

    fn sp(n: usize) -> String {
        " ".repeat(n)
    }

    // ---- setup ----

    #[test]
    fn first_line_starts_at_left_margin() {
        let t = quiet(Options { left_margin: 5, ..opts() });
        assert_eq!(t.give_buffer(), sp(5));
        assert_eq!(t.cursor, 5);
    }

    #[test]
    fn right_margin_defaults_to_width() {
        assert_eq!(quiet(opts()).right_margin, 75);
    }

    #[test]
    fn right_margin_clamped_to_width() {
        assert_eq!(quiet(Options { right_margin: Some(100), ..opts() }).right_margin, 75);
    }

    #[test]
    fn tab_bar_shows_margins() {
        let bar: Vec<char> =
            quiet(Options { left_margin: 5, right_margin: Some(60), ..opts() }).give_tab_bar().chars().collect();
        assert_eq!(bar[5], '!');
        assert_eq!(bar[60], '!');
        assert_eq!(bar.iter().filter(|&&c| c == '!').count(), 2);
    }

    #[test]
    fn tab_bar_marks_hot_zone_start() {
        let bar = quiet(Options { left_margin: 5, right_margin: Some(60), margin_bell: 8, ..opts() })
            .give_tab_bar();
        assert_eq!(bar.as_bytes()[52], b':');
        assert_eq!(bar.matches(':').count(), 1);
        assert!(bar.chars().all(|c| ".!:".contains(c)));
    }

    #[test]
    fn hot_zone_mark_follows_right_margin() {
        let mut t = quiet(Options { right_margin: Some(60), margin_bell: 8, ..opts() });
        t.cursor = 70;
        t.margin_set_right();
        let bar = t.give_tab_bar();
        assert_eq!(bar.as_bytes()[62], b':');
        assert_eq!(bar.as_bytes()[52], b'.');
    }

    #[test]
    fn hot_zone_mark_is_where_the_bell_rings() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(30), margin_bell: 8, ..opts() }, &bell);
        let col = t.give_tab_bar().find(':').unwrap();
        type_text(&mut t, &"x".repeat(col));
        assert_eq!(bell.count(), 0);
        t.char_add('x').unwrap(); // typing at the marked column
        assert_eq!(bell.count(), 1);
    }

    #[test]
    fn hot_zone_plays_ding_not_plain_bell() {
        let (bell, ding) = (Bell::default(), Bell::default());
        let mut t = Typewriter::new(
            Options { right_margin: Some(30), margin_bell: 8, ..opts() },
            None,
            bell.sound(),
            ding.sound(),
        );
        type_text(&mut t, &"x".repeat(23));
        assert_eq!((ding.count(), bell.count()), (1, 0));
        type_text(&mut t, &"x".repeat(7)); // line full: error beep, no ding
        t.char_add('x').unwrap();
        assert_eq!((ding.count(), bell.count()), (1, 1));
    }

    #[test]
    fn bell_off_silences_ding_and_beeps() {
        let (bell, ding) = (Bell::default(), Bell::default());
        let mut t = Typewriter::new(
            Options { right_margin: Some(30), margin_bell: 8, ..opts() },
            None,
            bell.sound(),
            ding.sound(),
        );
        t.toggle_bell();
        assert!(!t.bell_on);
        type_text(&mut t, &"x".repeat(31)); // through the hot zone to full
        t.cursor_right(); // at the end: would beep
        assert_eq!((ding.count(), bell.count()), (0, 0));
        t.toggle_bell();
        t.cursor_right();
        assert_eq!(bell.count(), 1);
    }

    #[test]
    fn bell_can_start_off() {
        let bell = Bell::default();
        let mut t = make(Options { bell_on: false, ..opts() }, &bell);
        t.cursor_left();
        assert_eq!(bell.count(), 0);
    }

    #[test]
    fn margin_wins_if_hot_zone_starts_on_it() {
        let bar = quiet(Options { left_margin: 10, right_margin: Some(18), margin_bell: 8, ..opts() })
            .give_tab_bar();
        assert_eq!(bar.as_bytes()[10], b'!');
        assert!(!bar.contains(':'));
    }

    #[test]
    fn tab_bar_right_marker_at_full_width() {
        let bar = quiet(Options { right_margin: Some(75), ..opts() }).give_tab_bar();
        assert_eq!(bar.len(), 75);
        assert_eq!(bar.as_bytes()[74], b'!');
    }

    #[test]
    fn huge_margin_bell_does_not_panic() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(5), margin_bell: 20, ..opts() }, &bell);
        assert!(!t.give_tab_bar().contains(':'));
        t.char_add('a').unwrap();
        assert!(t.margin_hot_zone);
    }

    // ---- typing and backspace ----

    #[test]
    fn typing_adds_chars_and_moves_cursor() {
        let mut t = quiet(opts());
        type_text(&mut t, "hello");
        assert_eq!(t.give_buffer(), "hello");
        assert_eq!(t.cursor, 5);
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut t = quiet(opts());
        type_text(&mut t, "hello");
        t.backspace();
        assert_eq!(t.give_buffer(), "hell");
        assert_eq!(t.cursor, 4);
    }

    #[test]
    fn backspace_at_column_zero_does_nothing() {
        let mut t = quiet(opts());
        t.backspace();
        assert_eq!(t.give_buffer(), "");
        assert_eq!(t.cursor, 0);
    }

    #[test]
    fn backspace_with_text_but_cursor_at_zero_keeps_text() {
        let mut t = quiet(opts());
        type_text(&mut t, "abc");
        t.cursor = 0;
        t.backspace();
        assert_eq!(t.give_buffer(), "abc");
    }

    #[test]
    fn full_line_rejects_char_and_dings() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(60), ..opts() }, &bell);
        fill_to(&mut t, 60);
        t.char_add('q').unwrap();
        assert_eq!(t.buffer.len(), 60);
        assert!(bell.count() >= 1);
    }

    #[test]
    fn margin_release_allows_typing_past_right_margin() {
        let mut t = quiet(Options { right_margin: Some(60), ..opts() });
        fill_to(&mut t, 60);
        t.toggle_margin_release();
        t.char_add('q').unwrap();
        assert_eq!(t.buffer.len(), 61);
    }

    #[test]
    fn never_types_past_printable_width() {
        let mut t = quiet(Options { right_margin: Some(75), ..opts() });
        t.toggle_margin_release();
        fill_to(&mut t, 74);
        t.char_add('q').unwrap();
        assert_eq!(t.buffer.len(), 74);
    }

    // ---- margin bell and autoreturn ----

    #[test]
    fn margin_bell_rings_at_bell_point() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(60), margin_bell: 8, ..opts() }, &bell);
        fill_to(&mut t, 51);
        t.char_add('a').unwrap();
        assert_eq!(bell.count(), 0);
        t.char_add('b').unwrap(); // buffer was 52 = 60 - 8
        assert_eq!(bell.count(), 1);
        assert!(t.margin_hot_zone);
    }

    #[test]
    fn hot_zone_survives_backspace_and_retype() {
        let mut t = quiet(Options { right_margin: Some(60), margin_bell: 8, ..opts() });
        fill_to(&mut t, 52);
        t.char_add('a').unwrap();
        t.backspace();
        t.char_add('a').unwrap();
        assert!(t.margin_hot_zone);
    }

    #[test]
    fn not_in_hot_zone_before_bell_point() {
        let mut t = quiet(Options { right_margin: Some(60), margin_bell: 8, ..opts() });
        type_text(&mut t, "short");
        assert!(!t.margin_hot_zone);
    }

    #[test]
    fn space_in_hot_zone_dings_without_autoreturn() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(60), margin_bell: 8, ..opts() }, &bell);
        fill_to(&mut t, 55);
        let before = bell.count();
        t.char_add(' ').unwrap();
        assert_eq!(bell.count(), before + 1);
        assert_eq!(t.line_count, 0);
    }

    #[test]
    fn space_or_hyphen_in_hot_zone_autoreturns() {
        for key in [' ', '-'] {
            let mut t = quiet(Options {
                right_margin: Some(60),
                margin_bell: 8,
                autoreturn: true,
                left_margin: 3,
                ..opts()
            });
            fill_to(&mut t, 55);
            t.char_add(key).unwrap();
            assert_eq!(t.line_count, 1);
            assert_eq!(t.give_buffer(), sp(3));
            assert_eq!(t.cursor, 3);
        }
    }

    #[test]
    fn letters_in_hot_zone_do_not_autoreturn() {
        let mut t = quiet(Options { right_margin: Some(60), margin_bell: 8, autoreturn: true, ..opts() });
        fill_to(&mut t, 55);
        t.char_add('a').unwrap();
        assert_eq!(t.line_count, 0);
    }

    #[test]
    fn toggle_autoreturn() {
        let mut t = quiet(opts());
        t.toggle_autoreturn();
        assert!(t.autoreturn);
        t.toggle_autoreturn();
        assert!(!t.autoreturn);
    }

    // ---- carriage return and history ----

    #[test]
    fn carriage_return_resets_line() {
        let mut t = quiet(Options { left_margin: 4, ..opts() });
        type_text(&mut t, "words here");
        t.carriage_return().unwrap();
        assert_eq!(t.give_buffer(), sp(4));
        assert_eq!(t.cursor, 4);
        assert_eq!(t.line_count, 1);
        assert_eq!(t.word_count, 2);
    }

    #[test]
    fn carriage_return_clears_margin_release() {
        let mut t = quiet(opts());
        t.toggle_margin_release();
        t.carriage_return().unwrap();
        assert!(!t.margin_release);
    }

    #[test]
    fn previous_lines_ripple_up() {
        let mut t = quiet(opts());
        for line in ["one", "two", "three", "four"] {
            type_text(&mut t, line);
            t.carriage_return().unwrap();
        }
        assert_eq!(t.previous_line(1), "four");
        assert_eq!(t.previous_line(2), "three");
        assert_eq!(t.previous_line(3), "two");
    }

    #[test]
    fn carriage_return_writes_line_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = quiet(Options { use_file: true, ..opts() });
        t.save_folder = dir.path().display().to_string(); // no trailing slash
        type_text(&mut t, "first");
        t.carriage_return().unwrap();
        type_text(&mut t, "second");
        t.carriage_return().unwrap();
        let text = std::fs::read_to_string(dir.path().join(&t.file_name)).unwrap();
        assert_eq!(text, "first\nsecond\n");
    }

    #[test]
    fn finish_line_saves_and_prints_unfinished_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = FakePrinter::default();
        let mut t = with_printer(Options { use_file: true, use_printer: true, ..opts() }, &p);
        t.save_folder = dir.path().display().to_string();
        type_text(&mut t, "first");
        t.carriage_return().unwrap();
        type_text(&mut t, "no enter yet");
        t.finish_line().unwrap();
        let text = std::fs::read_to_string(dir.path().join(&t.file_name)).unwrap();
        assert_eq!(text, "first\nno enter yet\n");
        assert_eq!(p.texts(), ["first", "no enter yet"]);
        assert_eq!(t.line_count, 2);
    }

    #[test]
    fn finish_line_leaves_blank_line_alone() {
        let dir = tempfile::tempdir().unwrap();
        let p = FakePrinter::default();
        let mut t = with_printer(
            Options { use_file: true, use_printer: true, left_margin: 5, ..opts() },
            &p,
        );
        t.save_folder = dir.path().display().to_string();
        type_text(&mut t, "done");
        t.carriage_return().unwrap();
        t.finish_line().unwrap(); // only the left margin's spaces
        let text = std::fs::read_to_string(dir.path().join(&t.file_name)).unwrap();
        assert_eq!(text, "     done\n");
        assert_eq!(p.texts().len(), 1);
        assert_eq!(t.line_count, 1);
    }

    #[test]
    fn plain_file_name_goes_in_save_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = quiet(Options { use_file: true, ..opts() });
        t.save_folder = dir.path().display().to_string();
        t.file_name = "notes.txt".into();
        std::fs::write(dir.path().join("notes.txt"), "already here\n").unwrap();
        type_text(&mut t, "more");
        t.carriage_return().unwrap();
        let text = std::fs::read_to_string(dir.path().join("notes.txt")).unwrap();
        assert_eq!(text, "already here\nmore\n");
    }

    #[test]
    fn full_file_path_ignores_save_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = quiet(Options { use_file: true, ..opts() });
        t.save_folder = dir.path().join("elsewhere").display().to_string();
        t.file_name = dir.path().join("chosen.txt").display().to_string();
        type_text(&mut t, "hi");
        t.carriage_return().unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("chosen.txt")).unwrap(), "hi\n");
    }

    #[test]
    fn unwritable_save_folder_gives_message() {
        let mut t = quiet(Options { use_file: true, ..opts() });
        t.save_folder = "/nonexistent/folder".into();
        let err = t.carriage_return().unwrap_err();
        assert!(err.contains("problem writing to the file"), "{err}");
    }

    // ---- line spacing ----

    #[test]
    fn line_spacing_cycles() {
        let mut t = quiet(Options { spacing_index: 0, ..opts() });
        let mut seen = vec![t.current_spacing];
        for _ in 0..3 {
            t.toggle_line_space();
            seen.push(t.current_spacing);
        }
        assert_eq!(seen, [1.0, 1.5, 2.0, 1.0]);
    }

    #[test]
    fn saved_spacing_index_restores_spacing() {
        let mut t = quiet(opts());
        t.toggle_line_space(); // 1.5 -> 2
        let restored = quiet(Options { spacing_index: t.spacing_index, ..opts() });
        assert_eq!(restored.current_spacing, 2.0);
    }

    // ---- margins ----

    #[test]
    fn set_left_margin_at_cursor() {
        let mut t = quiet(Options { left_margin: 5, right_margin: Some(60), ..opts() });
        t.cursor = 20;
        t.margin_set_left();
        assert_eq!(t.left_margin, 20);
        let bar = t.give_tab_bar();
        assert_eq!((bar.as_bytes()[20], bar.as_bytes()[5]), (b'!', b'.'));
    }

    #[test]
    fn left_margin_cannot_pass_right_margin() {
        let bell = Bell::default();
        let mut t = make(Options { left_margin: 5, right_margin: Some(60), ..opts() }, &bell);
        t.cursor = 60;
        t.margin_set_left();
        assert_eq!(t.left_margin, 5);
        assert_eq!(bell.count(), 1);
    }

    #[test]
    fn set_right_margin_at_cursor() {
        let mut t = quiet(Options { left_margin: 5, right_margin: Some(60), ..opts() });
        t.cursor = 70;
        t.margin_set_right();
        assert_eq!(t.right_margin, 70);
        let bar = t.give_tab_bar();
        assert_eq!((bar.as_bytes()[70], bar.as_bytes()[60]), (b'!', b'.'));
    }

    #[test]
    fn right_margin_can_be_set_to_full_width() {
        let mut t = quiet(Options { right_margin: Some(60), ..opts() });
        t.cursor = 75;
        t.margin_set_right();
        assert_eq!(t.right_margin, 75);
        assert_eq!(t.give_tab_bar().as_bytes()[74], b'!');
    }

    #[test]
    fn invalid_right_margin_rejected() {
        for column in [5, 2, 76] {
            let bell = Bell::default();
            let mut t = make(Options { left_margin: 5, right_margin: Some(60), ..opts() }, &bell);
            t.cursor = column;
            t.margin_set_right();
            assert_eq!(t.right_margin, 60);
            assert_eq!(bell.count(), 1);
        }
    }

    // ---- centring ----

    #[test]
    fn centre_text_centres_between_margins() {
        let mut t = quiet(Options { left_margin: 10, right_margin: Some(60), ..opts() });
        t.centre_text();
        type_text(&mut t, "Chapter One");
        let line = t.give_buffer();
        assert_eq!(line.trim(), "Chapter One");
        assert_eq!(line.find('C').unwrap(), 10 + (50 - 11) / 2);
        assert_eq!(t.cursor, line.len());
    }

    #[test]
    fn centre_picks_up_existing_text() {
        let mut t = quiet(Options { left_margin: 10, right_margin: Some(60), ..opts() });
        type_text(&mut t, "Title");
        t.centre_text();
        assert_eq!(t.give_buffer().find('T').unwrap(), 10 + (50 - 5) / 2);
    }

    #[test]
    fn centre_backspace_recentres() {
        let mut t = quiet(Options { right_margin: Some(60), ..opts() });
        t.centre_text();
        type_text(&mut t, "abcd");
        t.backspace();
        let line = t.give_buffer();
        assert_eq!(line.trim(), "abc");
        assert_eq!(line.find('a').unwrap(), (60 - 3) / 2);
    }

    #[test]
    fn centre_backspace_on_empty_does_nothing() {
        let mut t = quiet(opts());
        t.centre_text();
        t.backspace();
        assert!(t.centre_mode());
        assert_eq!(t.give_buffer().trim(), "");
    }

    #[test]
    fn centre_dings_when_full() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(10), ..opts() }, &bell);
        t.centre_text();
        type_text(&mut t, &"x".repeat(10));
        let before = bell.count();
        t.char_add('y').unwrap();
        assert_eq!(bell.count(), before + 1);
        assert_eq!(t.give_buffer(), "x".repeat(10));
    }

    #[test]
    fn carriage_return_ends_centre_mode() {
        let mut t = quiet(Options { left_margin: 10, right_margin: Some(60), ..opts() });
        t.centre_text();
        type_text(&mut t, "Hi");
        t.carriage_return().unwrap();
        assert!(!t.centre_mode());
        assert_eq!(t.previous_line(1).trim(), "Hi");
        assert_eq!(t.give_buffer(), sp(10));
    }

    #[test]
    fn toggling_centre_off_returns_text_to_left_margin() {
        let mut t = quiet(Options { left_margin: 4, right_margin: Some(60), ..opts() });
        t.centre_text();
        type_text(&mut t, "Hi");
        t.centre_text();
        assert!(!t.centre_mode());
        assert_eq!(t.give_buffer(), "    Hi");
        assert_eq!(t.cursor, 6);
        t.char_add('!').unwrap();
        assert_eq!(t.give_buffer(), "    Hi!");
    }

    #[test]
    fn pausing_printer_keeps_lines_in_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = FakePrinter::default();
        let mut t = with_printer(Options { use_printer: true, use_file: true, ..opts() }, &p);
        t.save_folder = dir.path().display().to_string();
        type_text(&mut t, "one");
        t.carriage_return().unwrap();
        t.toggle_printer();
        assert!(!t.use_printer);
        type_text(&mut t, "two");
        t.carriage_return().unwrap();
        t.toggle_printer();
        assert!(t.use_printer);
        type_text(&mut t, "three");
        t.carriage_return().unwrap();
        // paused line is never sent, not even after resuming
        assert_eq!(p.texts(), ["one", "three"]);
        let text = std::fs::read_to_string(dir.path().join(&t.file_name)).unwrap();
        assert_eq!(text, "one\ntwo\nthree\n");
    }

    // ---- line editing ----

    #[test]
    fn arrows_move_cursor_and_typing_inserts() {
        let mut t = quiet(opts());
        type_text(&mut t, "helo");
        t.cursor_left();
        t.char_add('l').unwrap();
        assert_eq!(t.give_buffer(), "hello");
        assert_eq!(t.cursor, 4);
    }

    #[test]
    fn cursor_stops_at_ends_and_dings() {
        let bell = Bell::default();
        let mut t = make(opts(), &bell);
        type_text(&mut t, "ab");
        t.cursor_right();
        assert_eq!((t.cursor, bell.count()), (2, 1));
        t.cursor_left();
        t.cursor_left();
        t.cursor_left();
        assert_eq!((t.cursor, bell.count()), (0, 2));
    }

    #[test]
    fn backspace_mid_line() {
        let mut t = quiet(opts());
        type_text(&mut t, "abXc");
        t.cursor_left();
        t.backspace();
        assert_eq!(t.give_buffer(), "abc");
        assert_eq!(t.cursor, 2);
    }

    #[test]
    fn delete_under_cursor() {
        let mut t = quiet(opts());
        type_text(&mut t, "abXc");
        t.cursor_left();
        t.cursor_left();
        t.delete();
        assert_eq!(t.give_buffer(), "abc");
        assert_eq!(t.cursor, 2);
        t.cursor_end();
        t.delete(); // nothing under the cursor at the end
        assert_eq!(t.give_buffer(), "abc");
    }

    #[test]
    fn home_goes_to_margin_then_line_start() {
        let mut t = quiet(Options { left_margin: 5, ..opts() });
        type_text(&mut t, "hello");
        t.cursor_home();
        assert_eq!(t.cursor, 5);
        t.cursor_home();
        assert_eq!(t.cursor, 0);
        t.cursor_end();
        assert_eq!(t.cursor, 10);
    }

    #[test]
    fn insert_on_full_line_dings_without_changing_it() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(10), autoreturn: true, ..opts() }, &bell);
        type_text(&mut t, "abcdefghij");
        t.cursor_home();
        bell.reset();
        t.char_add('X').unwrap();
        assert_eq!(t.give_buffer(), "abcdefghij");
        assert_eq!(t.line_count, 0); // no autoreturn mid-line
        assert!(bell.count() >= 1);
    }

    #[test]
    fn space_mid_line_in_hot_zone_does_not_autoreturn() {
        let mut t = quiet(Options { right_margin: Some(20), margin_bell: 8, autoreturn: true, ..opts() });
        type_text(&mut t, "abcdefghijklmn"); // into the hot zone
        t.cursor_left();
        t.cursor_left();
        t.char_add(' ').unwrap();
        assert_eq!(t.line_count, 0);
        assert_eq!(t.give_buffer(), "abcdefghijkl mn");
    }

    #[test]
    fn edited_line_is_what_gets_printed() {
        let p = FakePrinter::default();
        let mut t = with_printer(Options { use_printer: true, ..opts() }, &p);
        type_text(&mut t, "teh cat");
        t.cursor_home();
        t.cursor_right();
        t.cursor_right();
        t.delete();
        t.cursor_left();
        t.char_add('h').unwrap();
        t.carriage_return().unwrap();
        assert_eq!(p.texts()[0], "the cat");
    }

    #[test]
    fn delete_word_at_end() {
        let mut t = quiet(opts());
        type_text(&mut t, "hello big world");
        t.delete_word();
        assert_eq!(t.give_buffer(), "hello big ");
        t.delete_word(); // skips the space, then deletes "big"
        assert_eq!(t.give_buffer(), "hello ");
        assert_eq!(t.cursor, 6);
    }

    #[test]
    fn delete_word_mid_line() {
        let mut t = quiet(opts());
        type_text(&mut t, "one two three");
        for _ in 0..6 {
            t.cursor_left(); // cursor just after "two"
        }
        t.delete_word();
        assert_eq!(t.give_buffer(), "one  three");
        assert_eq!(t.cursor, 4);
    }

    #[test]
    fn delete_word_stops_at_left_margin() {
        let mut t = quiet(Options { left_margin: 5, ..opts() });
        type_text(&mut t, "hello");
        t.delete_word();
        assert_eq!(t.give_buffer(), sp(5));
        assert_eq!(t.cursor, 5);
        t.delete_word(); // at the margin: clears to line start
        assert_eq!(t.give_buffer(), "");
        assert_eq!(t.cursor, 0);
    }

    #[test]
    fn delete_word_on_empty_line_does_nothing() {
        let mut t = quiet(opts());
        t.delete_word();
        assert_eq!((t.give_buffer().as_str(), t.cursor), ("", 0));
    }

    #[test]
    fn delete_word_in_right_justify() {
        let mut t = quiet(Options { right_margin: Some(20), ..opts() });
        t.right_justify();
        type_text(&mut t, "ab cd");
        t.delete_word();
        assert_eq!(t.give_buffer(), sp(17) + "ab ");
        assert!(t.right_mode());
    }

    #[test]
    fn arrows_ignored_in_align_mode() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(20), ..opts() }, &bell);
        t.right_justify();
        type_text(&mut t, "ab");
        t.cursor_left();
        assert_eq!((t.cursor, bell.count()), (20, 1));
    }

    // ---- right justify ----

    #[test]
    fn right_justify_sits_against_right_margin() {
        let mut t = quiet(Options { left_margin: 5, right_margin: Some(40), ..opts() });
        t.right_justify();
        type_text(&mut t, "hello");
        assert_eq!(t.give_buffer(), sp(35) + "hello");
        assert_eq!(t.cursor, 40);
    }

    #[test]
    fn right_justify_picks_up_existing_text() {
        let mut t = quiet(Options { right_margin: Some(20), ..opts() });
        type_text(&mut t, "abc");
        t.right_justify();
        assert_eq!(t.give_buffer(), sp(17) + "abc");
    }

    #[test]
    fn right_justify_backspace() {
        let mut t = quiet(Options { right_margin: Some(20), ..opts() });
        t.right_justify();
        type_text(&mut t, "abcd");
        t.backspace();
        assert_eq!(t.give_buffer(), sp(17) + "abc");
    }

    #[test]
    fn right_justify_dings_when_full() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(4), ..opts() }, &bell);
        t.right_justify();
        type_text(&mut t, "abcd");
        bell.reset();
        t.char_add('e').unwrap();
        assert_eq!(t.give_buffer(), "abcd");
        assert_eq!(bell.count(), 1);
    }

    #[test]
    fn right_justify_line_is_printed_aligned() {
        let p = FakePrinter::default();
        let mut t = with_printer(Options { right_margin: Some(10), use_printer: true, ..opts() }, &p);
        t.right_justify();
        type_text(&mut t, "end");
        t.carriage_return().unwrap();
        assert_eq!(p.texts()[0], sp(7) + "end");
        assert_eq!(p.0.borrow()[0].2, Some(Align::Right));
        assert!(!t.right_mode());
        assert_eq!(t.give_buffer(), "");
    }

    #[test]
    fn switch_from_centre_to_right() {
        let mut t = quiet(Options { right_margin: Some(20), ..opts() });
        t.centre_text();
        type_text(&mut t, "ab");
        t.right_justify();
        assert!(t.right_mode() && !t.centre_mode());
        assert_eq!(t.give_buffer(), sp(18) + "ab");
    }

    #[test]
    fn toggling_right_off_returns_text_to_left_margin() {
        let mut t = quiet(Options { left_margin: 2, right_margin: Some(20), ..opts() });
        t.right_justify();
        type_text(&mut t, "ab");
        t.right_justify();
        assert!(!t.right_mode());
        assert_eq!(t.give_buffer(), "  ab");
        t.char_add('!').unwrap();
        assert_eq!(t.give_buffer(), "  ab!");
    }

    #[test]
    fn toggling_off_after_switching_modes_returns_left() {
        let mut t = quiet(Options { right_margin: Some(20), ..opts() });
        t.centre_text();
        type_text(&mut t, "ab");
        t.right_justify();
        t.right_justify();
        assert_eq!(t.align, None);
        assert_eq!(t.give_buffer(), "ab");
    }

    #[test]
    fn overlong_text_aligned_without_panicking() {
        let mut t = quiet(Options { right_margin: Some(10), ..opts() });
        t.toggle_margin_release();
        type_text(&mut t, "abcdefghijklmno");
        t.centre_text();
        assert_eq!(t.give_buffer(), "abcdefghijklmno");
    }

    // ---- printer ----

    #[test]
    fn no_printer_means_no_printer_found() {
        assert!(!quiet(Options { use_printer: true, ..opts() }).printer_found);
    }

    #[test]
    fn carriage_return_sends_line_and_spacing_to_printer() {
        let p = FakePrinter::default();
        let mut t = with_printer(Options { use_printer: true, ..opts() }, &p);
        assert!(t.printer_found);
        type_text(&mut t, "hello");
        t.toggle_line_space(); // 1.5 -> 2
        t.carriage_return().unwrap();
        assert_eq!(p.lines(), [("hello".to_string(), 2.0)]);
    }

    #[test]
    fn printer_not_used_when_use_printer_off() {
        let p = FakePrinter::default();
        let mut t = with_printer(opts(), &p);
        type_text(&mut t, "hello");
        t.carriage_return().unwrap();
        assert!(p.lines().is_empty());
    }

    #[test]
    fn blank_carriage_return_sends_blank_line_to_printer() {
        let p = FakePrinter::default();
        let mut t = with_printer(Options { left_margin: 4, use_printer: true, ..opts() }, &p);
        t.carriage_return().unwrap();
        t.toggle_line_space(); // 1.5 -> 2
        t.carriage_return().unwrap();
        assert_eq!(p.lines(), [(sp(4), 1.5), (sp(4), 2.0)]);
    }

    // ---- tab stops ----

    fn tabs(stops: &[usize]) -> Options {
        Options { tab_stops: stops.to_vec(), ..opts() }
    }

    #[test]
    fn tab_stops_sorted_and_kept_on_the_page() {
        let t = quiet(Options { width: 40, ..tabs(&[20, 5, 20, 99]) });
        assert_eq!(t.tab_stops, [5, 20]);
    }

    #[test]
    fn tab_pads_to_the_next_stop() {
        let mut t = quiet(tabs(&[5, 20]));
        type_text(&mut t, "ab");
        t.tab();
        assert_eq!((t.give_buffer(), t.cursor), (format!("ab{}", sp(3)), 5));
        t.tab();
        type_text(&mut t, "c");
        assert_eq!(t.give_buffer(), format!("ab{}c", sp(18)));
    }

    #[test]
    fn tab_inside_the_text_just_moves() {
        let mut t = quiet(tabs(&[3]));
        type_text(&mut t, "hello there");
        t.cursor_home();
        t.tab();
        assert_eq!((t.give_buffer(), t.cursor), ("hello there".to_string(), 3));
    }

    #[test]
    fn tab_with_no_stop_ahead_dings() {
        let bell = Bell::default();
        let mut t = make(tabs(&[3]), &bell);
        type_text(&mut t, "hello");
        t.tab();
        assert_eq!((t.give_buffer(), bell.count()), ("hello".to_string(), 1));
        let mut t = make(opts(), &bell);
        t.tab();
        assert_eq!(bell.count(), 2);
    }

    #[test]
    fn tab_stops_at_the_right_margin_unless_released() {
        let bell = Bell::default();
        let mut t = make(Options { right_margin: Some(30), ..tabs(&[30, 40]) }, &bell);
        t.tab();
        assert_eq!((t.cursor, bell.count()), (0, 1));
        t.toggle_margin_release();
        t.tab();
        assert_eq!(t.cursor, 30);
    }

    #[test]
    fn tab_past_the_bell_point_rings_the_margin_bell() {
        let (bell, ding) = (Bell::default(), Bell::default());
        let o = Options { right_margin: Some(40), margin_bell: 8, ..tabs(&[30, 35]) };
        let mut t = Typewriter::new(o, None, bell.sound(), ding.sound());
        t.tab(); // to 30, the bell point is 32
        assert_eq!(ding.count(), 0);
        t.tab();
        assert!(t.margin_hot_zone);
        assert_eq!(ding.count(), 1);
    }

    #[test]
    fn alt_t_sets_and_clears_a_stop_at_the_cursor() {
        let mut t = quiet(opts());
        type_text(&mut t, "abcd");
        t.toggle_tab_stop();
        t.cursor_home();
        t.toggle_tab_stop();
        assert_eq!(t.tab_stops, [0, 4]);
        // the margin mark wins over a stop on the same column
        assert_eq!(&t.give_tab_bar()[..6], "!...,.");
        t.cursor_end();
        t.toggle_tab_stop();
        assert_eq!(t.tab_stops, [0]);
        t.clear_tab_stops();
        assert!(t.tab_stops.is_empty());
    }

    // ---- session and carrying on ----

    #[test]
    fn word_target_rings_once() {
        let (bell, ding) = (Bell::default(), Bell::default());
        let o = Options { word_target: 3, ..opts() };
        let mut t = Typewriter::new(o, None, bell.sound(), ding.sound());
        type_text(&mut t, "one two");
        t.carriage_return().unwrap();
        assert!(!t.target_reached);
        type_text(&mut t, "three");
        t.carriage_return().unwrap();
        type_text(&mut t, "four");
        t.carriage_return().unwrap();
        assert!(t.target_reached);
        assert_eq!((ding.count(), t.session_words()), (1, 4));
    }

    #[test]
    fn carry_on_picks_up_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "a b\n\nc d e\nf\n").unwrap();
        let mut t = quiet(Options { use_file: true, word_target: 2, ..opts() });
        t.save_folder = dir.path().display().to_string();
        t.carry_on("old.txt").unwrap();
        assert_eq!([t.previous_line(3), t.previous_line(2), t.previous_line(1)], ["", "c d e", "f"]);
        assert_eq!((t.line_count, t.word_count, t.session_words()), (4, 6, 0));
        type_text(&mut t, "g h");
        t.carriage_return().unwrap();
        assert_eq!((t.word_count, t.session_words()), (8, 2));
        assert!(t.target_reached);
        let text = std::fs::read_to_string(dir.path().join("old.txt")).unwrap();
        assert!(text.ends_with("f\ng h\n"));
    }

    #[test]
    fn previous_lines_kept_up_to_the_most() {
        let mut t = quiet(Options { previous_lines: 2, ..opts() });
        for i in 0..10 {
            type_text(&mut t, &i.to_string());
            t.carriage_return().unwrap();
        }
        assert_eq!(t.previous.len(), MAX_PREVIOUS);
        assert_eq!(t.shown_previous(), ["8", "9"]);
        t.previous_lines = 0;
        assert!(t.shown_previous().is_empty());
        t.previous_lines = 7;
        assert_eq!(t.shown_previous(), ["3", "4", "5", "6", "7", "8", "9"]);
    }

    #[test]
    fn previous_lines_capped() {
        assert_eq!(quiet(Options { previous_lines: 99, ..opts() }).previous_lines, MAX_PREVIOUS);
    }

    #[test]
    fn carry_on_shows_as_many_lines_as_wanted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n").unwrap();
        let mut t = quiet(Options { previous_lines: 5, ..opts() });
        t.save_folder = dir.path().display().to_string();
        t.carry_on("old.txt").unwrap();
        assert_eq!(t.shown_previous(), ["5", "6", "7", "8", "9"]);
        assert_eq!(t.line_count, 9);
    }

    #[test]
    fn carry_on_short_file_leaves_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "only\n").unwrap();
        let mut t = quiet(opts());
        t.save_folder = dir.path().display().to_string();
        t.carry_on("old.txt").unwrap();
        assert_eq!([t.previous_line(3), t.previous_line(2), t.previous_line(1)], ["", "", "only"]);
    }
}
