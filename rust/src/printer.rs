//! the printers the typewriter can drive, and choosing one from the config

use crate::config::Config;
use crate::typewriter::{Align, LinePrinter};
use crate::usb::UsbOut;
use crate::zebra::{ZebraLinePrinter, ZebraOptions};
use std::io;

/// dot matrix printer that speaks ESC/POS over USB
pub struct EpsonPrinter {
    usb: UsbOut,
}

impl EpsonPrinter {
    pub fn open(vendor_id: u16, product_id: u16) -> Result<Self, String> {
        Ok(EpsonPrinter { usb: UsbOut::open(vendor_id, product_id)? })
    }
}

/// ESC 3 n: line spacing of n/180 inch, for 1, 1.5 and double spacing
pub fn epson_line_bytes(text: &str, spacing: f64) -> Vec<u8> {
    let n = if spacing >= 2.0 {
        60
    } else if spacing >= 1.5 {
        40
    } else {
        30
    };
    let mut data = vec![0x1b, b'3', n];
    data.extend_from_slice(text.as_bytes());
    data.push(b'\n');
    data
}

impl LinePrinter for EpsonPrinter {
    // every character is the same width on the dot matrix, so the spaces
    // in the line already put it in the right place
    fn print_line(&mut self, text: &str, spacing: f64, _align: Option<Align>) -> io::Result<()> {
        self.usb.write(&epson_line_bytes(text, spacing))
    }
}

/// which printer the config asks for, epson if it doesn't say
pub fn printer_name(config: &Config) -> String {
    config.str("printer").unwrap_or("epson").to_lowercase()
}

fn usb_id(config: &Config, key: &str) -> Result<u16, String> {
    config
        .int(key)
        .and_then(|v| u16::try_from(v).ok())
        .ok_or_else(|| format!("{key} missing from config"))
}

/// build the printer named by `printer` in the config ("epson" or
/// "zebra"). gives Ok(None) if it can't be opened, so the typewriter still
/// runs without one; an unknown printer name is an error
pub fn open_printer(config: &Config) -> Result<Option<Box<dyn LinePrinter>>, String> {
    let kind = printer_name(config);
    let opened: Result<Box<dyn LinePrinter>, String> = match kind.as_str() {
        "epson" => (|| {
            let vid = usb_id(config, "printer_vendor_id")?;
            let pid = usb_id(config, "printer_product_id")?;
            Ok(Box::new(EpsonPrinter::open(vid, pid)?) as Box<dyn LinePrinter>)
        })(),
        "zebra" => ZebraLinePrinter::open(ZebraOptions::from_config(config))
            .map(|p| Box::new(p) as Box<dyn LinePrinter>),
        _ => return Err(format!("unknown printer \"{kind}\" in config, use \"epson\" or \"zebra\"")),
    };
    match opened {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            crate::log_warning(&format!("could not open {kind} printer: {e}"));
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> Config {
        Config::parse(text).unwrap()
    }

    #[test]
    fn unknown_printer_type_is_an_error() {
        assert!(open_printer(&cfg("printer = \"laser\"")).is_err());
    }

    #[test]
    fn missing_epson_ids_gives_no_printer() {
        assert!(open_printer(&cfg("printer = \"epson\"")).unwrap().is_none());
    }

    #[test]
    fn printer_name_defaults_to_epson() {
        assert_eq!(printer_name(&cfg("")), "epson");
        assert_eq!(printer_name(&cfg("printer = \"Zebra\"")), "zebra");
    }

    #[test]
    fn epson_line_sets_spacing_then_text() {
        assert_eq!(epson_line_bytes("hi", 1.0), b"\x1b3\x1ehi\n");
        assert_eq!(epson_line_bytes("", 1.5), b"\x1b3\x28\n");
        assert_eq!(epson_line_bytes("", 2.0), b"\x1b3\x3c\n");
    }
}
