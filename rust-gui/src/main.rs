//! typewriter for a line printer, in a window. the machine, printers,
//! config and bell all come from the terminal version's library in ../rust;
//! this file is only the window and the keys.

use clap::{Parser, ValueEnum};
use eframe::egui::{self, Align2, Color32, Event, FontId, Key, Pos2, Rect, Sense, Vec2};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use typewriter::config::{self, Config, Setting};
use typewriter::printer::{open_printer, printer_name};
use typewriter::sound::BellSound;
use typewriter::typewriter::{Options, Sound, Typewriter};

/// rows of text the window lays out: 4 lines of typing, the tab bar, the
/// settings, the help and a message line at the bottom
const ROWS: usize = 17;
/// fewest columns laid out; the help lines are 80 wide
const MIN_COLS: usize = 81;
const MESSAGE_ROW: usize = 16;
/// how long the line flashes when the machine beeps
const FLASH: Duration = Duration::from_millis(150);
const BLINK: Duration = Duration::from_millis(530);

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

/// typewriter for a line printer, in a window. anything given here
/// overrides config.toml
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
    /// file to write to instead of a dated one. a plain name goes in
    /// save_folder; a path (./test.txt, ~/notes.txt) is taken from the
    /// current folder. lines are added to the end if it already exists.
    /// turns on saving to a file
    #[arg(short, long, value_name = "NAME")]
    file: Option<String>,
}

impl Args {
    /// parse and check the options go together
    fn parse_checked<I, T>(args: I) -> Result<Self, clap::Error>
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
/// like ./test.txt, is taken from the current folder
fn file_arg_path(name: &str) -> String {
    if name.contains('/') {
        if let Ok(p) = std::path::absolute(name) {
            return p.display().to_string();
        }
    }
    name.to_string()
}

fn py_bool(b: bool) -> &'static str {
    if b { "True" } else { "False" }
}

/// a path cut down to fit in width columns: the home folder becomes ~,
/// then if it's still too long the start is replaced with ... (at a folder
/// boundary where possible) so the file name shows
fn short_path(text: &str, width: usize, home: Option<&str>) -> String {
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

fn short_home_path(text: &str, width: usize) -> String {
    short_path(text, width, std::env::var("HOME").ok().as_deref())
}

/// something the keyboard asked for, the same keys as the terminal version
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Char(char),
    /// alt and a letter
    Alt(char),
    DeleteWord,
    Backspace,
    Enter,
    Left,
    Right,
    Home,
    End,
    Delete,
    Quit,
}

fn letter(key: Key) -> Option<char> {
    use Key::*;
    Some(match key {
        A => 'a', B => 'b', C => 'c', D => 'd', E => 'e', F => 'f', G => 'g',
        H => 'h', I => 'i', J => 'j', K => 'k', L => 'l', M => 'm', N => 'n',
        O => 'o', P => 'p', Q => 'q', R => 'r', S => 's', T => 't', U => 'u',
        V => 'v', W => 'w', X => 'x', Y => 'y', Z => 'z',
        _ => return None,
    })
}

/// what one window event means to the typewriter. alt_down says alt is
/// held, so the letter that comes as text alongside alt-l isn't typed
fn actions_for(event: &Event, alt_down: bool) -> Vec<Action> {
    match event {
        // what's typed comes as text, so the keyboard layout and shift
        // are already taken care of
        Event::Text(t) if !alt_down => {
            t.chars().filter(|c| (' '..='~').contains(c)).map(Action::Char).collect()
        }
        // egui turns ctrl-c into a copy; the terminal version quit on it
        Event::Copy => vec![Action::Quit],
        Event::Key { key, pressed: true, modifiers, .. } => {
            let action = if modifiers.ctrl || modifiers.command {
                (*key == Key::C).then_some(Action::Quit)
            } else if modifiers.alt {
                match key {
                    Key::Backspace => Some(Action::DeleteWord),
                    k => letter(*k).map(Action::Alt),
                }
            } else {
                match key {
                    Key::Backspace => Some(Action::Backspace),
                    Key::Enter => Some(Action::Enter),
                    Key::ArrowLeft => Some(Action::Left),
                    Key::ArrowRight => Some(Action::Right),
                    Key::Home => Some(Action::Home),
                    Key::End => Some(Action::End),
                    Key::Delete => Some(Action::Delete),
                    _ => None,
                }
            };
            action.into_iter().collect()
        }
        _ => Vec::new(),
    }
}

fn save(config_path: &Path, settings: &[(&str, Setting)]) {
    if let Err(e) = config::save_settings(config_path, settings) {
        typewriter::log_warning(&format!("can't save settings to {}: {e}", config_path.display()));
    }
}

fn save_margins(m: &Typewriter, config_path: &Path) {
    save(config_path, &[("left_margin", m.left_margin.into()), ("right_margin", m.right_margin.into())]);
}

/// do what the key asks. Ok(false) means quit
fn apply(action: Action, m: &mut Typewriter, config_path: &Path) -> Result<bool, String> {
    match action {
        Action::Quit => return Ok(false),
        Action::Alt(c) => match c {
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
            'q' => return Ok(false),
            'r' => {
                m.margin_set_right();
                save_margins(m, config_path);
            }
            's' => {
                m.toggle_line_space();
                save(config_path, &[("spacing_index", m.spacing_index.into())]);
            }
            _ => {}
        },
        Action::DeleteWord => m.delete_word(),
        Action::Char(c) => m.char_add(c)?,
        Action::Backspace => m.backspace(),
        Action::Enter => m.carriage_return()?,
        Action::Left => m.cursor_left(),
        Action::Right => m.cursor_right(),
        Action::Home => m.cursor_home(),
        Action::End => m.cursor_end(),
        Action::Delete => m.delete(),
    }
    Ok(true)
}

/// colours for light and dark desktops, matching the terminal version's
/// grey, green and grey-teal
struct Palette {
    paper: Color32,
    dim: Color32,
    typing: Color32,
    highlight: Color32,
    error: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Palette {
                paper: Color32::from_rgb(0x1c, 0x1c, 0x1c),
                dim: Color32::from_rgb(0x62, 0x62, 0x62),
                typing: Color32::from_rgb(0x4e, 0xb3, 0x4e),
                highlight: Color32::from_rgb(0x87, 0xaf, 0xaf),
                error: Color32::from_rgb(0xe0, 0x6c, 0x6c),
            }
        } else {
            Palette {
                paper: Color32::from_rgb(0xfa, 0xf6, 0xec),
                dim: Color32::from_rgb(0x8a, 0x86, 0x7e),
                typing: Color32::from_rgb(0x1f, 0x6e, 0x1f),
                highlight: Color32::from_rgb(0x3a, 0x7a, 0x7a),
                error: Color32::from_rgb(0xb0, 0x30, 0x30),
            }
        }
    }
}

/// a grid of character cells, like the terminal, sized to the window
struct Grid {
    origin: Pos2,
    cell: Vec2,
    font: FontId,
}

impl Grid {
    /// the biggest font that fits cols by ROWS cells in rect
    fn fit(ctx: &egui::Context, rect: Rect, cols: usize) -> Self {
        let probe = FontId::monospace(100.0);
        let (w, h) = ctx.fonts(|f| (f.glyph_width(&probe, 'M'), f.row_height(&probe)));
        let size = (rect.width() / (cols as f32 * w / 100.0))
            .min(rect.height() / (ROWS as f32 * h / 100.0))
            .clamp(8.0, 72.0)
            .floor();
        let font = FontId::monospace(size);
        // measured from a laid out line, not one glyph: egui snaps each
        // letter's advance to the pixel grid, and one glyph's width would
        // drift away from the text a little more with every column
        let cell = ctx.fonts(|f| {
            let line = f.layout_no_wrap("M".repeat(200), font.clone(), Color32::WHITE);
            Vec2::new(line.size().x / 200.0, f.row_height(&font))
        });
        Grid { origin: rect.min, cell, font }
    }

    /// where column col of text starts on the screen, from the text's own
    /// layout so the cursor sits exactly on the letters
    fn text_x(&self, ctx: &egui::Context, text: &str, col: usize) -> f32 {
        let galley = ctx.fonts(|f| f.layout_no_wrap(text.to_string(), self.font.clone(), Color32::WHITE));
        let x = match galley.rows.first().and_then(|r| r.glyphs.get(col)) {
            Some(glyph) => glyph.pos.x,
            // past the end of the text: carry on from its right edge
            None => galley.size().x + (col - text.chars().count().min(col)) as f32 * self.cell.x,
        };
        self.origin.x + x
    }

    fn cells(&self, row: usize, col: usize, len: usize) -> Rect {
        let min = self.origin + Vec2::new(col as f32 * self.cell.x, row as f32 * self.cell.y);
        Rect::from_min_size(min, Vec2::new(len as f32 * self.cell.x, self.cell.y))
    }

    fn put(&self, p: &egui::Painter, row: usize, col: usize, text: &str, color: Color32) {
        p.text(self.cells(row, col, 0).min, Align2::LEFT_TOP, text, self.font.clone(), color);
    }
}

/// beep: flash the line and play a short tone (there's no terminal bell
/// in a window)
fn make_beep(flash: Rc<Cell<Option<Instant>>>) -> Sound {
    let mut tone = beep_wav_path().and_then(|p| BellSound::new(&p.display().to_string(), Path::new("/")));
    Box::new(move || {
        flash.set(Some(Instant::now()));
        if let Some(t) = tone.as_mut() {
            t.play();
        }
    })
}

/// a short 880Hz beep written to the temp folder for the bell player
fn beep_wav_path() -> Option<PathBuf> {
    let path = std::env::temp_dir().join("typewriter-gui-beep.wav");
    let rate = 44100u32;
    let n = rate / 10;
    let mut data = Vec::with_capacity(n as usize * 2);
    for i in 0..n {
        let t = i as f32 / rate as f32;
        // fade in and out so it doesn't click
        let env = (i.min(n - i) as f32 / 400.0).min(1.0);
        let s = (t * 880.0 * std::f32::consts::TAU).sin() * 0.25 * env;
        data.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // pcm
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    std::fs::write(&path, wav).ok()?;
    Some(path)
}

struct TypewriterApp {
    /// taken out and dropped on quit, so the zebra gets its settings back
    machine: Option<Typewriter>,
    config_path: PathBuf,
    flash: Rc<Cell<Option<Instant>>>,
    /// shown on the bottom line until the first key
    message: Option<String>,
    /// a file or printing problem, shown in red
    error: Option<String>,
    /// when a key was last pressed, to keep the cursor solid while typing
    last_key: Instant,
}

impl TypewriterApp {
    fn quit(&mut self, ctx: &egui::Context) {
        self.machine = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (events, alt_down) = ctx.input(|i| (i.events.clone(), i.modifiers.alt));
        let actions: Vec<Action> = events.iter().flat_map(|e| actions_for(e, alt_down)).collect();
        if actions.is_empty() {
            return;
        }
        // keys are the typewriter's, not for focusing or clicking widgets;
        // ctrl keys are left for the zoom shortcuts
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Key { modifiers, .. } => modifiers.ctrl,
                Event::Text(_) => false,
                _ => true,
            })
        });
        self.message = None;
        self.last_key = Instant::now();
        let Some(m) = self.machine.as_mut() else { return };
        for action in actions {
            match apply(action, m, &self.config_path) {
                Ok(true) => {}
                Ok(false) => return self.quit(ctx),
                Err(e) => {
                    typewriter::log_warning(&e);
                    self.error = Some(e);
                }
            }
        }
    }

    /// a setting on the info panel that can be clicked as well as keyed
    fn clickable(&self, ui: &mut egui::Ui, g: &Grid, row: usize, col: usize, text: &str) -> bool {
        let rect = g.cells(row, col, text.chars().count());
        let r = ui.interact(rect, ui.id().with((row, col)), Sense::CLICK);
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        r.clicked()
    }

    fn draw(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let Some(m) = self.machine.as_ref() else { return };
        let cols = MIN_COLS.max(m.width + 1);
        let g = Grid::fit(ui.ctx(), ui.max_rect(), cols);
        let p = ui.painter().clone();

        // the typing, newest line at the bottom, the tab bar under it
        g.put(&p, 0, 0, &m.prev_buf_3, pal.dim);
        g.put(&p, 1, 0, &m.prev_buf_2, pal.dim);
        g.put(&p, 2, 0, &m.prev_buf_1, pal.dim);
        let flashing = self.flash.get().is_some_and(|t| t.elapsed() < FLASH);
        if flashing {
            p.rect_filled(g.cells(3, 0, cols), 0.0, pal.error.gamma_multiply(0.25));
        }
        // the cursor: a solid block while typing, blinking when idle
        let idle = self.last_key.elapsed();
        let typing = m.give_buffer();
        if idle < BLINK || (idle.as_millis() / BLINK.as_millis()) % 2 == 0 {
            let mut block = g.cells(3, 0, 1);
            block = block.translate(Vec2::new(g.text_x(ui.ctx(), &typing, m.cursor) - block.min.x, 0.0));
            p.rect_filled(block, 0.0, pal.typing.gamma_multiply(0.35));
        }
        g.put(&p, 3, 0, &typing, pal.typing);
        g.put(&p, 4, 0, &m.give_tab_bar(), pal.dim);

        // the settings
        let r = 6;
        let ls = format!("LS={}", m.current_spacing);
        let artn = format!("A-RTN={}", py_bool(m.autoreturn));
        g.put(&p, r, 0, &format!("Width={}", m.width), pal.dim);
        g.put(&p, r, 10, &ls, pal.dim);
        g.put(&p, r, 17, &artn, pal.dim);
        g.put(&p, r, 30, &format!("LM={}", m.left_margin), pal.dim);
        g.put(&p, r, 36, &format!("RM={}", m.right_margin), pal.dim);
        if m.centre_mode() {
            g.put(&p, r, 44, "CENTRE", pal.highlight);
        } else if m.right_mode() {
            g.put(&p, r, 44, "RIGHT", pal.highlight);
        }
        let file_name = short_home_path(&m.file_name, cols - 1 - "File=".len());
        g.put(&p, r + 1, 0, &format!("File={file_name}"), pal.dim);
        g.put(&p, r + 2, 0, &format!("Printer Found={}", py_bool(m.printer_found)), pal.dim);
        g.put(&p, r + 2, 20, &format!("LC={}", m.line_count), pal.dim);
        g.put(&p, r + 2, 27, &format!("WC={}", m.word_count), pal.dim);
        g.put(&p, r + 2, 37, &format!("File={}", py_bool(m.use_file)), pal.highlight);
        g.put(&p, r + 3, 0, &format!("Printer={}", m.printer_name), pal.dim);
        let printing = format!("Printing={}", py_bool(m.use_printer));
        let bell = format!("Bell={}", py_bool(m.bell_on));
        g.put(&p, r + 3, 20, &printing, pal.highlight);
        g.put(&p, r + 3, 37, &bell, pal.highlight);
        let help = "alt-h for help";
        g.put(&p, r + 4, 0, help, pal.dim);

        if m.help_wanted {
            let folder = short_home_path(&m.save_folder, cols - 1 - "file path:".len());
            let lines = [
                "arrows/home/end move, del deletes, alt+backspace deletes a word. alt+ below:".to_string(),
                "l = set left margin  r = set right margin       s = toggle line spacing".into(),
                "a = set autoreturn   n = toggle margin release  q = quit  p = printer on/off".into(),
                "c = centre text      j = right justify (enter to finish)  b = bell on/off".into(),
                format!("file path:{folder}"),
            ];
            for (i, line) in lines.iter().enumerate() {
                g.put(&p, 11 + i, 0, line, pal.dim);
            }
        }

        if let Some(e) = &self.error {
            g.put(&p, MESSAGE_ROW, 0, &format!("problem: {e}"), pal.error);
        } else if let Some(msg) = &self.message {
            g.put(&p, MESSAGE_ROW, 0, msg, pal.highlight);
        }

        // the highlighted settings can be clicked too
        let key = if self.clickable(ui, &g, r, 10, &ls) {
            Some('s')
        } else if self.clickable(ui, &g, r, 17, &artn) {
            Some('a')
        } else if self.clickable(ui, &g, r + 3, 20, &printing) {
            Some('p')
        } else if self.clickable(ui, &g, r + 3, 37, &bell) {
            Some('b')
        } else if self.clickable(ui, &g, r + 4, 0, help) {
            Some('h')
        } else {
            None
        };
        if let (Some(c), Some(m)) = (key, self.machine.as_mut()) {
            // these never fail or quit
            let _ = apply(Action::Alt(c), m, &self.config_path);
        }

        // keep the blink and the flash going
        let blink_left = BLINK - Duration::from_millis((idle.as_millis() % BLINK.as_millis()) as u64);
        let next = if flashing { Duration::from_millis(16) } else { blink_left };
        ui.ctx().request_repaint_after(next);
    }
}

impl eframe::App for TypewriterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            self.machine = None;
            return;
        }
        self.handle_keys(ctx);
        if self.machine.is_none() {
            return;
        }
        let pal = Palette::new(ctx.style().visuals.dark_mode);
        let frame = egui::Frame::new().fill(pal.paper).inner_margin(16.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| self.draw(ui, &pal));
    }
}

/// shows a problem at startup, for when there's no terminal to print it in
struct ErrorApp(String);

impl eframe::App for ErrorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("typewriter can't start");
            ui.add_space(8.0);
            ui.label(&self.0);
            ui.add_space(8.0);
            if ui.button("close").clicked() || ctx.input(|i| i.key_pressed(Key::Escape)) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

fn window_options(size: [f32; 2]) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Typewriter")
            // wayland finds the window's .desktop file and icon by this
            .with_app_id("typewriter-gui")
            .with_inner_size(size)
            .with_min_inner_size([420.0, 200.0]),
        ..Default::default()
    }
}

/// load the config and build the machine
fn build(args: Args) -> Result<TypewriterApp, String> {
    let mut message = None;
    let config_path = match args.config {
        Some(p) => p,
        None => {
            let p = config::default_config_path()?;
            if config::create_if_missing(&p)? {
                message = Some(format!("created {} with the starting settings", p.display()));
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

    let flash = Rc::new(Cell::new(None));
    let base_dir = config_path.parent().unwrap_or(Path::new("."));
    let ding: Option<Sound> = BellSound::new(cfg.str("bell_sound").unwrap_or("ding.wav"), base_dir)
        .map(|mut s| -> Sound {
            let mut beep = make_beep(flash.clone());
            Box::new(move || {
                if !s.play() {
                    beep();
                }
            })
        });

    let mut machine = Typewriter::new(opts, printer, Some(make_beep(flash.clone())), ding);
    machine.save_folder = cfg.require_str("save_folder")?.to_string();
    if let Some(file) = args.file {
        machine.file_name = file_arg_path(&file);
        machine.use_file = true;
    }

    Ok(TypewriterApp {
        machine: Some(machine),
        config_path,
        flash,
        message,
        error: None,
        last_key: Instant::now(),
    })
}

fn main() {
    let args = Args::parse_checked(std::env::args_os()).unwrap_or_else(|e| e.exit());
    let result = match build(args) {
        Ok(app) => eframe::run_native(
            "typewriter-gui",
            window_options([960.0, 440.0]),
            Box::new(|_cc| Ok(Box::new(app))),
        ),
        Err(msg) => {
            eprintln!("{msg}");
            let _ = eframe::run_native(
                "typewriter-gui",
                window_options([520.0, 200.0]),
                Box::new(|_cc| Ok(Box::new(ErrorApp(msg)))),
            );
            std::process::exit(1);
        }
    };
    if let Err(e) = result {
        eprintln!("can't open a window: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    #[test]
    fn typed_text_becomes_characters() {
        assert_eq!(actions_for(&Event::Text("a".into()), false), vec![Action::Char('a')]);
        assert_eq!(actions_for(&Event::Text(" ".into()), false), vec![Action::Char(' ')]);
        // only what the printer can print
        assert!(actions_for(&Event::Text("é".into()), false).is_empty());
    }

    #[test]
    fn alt_letter_is_a_command_not_typing() {
        assert_eq!(actions_for(&key(Key::L, Modifiers::ALT), true), vec![Action::Alt('l')]);
        // the text that comes with alt-l is dropped
        assert!(actions_for(&Event::Text("l".into()), true).is_empty());
        assert_eq!(actions_for(&key(Key::Backspace, Modifiers::ALT), true), vec![Action::DeleteWord]);
    }

    #[test]
    fn editing_keys() {
        let k = |k| actions_for(&key(k, Modifiers::NONE), false);
        assert_eq!(k(Key::Enter), vec![Action::Enter]);
        assert_eq!(k(Key::Backspace), vec![Action::Backspace]);
        assert_eq!(k(Key::ArrowLeft), vec![Action::Left]);
        assert_eq!(k(Key::End), vec![Action::End]);
        assert_eq!(k(Key::Delete), vec![Action::Delete]);
        // letters come as text, not keys
        assert!(k(Key::A).is_empty());
    }

    #[test]
    fn ctrl_c_quits() {
        assert_eq!(actions_for(&key(Key::C, Modifiers::CTRL), false), vec![Action::Quit]);
        assert_eq!(actions_for(&Event::Copy, false), vec![Action::Quit]);
        assert!(actions_for(&key(Key::Plus, Modifiers::CTRL), false).is_empty());
    }

    #[test]
    fn typing_and_commands_drive_the_machine() {
        let dir = std::env::temp_dir().join(format!("tw-gui-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.toml");
        std::fs::write(&config_path, "left_margin = 0\nright_margin = 75\nspacing_index = 0\n").unwrap();
        let opts = Options { use_file: false, use_printer: false, ..Options::default() };
        let mut m = Typewriter::new(opts, None, None, None);
        for c in "hi there".chars() {
            assert!(apply(Action::Char(c), &mut m, &config_path).unwrap());
        }
        apply(Action::DeleteWord, &mut m, &config_path).unwrap();
        assert_eq!(m.give_buffer().trim_end(), "hi");
        apply(Action::Alt('s'), &mut m, &config_path).unwrap();
        let saved = std::fs::read_to_string(&config_path).unwrap();
        assert!(saved.contains(&format!("spacing_index = {}", m.spacing_index)), "{saved}");
        assert!(!apply(Action::Alt('q'), &mut m, &config_path).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn beep_file_is_a_wav_the_bell_can_read() {
        let p = beep_wav_path().unwrap();
        let w = typewriter::sound::parse_wav(&std::fs::read(p).unwrap()).unwrap();
        assert_eq!((w.sample_bytes, w.rate, w.channels), (2, 44100, 1));
    }

    #[test]
    fn short_path_fits() {
        assert_eq!(short_path("/home/me/writing/a.txt", 70, Some("/home/me")), "~/writing/a.txt");
        let short = short_path(&format!("/x/{}.txt", "a".repeat(100)), 30, None);
        assert!(short.len() == 30 && short.starts_with("...") && short.ends_with("a.txt"));
    }

    #[test]
    fn file_option_clashes_with_false() {
        assert!(Args::parse_checked(["typewriter-gui", "False", "-f", "notes.txt"]).is_err());
        assert_eq!(file_arg_path("test.txt"), "test.txt");
    }
}
