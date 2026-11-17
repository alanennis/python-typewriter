//! use the Zebra GK420d as a line printer on its own: type a line, press
//! enter, it prints. a quick test that the printer is working.
//!
//! empty line = blank line. ctrl-d, ctrl-c or 'quit' to exit.

use clap::Parser;
use std::io::{self, BufRead, Write};
use typewriter::typewriter::LinePrinter;
use typewriter::zebra::{CHARS_PER_LINE, ZebraLinePrinter, ZebraOptions};

#[derive(Parser)]
struct Args {
    /// e.g. /dev/usb/lp0
    #[arg(long)]
    device: Option<String>,
    /// set continuous media mode (no label gaps)
    #[arg(long)]
    continuous: bool,
    /// wrap lines longer than this many characters
    #[arg(long, default_value_t = CHARS_PER_LINE)]
    width: usize,
    /// print text the normal way up (default is inverted)
    #[arg(long)]
    upright: bool,
    /// print darkness 0 (lightest) to 30 (darkest)
    #[arg(long)]
    darkness: Option<i64>,
    /// font file to draw text with instead of the printer's built-in font
    #[arg(long)]
    font: Option<String>,
}

/// split a line into pieces of at most width characters, breaking at
/// spaces where possible
fn wrap(line: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in line.split_whitespace() {
        let mut word = word.to_string();
        if !cur.is_empty() && cur.len() + 1 + word.len() > width {
            out.push(std::mem::take(&mut cur));
        }
        while word.len() > width.max(1) {
            let rest = word.split_off(width.max(1));
            out.push(std::mem::replace(&mut word, rest));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&word);
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

fn main() {
    let args = Args::parse();
    let mut printer = match ZebraLinePrinter::open(ZebraOptions {
        device: args.device,
        continuous: args.continuous,
        upright: args.upright,
        darkness: args.darkness,
        font_file: args.font,
        ..Default::default()
    }) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Could not open printer: {e}");
            eprintln!(
                "Hint: run with sudo, add a udev rule for 0a5f:0080, or load usblp (sudo modprobe usblp)."
            );
            std::process::exit(1);
        }
    };

    println!("Using {}", printer.connection);
    println!("Type a line and press Enter to print it. Ctrl-D or 'quit' to exit.");
    let stdin = io::stdin();
    loop {
        print!("> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = line.trim_end_matches(['\n', '\r']);
        if line.trim().eq_ignore_ascii_case("quit") {
            break;
        }
        for chunk in wrap(line, args.width) {
            if let Err(e) = printer.print_line(&chunk, 1.0, None) {
                eprintln!("printing failed: {e}");
            }
        }
    }
    printer.close();
    println!();
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn wraps_at_spaces() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("", 10), [""]);
        assert_eq!(wrap("   ", 10), [""]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    }
}
