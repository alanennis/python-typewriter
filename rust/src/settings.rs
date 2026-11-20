//! the settings a front end can offer for editing all at once (the
//! window's settings panel): read them from the machine, check them,
//! then put them on the machine and save them to the config

use crate::config::{self, Setting};
use crate::typewriter::{MAX_PREVIOUS, SPACING_CHOICES, Typewriter};
use std::path::Path;

/// narrowest and widest page allowed
pub const WIDTHS: std::ops::RangeInclusive<usize> = 10..=250;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub width: usize,
    pub left_margin: usize,
    pub right_margin: usize,
    pub margin_bell: usize,
    pub spacing_index: usize,
    pub autoreturn: bool,
    pub bell_on: bool,
    pub use_printer: bool,
    pub show_timer: bool,
    pub show_wpm: bool,
    pub previous_lines: usize,
    pub word_target: usize,
    /// as typed: column numbers split by commas or spaces
    pub tab_stops: String,
    pub save_folder: String,
}

/// tab stops as written in the settings, like "10, 20 30"
pub fn parse_tab_stops(text: &str) -> Result<Vec<usize>, String> {
    let mut stops = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<usize>().map_err(|_| format!("tab stop \"{s}\" isn't a column number")))
        .collect::<Result<Vec<_>, _>>()?;
    stops.sort_unstable();
    stops.dedup();
    Ok(stops)
}

pub fn tab_stops_text(stops: &[usize]) -> String {
    stops.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
}

impl Settings {
    pub fn from_machine(m: &Typewriter) -> Self {
        Settings {
            width: m.width,
            left_margin: m.left_margin,
            right_margin: m.right_margin,
            margin_bell: m.margin_bell,
            spacing_index: m.spacing_index,
            autoreturn: m.autoreturn,
            bell_on: m.bell_on,
            use_printer: m.use_printer,
            show_timer: m.show_timer,
            show_wpm: m.show_wpm,
            previous_lines: m.previous_lines,
            word_target: m.word_target,
            tab_stops: tab_stops_text(&m.tab_stops),
            save_folder: m.save_folder.clone(),
        }
    }

    /// what's wrong with the settings, if anything; gives back the tab
    /// stops when they're all right
    pub fn check(&self) -> Result<Vec<usize>, String> {
        if !WIDTHS.contains(&self.width) {
            return Err(format!("width must be {} to {}", WIDTHS.start(), WIDTHS.end()));
        }
        if self.right_margin > self.width {
            return Err(format!("the right margin can't be past the width ({})", self.width));
        }
        if self.left_margin >= self.right_margin {
            return Err("the left margin must be left of the right margin".into());
        }
        if self.previous_lines > MAX_PREVIOUS {
            return Err(format!("previous lines must be 0 to {MAX_PREVIOUS}"));
        }
        if self.spacing_index >= SPACING_CHOICES.len() {
            return Err("unknown line spacing".into());
        }
        let stops = parse_tab_stops(&self.tab_stops)?;
        if let Some(s) = stops.iter().find(|&&s| s >= self.width) {
            return Err(format!("tab stop {s} is past the width ({})", self.width));
        }
        if !Path::new(&self.save_folder).is_dir() {
            return Err(format!("there's no folder {}", self.save_folder));
        }
        Ok(stops)
    }

    /// check the settings, put them on the machine and save them to the
    /// config. nothing changes if they aren't right. a line with nothing
    /// typed on it yet moves to the new left margin; one with text keeps
    /// it, and the new margin starts on the next line
    pub fn apply(&self, m: &mut Typewriter, config_path: &Path) -> Result<(), String> {
        let stops = self.check()?;
        let blank_line = m.align.is_none() && m.give_buffer().trim().is_empty();
        m.width = self.width;
        m.left_margin = self.left_margin;
        m.right_margin = self.right_margin;
        m.margin_bell = self.margin_bell;
        m.spacing_index = self.spacing_index;
        m.current_spacing = SPACING_CHOICES[self.spacing_index];
        m.autoreturn = self.autoreturn;
        m.bell_on = self.bell_on;
        m.use_printer = self.use_printer;
        m.show_timer = self.show_timer;
        m.show_wpm = self.show_wpm;
        m.previous_lines = self.previous_lines;
        // a new target counts the words already written this session, but
        // only rings the bell when it's reached from now on
        if m.word_target != self.word_target {
            m.target_reached = self.word_target > 0 && m.session_words() >= self.word_target;
        }
        m.word_target = self.word_target;
        m.tab_stops = stops.clone();
        m.save_folder = self.save_folder.clone();
        if blank_line {
            m.buffer = vec![' '; self.left_margin];
            m.cursor = self.left_margin;
        }
        m.cursor = m.cursor.min(m.buffer.len());

        let settings: [(&str, Setting); 14] = [
            ("width", self.width.into()),
            ("left_margin", self.left_margin.into()),
            ("right_margin", self.right_margin.into()),
            ("margin_bell", self.margin_bell.into()),
            ("spacing_index", self.spacing_index.into()),
            ("autoreturn", self.autoreturn.into()),
            ("bell", self.bell_on.into()),
            ("use_printer", self.use_printer.into()),
            ("show_timer", self.show_timer.into()),
            ("show_wpm", self.show_wpm.into()),
            ("previous_lines", self.previous_lines.into()),
            ("word_target", self.word_target.into()),
            ("tab_stops", stops.as_slice().into()),
            ("save_folder", self.save_folder.as_str().into()),
        ];
        config::save_settings(config_path, &settings)
            .map_err(|e| format!("can't save settings to {}: {e}", config_path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::typewriter::Options;

    fn machine(folder: &Path) -> Typewriter {
        let opts = Options { use_file: false, use_printer: false, ..Options::default() };
        let mut m = Typewriter::new(opts, None, None, None);
        m.save_folder = folder.display().to_string();
        m
    }

    #[test]
    fn tab_stops_written_any_way() {
        assert_eq!(parse_tab_stops("20, 10  30,,").unwrap(), [10, 20, 30]);
        assert!(parse_tab_stops("").unwrap().is_empty());
        assert!(parse_tab_stops("10, x").is_err());
        assert_eq!(tab_stops_text(&[5, 12]), "5, 12");
    }

    #[test]
    fn bad_settings_are_refused_and_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "width = 75\n").unwrap();
        let mut m = machine(dir.path());
        let good = Settings::from_machine(&m);
        assert!(good.check().is_ok());
        let bad = [
            Settings { width: 5, ..good.clone() },
            Settings { right_margin: 76, ..good.clone() },
            Settings { left_margin: 75, ..good.clone() },
            Settings { previous_lines: 8, ..good.clone() },
            Settings { tab_stops: "10, 80".into(), ..good.clone() },
            Settings { tab_stops: "ten".into(), ..good.clone() },
            Settings { save_folder: dir.path().join("nope").display().to_string(), ..good.clone() },
        ];
        for s in bad {
            assert!(s.apply(&mut m, &path).is_err(), "{s:?}");
        }
        assert_eq!(Settings::from_machine(&m), good);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "width = 75\n");
    }

    #[test]
    fn apply_sets_the_machine_and_saves_everything() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "width = 75 # page\nsave_folder = \"/old/\"\n").unwrap();
        let mut m = machine(dir.path());
        let other = tempfile::tempdir().unwrap();
        let s = Settings {
            width: 60,
            left_margin: 5,
            right_margin: 55,
            margin_bell: 6,
            spacing_index: 2,
            autoreturn: true,
            bell_on: false,
            use_printer: true,
            show_timer: false,
            show_wpm: false,
            previous_lines: 6,
            word_target: 300,
            tab_stops: "30 10".into(),
            save_folder: other.path().display().to_string(),
        };
        s.apply(&mut m, &path).unwrap();
        let mut back = s.clone();
        back.tab_stops = "10, 30".into();
        assert_eq!(Settings::from_machine(&m), back);
        assert_eq!((m.current_spacing, m.cursor, m.give_buffer()), (2.0, 5, " ".repeat(5)));

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("width = 60 # page"), "{text}");
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.str("save_folder"), Some(other.path().to_str().unwrap()));
        assert_eq!(cfg.usize_list("tab_stops").unwrap(), [10, 30]);
        assert_eq!((cfg.int("previous_lines"), cfg.int("word_target")), (Some(6), Some(300)));
        assert_eq!((cfg.bool("bell"), cfg.bool("show_wpm")), (Some(false), Some(false)));
    }

    #[test]
    fn typed_line_keeps_its_margin_until_the_next_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let mut m = machine(dir.path());
        for c in "hello".chars() {
            m.char_add(c).unwrap();
        }
        Settings { left_margin: 8, ..Settings::from_machine(&m) }.apply(&mut m, &path).unwrap();
        assert_eq!(m.give_buffer(), "hello");
        m.carriage_return().unwrap();
        assert_eq!(m.cursor, 8);
    }

    #[test]
    fn new_target_already_met_does_not_ring() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let mut m = machine(dir.path());
        m.word_count = 50;
        Settings { word_target: 40, ..Settings::from_machine(&m) }.apply(&mut m, &path).unwrap();
        assert!(m.target_reached);
        Settings { word_target: 100, ..Settings::from_machine(&m) }.apply(&mut m, &path).unwrap();
        assert!(!m.target_reached);
    }
}
