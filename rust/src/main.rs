//! typewriter for a line printer: the terminal screen and keyboard

use clap::{Parser, ValueEnum};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{self, BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate};
use crossterm::{cursor, execute, queue};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use typewriter::config::{self, Config, Setting};
use typewriter::printer::{open_printer, printer_name};
use typewriter::sound::BellSound;
use typewriter::typewriter::{Options, Sound, Typewriter};

/// terminal colour number for highlighted settings (0-255). 109 is a soft
/// grey-teal; try 144 (khaki), 146 (lavender grey) or 248 (light grey)
const HIGHLIGHT: Color = Color::AnsiValue(109);
const DIM: Color = Color::AnsiValue(241);
const TYPING: Color = Color::DarkGreen;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum PrinterChoice {
    Epson,
    Zebra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum YesNo {
    #[value(name = "True")]
    True,
    #[value(name = "False")]
    False,
}

/// typewriter for a line printer. anything given here overrides config.toml
#[derive(Parser, Debug)]
struct Args {
    /// save typed lines to a file (overrides use_file)
    use_file: Option<YesNo>,
    /// printer to use (overrides printer in config.toml)
    #[arg(short, long, value_enum)]
    printer: Option<PrinterChoice>,
    /// config file to use (default: config.toml next to this binary,
    /// created on the first run)
    #[arg(short, long)]
    config: Option<PathBuf>,
}

fn beep() {
    let mut out = io::stdout();
    let _ = out.write_all(b"\x07");
    let _ = out.flush();
}

fn py_bool(b: bool) -> &'static str {
    if b { "True" } else { "False" }
}

fn put(out: &mut impl Write, row: u16, col: u16, text: &str, color: Color) -> io::Result<()> {
    queue!(out, cursor::MoveTo(col, row), SetForegroundColor(color), Print(text), ResetColor)
}

fn show_handy_settings(out: &mut impl Write, m: &Typewriter) -> io::Result<()> {
    let r = 6;
    put(out, r, 0, &format!("Width={}", m.width), DIM)?;
    put(out, r, 10, &format!("LS={}", m.current_spacing), DIM)?;
    put(out, r, 17, &format!("A-RTN={}", py_bool(m.autoreturn)), DIM)?;
    put(out, r, 30, &format!("LM={}", m.left_margin), DIM)?;
    put(out, r, 36, &format!("RM={}", m.right_margin), DIM)?;
    if m.centre_mode() {
        put(out, r, 44, "CENTRE", HIGHLIGHT)?;
    } else if m.right_mode() {
        put(out, r, 44, "RIGHT", HIGHLIGHT)?;
    }
    put(out, r + 1, 0, &format!("File={}", m.file_name), DIM)?;
    put(out, r + 2, 0, &format!("Printer Found={}", py_bool(m.printer_found)), DIM)?;
    put(out, r + 2, 20, &format!("LC={}", m.line_count), DIM)?;
    put(out, r + 2, 27, &format!("WC={}", m.word_count), DIM)?;
    put(out, r + 2, 37, &format!("File={}", py_bool(m.use_file)), HIGHLIGHT)?;
    put(out, r + 3, 0, &format!("Printer={}", m.printer_name), DIM)?;
    put(out, r + 3, 20, &format!("Printing={}", py_bool(m.use_printer)), HIGHLIGHT)?;
    put(out, r + 3, 37, &format!("Bell={}", py_bool(m.bell_on)), HIGHLIGHT)?;
    put(out, r + 4, 0, "alt-h for help", DIM)
}

fn show_help(out: &mut impl Write, m: &Typewriter) -> io::Result<()> {
    let lines = [
        "arrows/home/end move, del deletes, alt+backspace deletes a word. alt+ below:".to_string(),
        "l = set left margin  r = set right margin       s = toggle line spacing".into(),
        "a = set autoreturn   n = toggle margin release  q = quit  p = printer on/off".into(),
        "c = centre text      j = right justify (enter to finish)  b = bell on/off".into(),
        format!("file path:{}", m.save_folder),
    ];
    for (i, line) in lines.iter().enumerate() {
        put(out, 11 + i as u16, 0, line, DIM)?;
    }
    Ok(())
}

fn show_buffers(out: &mut impl Write, m: &Typewriter) -> io::Result<()> {
    put(out, 4, 0, &m.give_tab_bar(), DIM)?;
    put(out, 0, 0, &m.prev_buf_3, DIM)?;
    put(out, 1, 0, &m.prev_buf_2, DIM)?;
    put(out, 2, 0, &m.prev_buf_1, DIM)?;
    put(out, 3, 0, &m.give_buffer(), TYPING)?;
    // put the screen cursor where the machine says the next character goes
    queue!(out, cursor::MoveTo(m.cursor as u16, 3))
}

fn draw(out: &mut impl Write, m: &Typewriter) -> io::Result<()> {
    queue!(out, BeginSynchronizedUpdate, Clear(ClearType::All))?;
    show_handy_settings(out, m)?;
    if m.help_wanted {
        show_help(out, m)?;
    }
    show_buffers(out, m)?;
    queue!(out, EndSynchronizedUpdate)?;
    out.flush()
}

fn save(config_path: &Path, settings: &[(&str, Setting)]) {
    if let Err(e) = config::save_settings(config_path, settings) {
        typewriter::log_warning(&format!("can't save settings to {}: {e}", config_path.display()));
    }
}

/// what to do after a key
enum Flow {
    Continue,
    Quit,
}

fn key_check(key: KeyEvent, m: &mut Typewriter, config_path: &Path) -> Result<Flow, String> {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => return Ok(Flow::Quit),
        KeyCode::Char(c) if alt => match c {
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
                save(config_path, &[("left_margin", m.left_margin.into()), ("right_margin", m.right_margin.into())]);
            }
            'n' => m.toggle_margin_release(),
            'q' => return Ok(Flow::Quit),
            'r' => {
                m.margin_set_right();
                save(config_path, &[("left_margin", m.left_margin.into()), ("right_margin", m.right_margin.into())]);
            }
            's' => {
                m.toggle_line_space();
                save(config_path, &[("spacing_index", m.spacing_index.into())]);
            }
            _ => {}
        },
        KeyCode::Backspace if alt => m.delete_word(),
        KeyCode::Char(c) if !ctrl && (' '..='~').contains(&c) => m.char_add(c)?,
        KeyCode::Backspace => m.backspace(),
        KeyCode::Enter => m.carriage_return()?,
        KeyCode::Left => m.cursor_left(),
        KeyCode::Right => m.cursor_right(),
        KeyCode::Home => m.cursor_home(),
        KeyCode::End => m.cursor_end(),
        KeyCode::Delete => m.delete(),
        _ => {}
    }
    Ok(Flow::Continue)
}

/// puts the terminal back however the program ends
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Show)?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), ResetColor, terminal::LeaveAlternateScreen, cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}

fn next_key() -> io::Result<Option<KeyEvent>> {
    match event::read()? {
        Event::Key(k) if k.kind != KeyEventKind::Release => Ok(Some(k)),
        _ => Ok(None),
    }
}

fn main_loop(m: &mut Typewriter, config_path: &Path) -> Result<(), String> {
    let io_err = |e: io::Error| format!("terminal problem: {e}");
    let _guard = TerminalGuard::new().map_err(io_err)?;
    let mut out = io::stdout();

    put(&mut out, 0, 0, "Typewriter starting up, press any key to go", HIGHLIGHT).map_err(io_err)?;
    out.flush().map_err(io_err)?;
    while next_key().map_err(io_err)?.is_none() {}

    loop {
        draw(&mut out, m).map_err(io_err)?;
        // anything that isn't a key press (a resize, say) just redraws
        if let Some(key) = next_key().map_err(io_err)? {
            if let Flow::Quit = key_check(key, m, config_path)? {
                return Ok(());
            }
        }
    }
}

/// load the config, build the machine and start the screen
fn run() -> Result<(), String> {
    let args = Args::parse();
    let (cols, rows) = terminal::size().map_err(|e| format!("can't read terminal size: {e}"))?;
    if rows < 16 || cols < 81 {
        println!("terminal is too small please make it at least 16 lines by 81 columns");
        return Ok(());
    }

    let config_path = match args.config {
        Some(p) => p,
        None => {
            let p = config::default_config_path()?;
            if config::create_if_missing(&p)? {
                // stays on screen after the typewriter exits
                println!("created {} with the starting settings", p.display());
            }
            p
        }
    };
    let mut cfg = Config::load(&config_path)?;
    match args.printer {
        Some(PrinterChoice::Epson) => cfg.set_str("printer", "epson"),
        Some(PrinterChoice::Zebra) => cfg.set_str("printer", "zebra"),
        None => {}
    }

    let width = cfg.require_usize("width")?;
    let opts = Options {
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
        printer_name: printer_name(&cfg),
        bell_on: cfg.bool("bell").unwrap_or(true),
    };
    // always open the printer so alt-p can turn printing on even if
    // use_printer starts off false in the config
    let printer = open_printer(&cfg)?;

    let base_dir = config_path.parent().unwrap_or(Path::new("."));
    let ding: Option<Sound> = BellSound::new(cfg.str("bell_sound").unwrap_or("ding.wav"), base_dir)
        .map(|mut s| -> Sound {
            Box::new(move || {
                if !s.play() {
                    beep();
                }
            })
        });

    let mut machine = Typewriter::new(opts, printer, Some(Box::new(beep)), ding);
    machine.save_folder = cfg.require_str("save_folder")?.to_string();

    main_loop(&mut machine, &config_path)
}

fn main() {
    if let Err(msg) = run() {
        eprintln!("{msg}");
        std::process::exit(1);
    }
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
}
