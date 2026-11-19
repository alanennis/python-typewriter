//! what the terminal and window versions have in common: the command
//! line, building the machine from the config, the alt-key commands and
//! the layout of the settings and help panel. each front end only draws
//! and reads keys in its own way.

use crate::config::{self, Config, Setting};
use crate::printer::{open_printer, printer_name};
use crate::typewriter::{LinePrinter, Options, Typewriter};
use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};

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
    /// current folder. lines are added to the end if it already exists.
    /// turns on saving to a file
    #[arg(short, long, value_name = "NAME")]
    pub file: Option<String>,
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

/// the config, read and ready to build a machine from
pub struct Setup {
    pub config_path: PathBuf,
    pub config: Config,
    /// the config file was just made from the starting settings
    pub created: bool,
}

impl Setup {
    /// find (or create) and read the config, with --printer applied
    pub fn load(args: &Args) -> Result<Self, String> {
        let (config_path, created) = match &args.config {
            Some(p) => (p.clone(), false),
            None => {
                let p = config::default_config_path()?;
                let created = config::create_if_missing(&p)?;
                (p, created)
            }
        };
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

    /// where the new machine saves: save_folder, and --file if given
    pub fn set_file(&self, m: &mut Typewriter, args: &Args) -> Result<(), String> {
        m.save_folder = self.config.require_str("save_folder")?.to_string();
        if let Some(file) = &args.file {
            m.file_name = file_arg_path(file);
            m.use_file = true;
        }
        Ok(())
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
}

fn item(row: usize, col: usize, text: impl Into<String>, look: Look, key: Option<char>) -> Item {
    Item { row, col, text: text.into(), look, key }
}

/// rows 0-3 are the typing and row 4 the tab bar; the panel starts here
pub const PANEL_ROW: usize = 6;
pub const HELP_ROW: usize = 11;

/// the settings, and the help if it's wanted, laid out on a grid cols
/// characters wide
pub fn panel(m: &Typewriter, cols: usize) -> Vec<Item> {
    use Look::{Dim, Highlight};
    let r = PANEL_ROW;
    let mut items = vec![
        item(r, 0, format!("Width={}", m.width), Dim, None),
        item(r, 10, format!("LS={}", m.current_spacing), Dim, Some('s')),
        item(r, 17, format!("A-RTN={}", py_bool(m.autoreturn)), Dim, Some('a')),
        item(r, 30, format!("LM={}", m.left_margin), Dim, None),
        item(r, 36, format!("RM={}", m.right_margin), Dim, None),
    ];
    if m.centre_mode() {
        items.push(item(r, 44, "CENTRE", Highlight, None));
    } else if m.right_mode() {
        items.push(item(r, 44, "RIGHT", Highlight, None));
    }
    let file_name = short_home_path(&m.file_name, cols.saturating_sub("File=".len()));
    items.extend([
        item(r + 1, 0, format!("File={file_name}"), Dim, None),
        item(r + 2, 0, format!("Printer Found={}", py_bool(m.printer_found)), Dim, None),
        item(r + 2, 20, format!("LC={}", m.line_count), Dim, None),
        item(r + 2, 27, format!("WC={}", m.word_count), Dim, None),
        item(r + 2, 37, format!("File={}", py_bool(m.use_file)), Highlight, None),
        item(r + 3, 0, format!("Printer={}", m.printer_name), Dim, None),
        item(r + 3, 20, format!("Printing={}", py_bool(m.use_printer)), Highlight, Some('p')),
        item(r + 3, 37, format!("Bell={}", py_bool(m.bell_on)), Highlight, Some('b')),
        item(r + 4, 0, "alt-h for help", Dim, Some('h')),
    ]);

    if m.help_wanted {
        let folder = short_home_path(&m.save_folder, cols.saturating_sub("file path:".len()));
        let lines = [
            "arrows/home/end move, del deletes, alt+backspace deletes a word. alt+ below:".to_string(),
            "l = set left margin  r = set right margin       s = toggle line spacing".into(),
            "a = set autoreturn   n = toggle margin release  q = quit  p = printer on/off".into(),
            "c = centre text      j = right justify (enter to finish)  b = bell on/off".into(),
            format!("file path:{folder}"),
        ];
        for (i, line) in lines.into_iter().enumerate() {
            items.push(item(HELP_ROW + i, 0, line, Dim, None));
        }
    }
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
        assert_eq!((bell.row, bell.col, bell.look, bell.key), (PANEL_ROW + 3, 37, Look::Highlight, Some('b')));
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
}
