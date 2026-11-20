//! what the terminal and window versions have in common: the command
//! line, building the machine from the config, the alt-key commands and
//! the layout of the settings and help panel. each front end only draws
//! and reads keys in its own way.

use crate::config::{self, Config, Setting};
use crate::printer::{open_printer, printer_name};
use crate::typewriter::{LinePrinter, MAX_PREVIOUS, Options, Typewriter};
use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum PrinterChoice {
    Epson,
    Zebra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum YesNo {
    #[value(name = "True")]
    True,
    #[value(name = "False")]
    False,
}

/// typewriter for a line printer. anything given here overrides config.toml
#[derive(Parser, Debug)]
pub struct Args {
    /// save typed lines to a file (overrides use_file)
    pub use_file: Option<YesNo>,
    /// printer to use (overrides printer in config.toml)
    #[arg(short, long, value_enum)]
    pub printer: Option<PrinterChoice>,
    /// config file to use (default: config.toml next to this binary,
    /// created on the first run)
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// file to write to instead of a dated one. a plain name goes in
    /// save_folder; a path (./test.txt, ~/notes.txt) is taken from the
    /// current folder. if it already exists lines are added to the end and
    /// its last lines are shown. turns on saving to a file
    #[arg(short, long, value_name = "NAME")]
    pub file: Option<String>,
    /// carry on with the newest dated file in save_folder: lines are added
    /// to it and its last lines are shown. turns on saving to a file
    #[arg(long = "continue")]
    pub carry_on: bool,
    /// finished lines to show above the typing, 0 to 7. saved to
    /// previous_lines in config.toml for next time
    #[arg(short, long, value_name = "N", value_parser = clap::value_parser!(u8).range(0..=MAX_PREVIOUS as i64))]
    pub lines: Option<u8>,
}

impl Args {
    /// parse and check the options go together
    pub fn parse_checked<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let a = Args::try_parse_from(args)?;
        if a.file.is_some() && a.use_file == Some(YesNo::False) {
            return Err(<Args as clap::CommandFactory>::command().error(
                clap::error::ErrorKind::ArgumentConflict,
                "--file and False don't go together: False turns off the file",
            ));
        }
        if a.carry_on && (a.file.is_some() || a.use_file == Some(YesNo::False)) {
            return Err(<Args as clap::CommandFactory>::command().error(
                clap::error::ErrorKind::ArgumentConflict,
                "--continue doesn't go with --file or False: it picks the file itself",
            ));
        }
        Ok(a)
    }
}

/// where --file NAME writes. a plain name goes in save_folder (it's
/// returned as it is, to be joined on later); a path with a folder in it,
/// like ./test.txt, is taken from the current folder, so it's made into a
/// full path now
pub fn file_arg_path(name: &str) -> String {
    if name.contains('/') {
        if let Ok(p) = std::path::absolute(name) {
            return p.display().to_string();
        }
    }
    name.to_string()
}

/// is this a name the typewriter gives a new file, like
/// 2026-11-17-202444.txt
pub fn is_dated_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 21
        && name.ends_with(".txt")
        && b[..17].iter().enumerate().all(|(i, c)| match i {
            4 | 7 | 10 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// the newest dated file in folder (the names sort by date and time)
pub fn latest_dated_file(folder: &Path) -> Option<String> {
    std::fs::read_dir(folder)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| is_dated_name(n))
        .max()
}

/// the config, read and ready to build a machine from
pub struct Setup {
    pub config_path: PathBuf,
    pub config: Config,
    /// the config file was just made from the starting settings
    pub created: bool,
}

impl Setup {
    /// find (or create) and read the config, with --printer applied and
    /// --lines saved to it
    pub fn load(args: &Args) -> Result<Self, String> {
        let (config_path, created) = match &args.config {
            Some(p) => (p.clone(), false),
            None => {
                let p = config::default_config_path()?;
                let created = config::create_if_missing(&p)?;
                (p, created)
            }
        };
        if let Some(n) = args.lines {
            save(&config_path, &[("previous_lines", (n as usize).into())]);
        }
        let mut config = Config::load(&config_path)?;
        match args.printer {
            Some(PrinterChoice::Epson) => config.set_str("printer", "epson"),
            Some(PrinterChoice::Zebra) => config.set_str("printer", "zebra"),
            None => {}
        }
        Ok(Setup { config_path, config, created })
    }

    /// the machine's settings from the config and command line
    pub fn options(&self, args: &Args) -> Result<Options, String> {
        let cfg = &self.config;
        let width = cfg.require_usize("width")?;
        Ok(Options {
            width,
            autoreturn: cfg.require_bool("autoreturn")?,
            spacing_index: cfg.require_usize("spacing_index")?,
            margin_bell: cfg.require_usize("margin_bell")?,
            left_margin: cfg.require_usize("left_margin")?,
            right_margin: Some(cfg.int("right_margin").map_or(width, |v| v.max(0) as usize)),
            use_file: match args.use_file {
                Some(v) => v == YesNo::True,
                None => cfg.bool("use_file").unwrap_or(true),
            },
            use_printer: cfg.bool("use_printer").unwrap_or(true),
            printer_name: printer_name(cfg),
            bell_on: cfg.bool("bell").unwrap_or(true),
            tab_stops: cfg.usize_list("tab_stops")?,
            word_target: cfg.int("word_target").map_or(0, |v| v.max(0) as usize),
            show_timer: cfg.bool("show_timer").unwrap_or(true),
            show_wpm: cfg.bool("show_wpm").unwrap_or(true),
            previous_lines: match args.lines {
                Some(n) => n as usize,
                None => match cfg.int("previous_lines") {
                    None => 3,
                    Some(n) if (0..=MAX_PREVIOUS as i64).contains(&n) => n as usize,
                    Some(_) => return Err(format!("config needs previous_lines set to 0 to {MAX_PREVIOUS}")),
                },
            },
        })
    }

    /// the printer the config names. it's always opened, so alt-p can turn
    /// printing on even if use_printer starts off false in the config
    pub fn printer(&self) -> Result<Option<Box<dyn LinePrinter>>, String> {
        open_printer(&self.config)
    }

    /// the bell_sound setting, and the folder a relative one is found in
    pub fn bell_sound(&self) -> (&str, &Path) {
        let file = self.config.str("bell_sound").unwrap_or("ding.wav");
        (file, self.config_path.parent().unwrap_or(Path::new(".")))
    }

    /// where the new machine saves: save_folder, and --file or --continue
    /// if given. a --file that's already there is carried on with, the same
    /// as --continue. gives back a note about which file to show at startup
    pub fn set_file(&self, m: &mut Typewriter, args: &Args) -> Result<Option<String>, String> {
        m.save_folder = self.config.require_str("save_folder")?.to_string();
        if let Some(file) = &args.file {
            let name = file_arg_path(file);
            m.use_file = true;
            // a full path ignores save_folder when joined on
            if Path::new(&m.save_folder).join(&name).is_file() {
                m.carry_on(&name).map_err(|e| format!("can't read {file} to carry on with it: {e}"))?;
                return Ok(Some(format!("carrying on with {file}")));
            }
            m.file_name = name;
            return Ok(None);
        }
        if !args.carry_on {
            return Ok(None);
        }
        m.use_file = true;
        let Some(name) = latest_dated_file(Path::new(&m.save_folder)) else {
            return Ok(Some(format!("no earlier file in {} to carry on with, starting a new one", m.save_folder)));
        };
        m.carry_on(&name).map_err(|e| format!("can't read {name} to carry on with it: {e}"))?;
        Ok(Some(format!("carrying on with {name}")))
    }
}

fn save(config_path: &Path, settings: &[(&str, Setting)]) {
    if let Err(e) = config::save_settings(config_path, settings) {
        crate::log_warning(&format!("can't save settings to {}: {e}", config_path.display()));
    }
}

fn save_margins(m: &Typewriter, config_path: &Path) {
    save(config_path, &[("left_margin", m.left_margin.into()), ("right_margin", m.right_margin.into())]);
}

/// alt and a letter. settings that are changed are saved to the config.
/// false means quit
pub fn alt_command(c: char, m: &mut Typewriter, config_path: &Path) -> bool {
    match c {
        'a' => m.toggle_autoreturn(),
        'c' => m.centre_text(),
        'b' => {
            m.toggle_bell();
            save(config_path, &[("bell", m.bell_on.into())]);
        }
        'p' => m.toggle_printer(),
        't' => {
            m.toggle_tab_stop();
            save(config_path, &[("tab_stops", m.tab_stops.as_slice().into())]);
        }
        'm' => {
            m.toggle_timer();
            save(config_path, &[("show_timer", m.show_timer.into())]);
        }
        'w' => {
            m.toggle_wpm();
            save(config_path, &[("show_wpm", m.show_wpm.into())]);
        }
        'x' => {
            m.clear_tab_stops();
            save(config_path, &[("tab_stops", m.tab_stops.as_slice().into())]);
        }
        'j' => m.right_justify(),
        'h' => m.toggle_help(),
        'l' => {
            m.margin_set_left();
            save_margins(m, config_path);
        }
        'n' => m.toggle_margin_release(),
        'q' => return false,
        'r' => {
            m.margin_set_right();
            save_margins(m, config_path);
        }
        's' => {
            m.toggle_line_space();
            save(config_path, &[("spacing_index", m.spacing_index.into())]);
        }
        _ => {}
    }
    true
}

pub fn py_bool(b: bool) -> &'static str {
    if b { "True" } else { "False" }
}

/// a path cut down to fit in width columns for the info panel: the home
/// folder becomes ~, then if it's still too long the start is replaced
/// with ... (at a folder boundary where possible) so the file name shows
pub fn short_path(text: &str, width: usize, home: Option<&str>) -> String {
    let mut text = text.to_string();
    if let Some(home) = home.filter(|h| !h.is_empty()) {
        if text == home || text.starts_with(&format!("{home}/")) {
            text = format!("~{}", &text[home.len()..]);
        }
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text;
    }
    if width <= 3 {
        return chars[chars.len() - width..].iter().collect();
    }
    let mut tail: String = chars[chars.len() - (width - 3)..].iter().collect();
    if let Some(slash) = tail.find('/').filter(|&i| i > 0) {
        tail = tail[slash..].to_string();
    }
    format!("...{tail}")
}

/// short_path with the real home folder
pub fn short_home_path(text: &str, width: usize) -> String {
    short_path(text, width, std::env::var("HOME").ok().as_deref())
}

/// how a piece of the panel is shown
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Dim,
    /// a setting or mode that's worth noticing
    Highlight,
}

/// one piece of text on the settings and help panel
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub look: Look,
    /// the alt key that does the same, for front ends that can click it
    pub key: Option<char>,
    /// clicking it opens the settings, for front ends that have them
    pub opens_settings: bool,
}

fn item(row: usize, col: usize, text: impl Into<String>, look: Look, key: Option<char>) -> Item {
    Item { row, col, text: text.into(), look, key, opens_settings: false }
}

/// a setting with no alt key of its own, changed in the settings
fn setting(row: usize, col: usize, text: impl Into<String>) -> Item {
    Item { opens_settings: true, ..item(row, col, text, Look::Dim, None) }
}

/// where things go on the screen. the previous lines are at the top,
/// then the line being typed, the tab bar, the settings and the help, so
/// everything moves down with each previous line shown
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rows {
    pub typing: usize,
    pub tab_bar: usize,
    pub panel: usize,
    pub help: usize,
    /// rows needed for all of it
    pub total: usize,
}

pub fn rows(m: &Typewriter) -> Rows {
    let typing = m.previous_lines;
    let panel = typing + 3;
    let help = panel + 5;
    Rows { typing, tab_bar: typing + 1, panel, help, total: help + 5 }
}

/// the settings, and the help if it's wanted, laid out on a grid cols
/// characters wide
pub fn panel(m: &Typewriter, cols: usize) -> Vec<Item> {
    use Look::{Dim, Highlight};
    let r = rows(m).panel;
    let mut items = vec![
        setting(r, 0, format!("Width={}", m.width)),
        item(r, 10, format!("LS={}", m.current_spacing), Dim, Some('s')),
        item(r, 17, format!("A-RTN={}", py_bool(m.autoreturn)), Dim, Some('a')),
        setting(r, 30, format!("LM={}", m.left_margin)),
        setting(r, 36, format!("RM={}", m.right_margin)),
    ];
    if m.centre_mode() {
        items.push(item(r, 44, "CENTRE", Highlight, None));
    } else if m.right_mode() {
        items.push(item(r, 44, "RIGHT", Highlight, None));
    }
    let file_name = short_home_path(&m.file_name, cols.saturating_sub("File=".len()));
    items.extend([
        setting(r + 1, 0, format!("File={file_name}")),
        item(r + 2, 0, format!("Printer Found={}", py_bool(m.printer_found)), Dim, None),
        item(r + 2, 20, format!("LC={}", m.line_count), Dim, None),
        item(r + 2, 27, format!("WC={}", m.word_count), Dim, None),
        item(r + 2, 37, format!("File={}", py_bool(m.use_file)), Highlight, None),
        item(r + 3, 0, format!("Printer={}", m.printer_name), Dim, None),
        item(r + 3, 20, format!("Printing={}", py_bool(m.use_printer)), Highlight, Some('p')),
        item(r + 3, 37, format!("Bell={}", py_bool(m.bell_on)), Highlight, Some('b')),
        item(r + 4, 0, "alt-h for help", Dim, Some('h')),
    ]);
    items.extend(session_items(m, Instant::now()));

    if m.help_wanted {
        let folder = short_home_path(&m.save_folder, cols.saturating_sub("file path:".len()));
        let lines = [
            "arrows/home/end/tab move, del deletes, alt+backspace deletes word. alt+ below:".to_string(),
            "l = left margin  r = right margin  s = spacing  t = tab stop  x = clear tabs".into(),
            "a = autoreturn  n = margin release  p = printer on/off  b = bell  q = quit".into(),
            "c = centre  j = right justify (enter to finish)  m = show time  w = show wpm".into(),
            format!("file path:{folder}"),
        ];
        for (i, line) in lines.into_iter().enumerate() {
            items.push(item(rows(m).help + i, 0, line, Dim, None));
        }
    }
    items
}

/// the session's time, words per minute and words written, on the
/// bottom row of the panel. the words are the target, if there is one.
/// the time and words per minute can be switched off (alt-m, alt-w or in
/// the config); the others stay where they are
fn session_items(m: &Typewriter, now: Instant) -> Vec<Item> {
    use Look::{Dim, Highlight};
    let r = rows(m).panel + 4;
    let words = m.session_words();
    let wpm = m.session.wpm(words).map_or("-".to_string(), |w| w.to_string());
    let (goal, look) = if m.word_target > 0 {
        let look = if m.target_reached { Highlight } else { Dim };
        (format!("Goal={words}/{}", m.word_target), look)
    } else {
        (format!("Words={words}"), Dim)
    };
    let mut items = Vec::new();
    if m.show_timer {
        items.push(item(r, 20, format!("Time={}", m.session.elapsed(now)), Dim, Some('m')));
    }
    if m.show_wpm {
        items.push(item(r, 31, format!("WPM={wpm}"), Dim, Some('w')));
    }
    items.push(item(r, 40, goal, look, None));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printer_option_on_command_line() {
        let p = |a: &[&str]| Args::try_parse_from([&["typewriter"], a].concat()).unwrap().printer;
        assert_eq!(p(&["--printer", "zebra"]), Some(PrinterChoice::Zebra));
        assert_eq!(p(&["-p", "epson"]), Some(PrinterChoice::Epson));
        assert_eq!(p(&[]), None); // fall back to config
    }

    #[test]
    fn bad_printer_option_rejected() {
        assert!(Args::try_parse_from(["typewriter", "--printer", "laser"]).is_err());
    }

    #[test]
    fn use_file_argument_still_works() {
        let a = Args::try_parse_from(["typewriter", "False", "-p", "zebra"]).unwrap();
        assert_eq!(a.use_file, Some(YesNo::False));
        assert_eq!(Args::try_parse_from(["typewriter"]).unwrap().use_file, None);
    }

    #[test]
    fn file_option_on_command_line() {
        let f = |a: &[&str]| Args::parse_checked([&["typewriter"], a].concat()).unwrap().file;
        assert_eq!(f(&["--file", "notes.txt"]).as_deref(), Some("notes.txt"));
        assert_eq!(f(&["-f", "/tmp/x.txt", "True"]).as_deref(), Some("/tmp/x.txt"));
        assert_eq!(f(&[]), None); // dated name as before
    }

    #[test]
    fn file_option_clashes_with_false() {
        assert!(Args::parse_checked(["typewriter", "False", "-f", "notes.txt"]).is_err());
    }

    #[test]
    fn file_path_with_folder_is_from_current_folder() {
        let cwd = std::env::current_dir().unwrap();
        // std::path::absolute keeps the "." in, which the os ignores
        assert_eq!(Path::new(&file_arg_path("./test.txt")), cwd.join("./test.txt"));
        assert_eq!(Path::new(&file_arg_path("sub/test.txt")), cwd.join("sub/test.txt"));
        assert_eq!(file_arg_path("/abs/test.txt"), "/abs/test.txt");
        assert_eq!(file_arg_path("test.txt"), "test.txt"); // left for save_folder
    }

    #[test]
    fn setup_reads_config_and_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, config::default_config_text()).unwrap();
        let args = Args::parse_checked([
            "typewriter",
            "--config",
            path.to_str().unwrap(),
            "-p",
            "zebra",
            "-f",
            "notes.txt",
        ])
        .unwrap();
        let setup = Setup::load(&args).unwrap();
        assert!(!setup.created);
        let opts = setup.options(&args).unwrap();
        assert_eq!(opts.printer_name, "zebra");
        let mut m = Typewriter::new(opts, None, None, None);
        setup.set_file(&mut m, &args).unwrap();
        assert_eq!(m.file_name, "notes.txt");
        assert!(m.use_file);
        let (bell, folder) = setup.bell_sound();
        assert!(bell.ends_with(".wav") && folder == dir.path());
    }

    #[test]
    fn alt_commands_save_settings_and_q_quits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "spacing_index = 0\nbell = true\n").unwrap();
        let opts = Options { use_file: false, use_printer: false, ..Options::default() };
        let mut m = Typewriter::new(opts, None, None, None);
        assert!(alt_command('s', &mut m, &path));
        assert!(alt_command('b', &mut m, &path));
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.int("spacing_index"), Some(m.spacing_index as i64));
        assert_eq!(cfg.bool("bell"), Some(false));
        assert!(!alt_command('q', &mut m, &path));
    }

    #[test]
    fn panel_shows_settings_and_help_when_wanted() {
        let mut m = Typewriter::new(Options::default(), None, None, None);
        let texts = |m: &Typewriter| panel(m, 80).into_iter().map(|i| i.text).collect::<Vec<_>>();
        assert!(texts(&m).contains(&"Width=75".to_string()));
        assert!(!texts(&m).iter().any(|t| t.starts_with("file path:")));
        m.toggle_help();
        assert!(texts(&m).iter().any(|t| t.starts_with("file path:")));
        let bell = panel(&m, 80).into_iter().find(|i| i.text.starts_with("Bell=")).unwrap();
        assert_eq!((bell.row, bell.col, bell.look, bell.key), (rows(&m).panel + 3, 37, Look::Highlight, Some('b')));
    }

    #[test]
    fn short_path_leaves_short_paths_alone() {
        assert_eq!(short_path("2026-11-17-202444.txt", 70, None), "2026-11-17-202444.txt");
        assert_eq!(short_path("/tmp/a.txt", 70, Some("/home/me")), "/tmp/a.txt");
    }

    #[test]
    fn short_path_uses_tilde_for_home() {
        let home = Some("/home/someone");
        assert_eq!(short_path("/home/someone/writing/a.txt", 70, home), "~/writing/a.txt");
        assert_eq!(short_path("/home/someoneelse/a.txt", 70, home), "/home/someoneelse/a.txt");
    }

    #[test]
    fn short_path_cuts_at_folder_and_keeps_file_name() {
        let folders: Vec<String> = (0..20).map(|i| format!("folder{i}")).collect();
        let deep = format!("/data/{}/chapter-12.txt", folders.join("/"));
        let short = short_path(&deep, 40, None);
        assert!(short.len() <= 40);
        assert!(short.starts_with(".../") && short.ends_with("/folder19/chapter-12.txt"), "{short}");
    }

    #[test]
    fn short_path_huge_file_name_still_fits() {
        let short = short_path(&format!("/x/{}.txt", "a".repeat(100)), 30, None);
        assert!(short.len() == 30 && short.starts_with("...") && short.ends_with("a.txt"));
        assert_eq!(short_path("abcdef", 2, None), "ef");
        assert_eq!(short_path("abcdef", 0, None), "");
        assert_eq!(short_path("/é/ü/ñandú.txt", 10, None).chars().count(), 10); // not bytes
    }

    #[test]
    fn continue_clashes_with_file_and_false() {
        assert!(Args::parse_checked(["typewriter", "--continue"]).unwrap().carry_on);
        assert!(Args::parse_checked(["typewriter", "--continue", "-f", "x.txt"]).is_err());
        assert!(Args::parse_checked(["typewriter", "--continue", "False"]).is_err());
    }

    #[test]
    fn dated_names() {
        assert!(is_dated_name("2026-11-17-202444.txt"));
        assert!(!is_dated_name("2026-11-17-202444.md"));
        assert!(!is_dated_name("notes.txt"));
        assert!(!is_dated_name("2026-11-17x202444.txt"));
    }

    fn setup_in(dir: &Path, extra: &[&str]) -> (Setup, Args) {
        let path = dir.join("config.toml");
        let folder = dir.join("writing");
        std::fs::create_dir_all(&folder).unwrap();
        let text = format!("width = 75\nautoreturn = false\nspacing_index = 0\nmargin_bell = 8\nleft_margin = 0\nsave_folder = \"{}/\"\ntab_stops = [10, 5]\nword_target = 50\n", folder.display());
        std::fs::write(&path, text).unwrap();
        let args = Args::parse_checked([&["typewriter", "--config", path.to_str().unwrap()], extra].concat()).unwrap();
        (Setup::load(&args).unwrap(), args)
    }

    #[test]
    fn continue_carries_on_with_the_newest_dated_file() {
        let dir = tempfile::tempdir().unwrap();
        let (setup, args) = setup_in(dir.path(), &["--continue"]);
        let folder = dir.path().join("writing");
        std::fs::write(folder.join("2026-11-01-090000.txt"), "old\n").unwrap();
        std::fs::write(folder.join("2026-11-19-210000.txt"), "newest words\n").unwrap();
        std::fs::write(folder.join("zzz.txt"), "not mine\n").unwrap();
        let opts = setup.options(&args).unwrap();
        assert_eq!((opts.tab_stops.clone(), opts.word_target), (vec![10, 5], 50));
        let mut m = Typewriter::new(Options { use_file: false, ..opts }, None, None, None);
        let note = setup.set_file(&mut m, &args).unwrap().unwrap();
        assert!(note.contains("2026-11-19-210000.txt"), "{note}");
        assert_eq!((m.file_name.as_str(), m.previous_line(1), m.word_count), ("2026-11-19-210000.txt", "newest words", 2));
        assert!(m.use_file);
        assert_eq!(m.tab_stops, [5, 10]);
    }

    #[test]
    fn file_that_is_already_there_is_carried_on_with() {
        let dir = tempfile::tempdir().unwrap();
        let (setup, args) = setup_in(dir.path(), &["-f", "letter.txt"]);
        std::fs::write(dir.path().join("writing/letter.txt"), "dear sir\nyours\n").unwrap();
        let mut m = Typewriter::new(setup.options(&args).unwrap(), None, None, None);
        let note = setup.set_file(&mut m, &args).unwrap();
        assert_eq!(note.as_deref(), Some("carrying on with letter.txt"));
        assert_eq!((m.file_name.as_str(), m.previous_line(2), m.previous_line(1)), ("letter.txt", "dear sir", "yours"));
        assert_eq!((m.line_count, m.word_count, m.session_words()), (2, 3, 0));
    }

    #[test]
    fn file_given_as_a_path_is_carried_on_with_too() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere.txt");
        std::fs::write(&elsewhere, "one line\n").unwrap();
        let (setup, args) = setup_in(dir.path(), &["-f", elsewhere.to_str().unwrap()]);
        let mut m = Typewriter::new(setup.options(&args).unwrap(), None, None, None);
        assert!(setup.set_file(&mut m, &args).unwrap().is_some());
        assert_eq!((m.file_name.as_str(), m.previous_line(1)), (elsewhere.to_str().unwrap(), "one line"));
    }

    #[test]
    fn new_file_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (setup, args) = setup_in(dir.path(), &["-f", "new.txt"]);
        let mut m = Typewriter::new(setup.options(&args).unwrap(), None, None, None);
        assert_eq!(setup.set_file(&mut m, &args).unwrap(), None);
        assert_eq!((m.file_name.as_str(), m.previous_line(1), m.line_count), ("new.txt", "", 0));
    }

    #[test]
    fn continue_with_nothing_to_carry_on_with_starts_new() {
        let dir = tempfile::tempdir().unwrap();
        let (setup, args) = setup_in(dir.path(), &["--continue"]);
        let mut m = Typewriter::new(setup.options(&args).unwrap(), None, None, None);
        let dated = m.file_name.clone();
        let note = setup.set_file(&mut m, &args).unwrap().unwrap();
        assert!(note.contains("starting a new one"), "{note}");
        assert_eq!(m.file_name, dated);
    }

    #[test]
    fn alt_t_and_x_save_tab_stops() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "tab_stops = []\n").unwrap();
        let mut m = Typewriter::new(Options { use_file: false, use_printer: false, ..Options::default() }, None, None, None);
        m.cursor = 12;
        alt_command('t', &mut m, &path);
        assert_eq!(Config::load(&path).unwrap().usize_list("tab_stops").unwrap(), vec![12]);
        alt_command('x', &mut m, &path);
        assert!(Config::load(&path).unwrap().usize_list("tab_stops").unwrap().is_empty());
    }

    #[test]
    fn alt_m_and_w_hide_the_time_and_wpm_and_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "show_timer = true\nshow_wpm = true\n").unwrap();
        let mut m = Typewriter::new(Options { use_file: false, use_printer: false, ..Options::default() }, None, None, None);
        let shown = |m: &Typewriter| session_items(m, Instant::now()).into_iter().map(|i| i.text).collect::<Vec<_>>();
        alt_command('m', &mut m, &path);
        assert!(!shown(&m).iter().any(|t| t.starts_with("Time=")));
        alt_command('w', &mut m, &path);
        assert!(!shown(&m).iter().any(|t| t.starts_with("WPM=")));
        let cfg = Config::load(&path).unwrap();
        assert_eq!((cfg.bool("show_timer"), cfg.bool("show_wpm")), (Some(false), Some(false)));
        alt_command('m', &mut m, &path);
        assert!(shown(&m).iter().any(|t| t.starts_with("Time=")));
        assert_eq!(Config::load(&path).unwrap().bool("show_timer"), Some(true));
    }

    #[test]
    fn panel_shows_the_session() {
        let mut m = Typewriter::new(Options::default(), None, None, None);
        let now = m.session.started + std::time::Duration::from_secs(125 * 60);
        let texts = |m: &Typewriter| session_items(m, now).into_iter().map(|i| i.text).collect::<Vec<_>>();
        assert_eq!(texts(&m), ["Time=2h05m", "WPM=-", "Words=0"]);
        m.word_target = 10;
        m.word_count = 4;
        m.session.typing = std::time::Duration::from_secs(60);
        assert_eq!(texts(&m), ["Time=2h05m", "WPM=4", "Goal=4/10"]);
        m.show_timer = false;
        assert_eq!(texts(&m), ["WPM=4", "Goal=4/10"]);
        m.show_wpm = false;
        assert_eq!(texts(&m), ["Goal=4/10"]);
    }

    #[test]
    fn timer_and_wpm_can_be_hidden_in_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let (setup, args) = setup_in(dir.path(), &[]);
        let opts = setup.options(&args).unwrap();
        assert!(opts.show_timer && opts.show_wpm); // on unless the config says
        let path = dir.path().join("config.toml");
        let text = std::fs::read_to_string(&path).unwrap() + "show_timer = false\nshow_wpm = false\n";
        std::fs::write(&path, text).unwrap();
        let opts = Setup::load(&args).unwrap().options(&args).unwrap();
        assert!(!opts.show_timer && !opts.show_wpm);
    }

    #[test]
    fn previous_lines_from_config_or_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let (setup, args) = setup_in(dir.path(), &[]);
        assert_eq!(setup.options(&args).unwrap().previous_lines, 3); // not in the config
        let path = dir.path().join("config.toml");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "previous_lines = 6\n").unwrap();
        let setup = Setup::load(&args).unwrap();
        assert_eq!(setup.options(&args).unwrap().previous_lines, 6);
        let a = Args::parse_checked(["typewriter", "--config", path.to_str().unwrap(), "-l", "0"]).unwrap();
        assert_eq!(Setup::load(&a).unwrap().options(&a).unwrap().previous_lines, 0);
        // --lines is kept for next time, keeping the rest of the file
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("previous_lines = 0\n") && text.contains("word_target = 50"), "{text}");
        assert_eq!(Setup::load(&args).unwrap().options(&args).unwrap().previous_lines, 0);
        std::fs::write(&path, text.replace("previous_lines = 0", "previous_lines = 6")).unwrap();
        assert!(Args::parse_checked(["typewriter", "--lines", "8"]).is_err());
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap().replace("previous_lines = 6", "previous_lines = 9")).unwrap();
        assert!(Setup::load(&args).unwrap().options(&args).is_err());
    }

    #[test]
    fn layout_moves_with_the_previous_lines() {
        let mut m = Typewriter::new(Options::default(), None, None, None);
        // 3 is the old fixed layout
        assert_eq!(rows(&m), Rows { typing: 3, tab_bar: 4, panel: 6, help: 11, total: 16 });
        m.previous_lines = 0;
        assert_eq!(rows(&m), Rows { typing: 0, tab_bar: 1, panel: 3, help: 8, total: 13 });
        m.previous_lines = 7;
        m.toggle_help();
        let last = panel(&m, 80).into_iter().map(|i| i.row).max().unwrap();
        assert_eq!((rows(&m).total, last), (20, 19));
    }

    #[test]
    fn help_lines_fit_the_screen() {
        let mut m = Typewriter::new(Options::default(), None, None, None);
        m.toggle_help();
        for i in panel(&m, 80) {
            assert!(i.col + i.text.chars().count() <= 80, "{}", i.text);
        }
    }
}
