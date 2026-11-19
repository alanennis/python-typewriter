//! typewriter for a line printer: the terminal screen and keyboard. the
//! command line, config and panel layout are shared with the window
//! version, in front.rs

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{self, BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate};
use crossterm::{cursor, execute, queue};
use std::io::{self, Write};
use std::path::Path;
use typewriter::front::{self, Args, Look, Setup};
use typewriter::sound::BellSound;
use typewriter::typewriter::{Sound, Typewriter};

/// terminal colour number for highlighted settings (0-255). 109 is a soft
/// grey-teal; try 144 (khaki), 146 (lavender grey) or 248 (light grey)
const HIGHLIGHT: Color = Color::AnsiValue(109);
const DIM: Color = Color::AnsiValue(241);
const TYPING: Color = Color::DarkGreen;

fn beep() {
    let mut out = io::stdout();
    let _ = out.write_all(b"\x07");
    let _ = out.flush();
}

fn put(out: &mut impl Write, row: usize, col: usize, text: &str, color: Color) -> io::Result<()> {
    queue!(out, cursor::MoveTo(col as u16, row as u16), SetForegroundColor(color), Print(text), ResetColor)
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
    // columns that can be written without wrapping onto the next line
    let cols = terminal::size().map_or(80, |(c, _)| c as usize).saturating_sub(1);
    queue!(out, BeginSynchronizedUpdate, Clear(ClearType::All))?;
    for item in front::panel(m, cols) {
        let color = match item.look {
            Look::Dim => DIM,
            Look::Highlight => HIGHLIGHT,
        };
        put(out, item.row, item.col, &item.text, color)?;
    }
    show_buffers(out, m)?;
    queue!(out, EndSynchronizedUpdate)?;
    out.flush()
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
        KeyCode::Char(c) if alt => {
            if !front::alt_command(c, m, config_path) {
                return Ok(Flow::Quit);
            }
        }
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
                // a line typed without pressing enter is still kept
                return m.finish_line();
            }
        }
    }
}

/// load the config, build the machine and start the screen
fn run() -> Result<(), String> {
    let args = Args::parse_checked(std::env::args_os()).unwrap_or_else(|e| e.exit());
    let (cols, rows) = terminal::size().map_err(|e| format!("can't read terminal size: {e}"))?;
    if rows < 16 || cols < 81 {
        println!("terminal is too small please make it at least 16 lines by 81 columns");
        return Ok(());
    }

    let setup = Setup::load(&args)?;
    if setup.created {
        // stays on screen after the typewriter exits
        println!("created {} with the starting settings", setup.config_path.display());
    }
    let opts = setup.options(&args)?;
    let printer = setup.printer()?;

    let (bell_file, bell_dir) = setup.bell_sound();
    let ding: Option<Sound> = BellSound::new(bell_file, bell_dir).map(|mut s| -> Sound {
        Box::new(move || {
            if !s.play() {
                beep();
            }
        })
    });

    let mut machine = Typewriter::new(opts, printer, Some(Box::new(beep)), ding);
    setup.set_file(&mut machine, &args)?;
    main_loop(&mut machine, &setup.config_path)
}

fn main() {
    if let Err(msg) = run() {
        eprintln!("{msg}");
        std::process::exit(1);
    }
}
