//! typewriter for a line printer: type a line, press enter, it prints.
//!
//! the pieces are kept apart so they can be tested without a terminal
//! or a printer attached.

pub mod config;
pub mod printer;
pub mod sound;
pub mod typewriter;
pub mod usb;
pub mod zebra;

use std::fs::OpenOptions;
use std::io::Write;

/// append a warning to type.log in the current folder, like the python
/// version's logging.warning. nothing may be printed to the screen while
/// the typewriter is running, so this is the only place problems go
pub fn log_warning(msg: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("type.log") {
        let _ = writeln!(f, "WARNING:root:{msg}");
    }
}
