//! use the Zebra GK420d as a line printer.
//!
//! each line is sent as its own tiny ZPL "label" just one text line tall,
//! so the paper advances one line at a time like the dot matrix printer.
//! text is printed upside down by default, so when the strip is torn off
//! and turned round the lines read top-to-bottom in typing order.

use crate::config::Config;
use crate::typewriter::{Align, LinePrinter};
use crate::usb::UsbOut;
use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use std::fs::{File, OpenOptions};
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;

pub const VID: u16 = 0x0A5F;
pub const PID: u16 = 0x0080;
pub const WIDTH_DOTS: i64 = 812; // 4" at 203 dpi
pub const LEFT: i64 = 20;
pub const LINE_HEIGHT: f64 = 40.0;
pub const FONT_H: i64 = 30;
pub const FONT_W: i64 = 30;
/// rough fit for font A0 30x30 across 4"
pub const CHARS_PER_LINE: usize = 58;
/// width of one typewriter column on the paper, in dots
pub const CHAR_DOTS: i64 = 12;
pub const PRINTABLE: i64 = WIDTH_DOTS - 2 * LEFT;
/// capital letter height of font A0 30x30, in dots
pub const CAP_HEIGHT: f32 = 22.0;

/// ^ and ~ are ZPL command characters; drop them from label text
pub fn clean(text: &str) -> String {
    text.replace(['^', '~'], "")
}

/// round like python does (halves go to the even number)
fn round(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// a font file for drawing lines as pictures instead of the printer's
/// built-in font
pub struct ZebraFont {
    font: FontVec,
    scale: PxScale,
}

impl ZebraFont {
    /// load a font file. without a size the font is scaled so its capitals
    /// match the built-in font's height
    pub fn load(path: &Path, size: Option<f64>) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let font = FontVec::try_from_vec(bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        // size is the em height in dots, as pillow means it (ab_glyph's own
        // pt_to_px_scale assumes 96 dpi, which comes out a third bigger)
        let em = font.units_per_em().unwrap_or(1000.0);
        let scale_for = |size: f32| PxScale::from(size * font.height_unscaled() / em);
        let scale = match size {
            Some(s) => scale_for(s as f32),
            None => {
                let mut size = 10.0;
                while size < 500.0 && Self::cap_height(&font, scale_for(size + 1.0)) <= CAP_HEIGHT {
                    size += 1.0;
                }
                scale_for(size)
            }
        };
        Ok(ZebraFont { font, scale })
    }

    /// height of the letter H in whole dots. measured from the exact
    /// outline and rounded down, which picks the same size as pillow does
    /// (pillow's hinting snaps the capitals down onto the dot grid)
    fn cap_height(font: &FontVec, scale: PxScale) -> f32 {
        let Some(outline) = font.outline(font.glyph_id('H')) else { return f32::INFINITY };
        // font units run upwards, so min.y is the top here
        let unscaled = (outline.bounds.max.y - outline.bounds.min.y).abs();
        (unscaled * font.as_scaled(scale).v_scale_factor()).floor()
    }

    /// width of text in dots
    pub fn length(&self, text: &str) -> f32 {
        let sf = self.font.as_scaled(self.scale);
        let mut prev = None;
        let mut x = 0.0;
        for c in text.chars() {
            let id = sf.glyph_id(c);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            x += sf.h_advance(id);
            prev = Some(id);
        }
        x
    }

    /// (ascent, descent) in whole dots
    pub fn metrics(&self) -> (i64, i64) {
        let sf = self.font.as_scaled(self.scale);
        (sf.ascent().ceil() as i64, (-sf.descent()).ceil() as i64)
    }

    /// draw text in black on a width x height picture, packed for ZPL:
    /// 1 bit per dot, 1 = black, rows padded to whole bytes
    pub fn render(&self, text: &str, width: i64, height: i64) -> Vec<u8> {
        let row_bytes = ((width + 7) / 8) as usize;
        let mut bits = vec![0u8; row_bytes * height as usize];
        let sf = self.font.as_scaled(self.scale);
        let baseline = self.metrics().0 as f32;
        let mut prev = None;
        let mut caret = 0.0;
        for c in text.chars() {
            let id = sf.glyph_id(c);
            if let Some(p) = prev {
                caret += sf.kern(p, id);
            }
            let glyph = id.with_scale_and_position(self.scale, point(caret, baseline));
            caret += sf.h_advance(id);
            prev = Some(id);
            let Some(outline) = self.font.outline_glyph(glyph) else { continue };
            let b = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let x = b.min.x as i64 + gx as i64;
                let y = b.min.y as i64 + gy as i64;
                if coverage >= 0.5 && (0..width).contains(&x) && (0..height).contains(&y) {
                    bits[y as usize * row_bytes + x as usize / 8] |= 0x80 >> (x % 8);
                }
            });
        }
        bits
    }
}

/// ZPL graphic field with words drawn in font. place(width) gives the
/// left edge for a drawing that many dots wide
fn image_field(words: &str, place: impl Fn(f64) -> f64, font: &ZebraFont) -> String {
    let width = (round(font.length(words) as f64) + 2).min(PRINTABLE);
    let x = round(place(width as f64)).min(LEFT + PRINTABLE - width).max(LEFT);
    let (ascent, descent) = font.metrics();
    let height = ascent + descent;
    let bits = font.render(words, width, height);
    let row_bytes = (width + 7) / 8;
    let total = bits.len();
    let hex: String = bits.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02X}");
        s
    });
    format!("^FO{x},2^GFA,{total},{total},{row_bytes},{hex}^FS")
}

/// ZPL for one printed line.
///
/// with a font the line is drawn on the computer in that font and sent as
/// a picture; otherwise the printer's own font 0 is used.
///
/// font A0 is proportional (a space is much narrower than a letter), so
/// leading spaces can't be used to position text. instead each typewriter
/// column is char_dots wide on the paper and the printer places the text:
///   no align: text starts at the column of its first character
///   right:    text ends exactly at the column of its last character
///   centre:   text is centred on the middle of where it sits
pub fn line_zpl(
    text: &str,
    spacing: f64,
    align: Option<Align>,
    upright: bool,
    char_dots: i64,
    font: Option<&ZebraFont>,
) -> String {
    // ^POI rotates the line 180 degrees so the torn-off strip reads
    // top-to-bottom in typing order; ^PON is normal orientation
    let orient = if upright { "^PON" } else { "^POI" };
    let mut zpl = format!("^XA{orient}^LL{}", round(LINE_HEIGHT * spacing));
    let words = clean(text.trim());
    if words.is_empty() {
        // the printer skips a label with nothing on it, so a blank line
        // gets an invisible 1 dot white box to make it feed the paper
        zpl += "^FO0,0^GB1,1,1,W^FS";
        return zpl + "^XZ\n";
    }
    let start = (text.len() - text.trim_start().len()) as i64; // first column with text
    let end = text.trim_end().len() as i64; // column just after the text
    let cd = char_dots as f64;

    if let Some(font) = font {
        let field = match align {
            Some(Align::Right) => image_field(&words, |w| (LEFT + end * char_dots) as f64 - w, font),
            Some(Align::Centre) => image_field(
                &words,
                |w| LEFT as f64 + (start + end) as f64 / 2.0 * cd - w / 2.0,
                font,
            ),
            None => image_field(&words, |_| (LEFT + start * char_dots) as f64, font),
        };
        return zpl + &field + "^XZ\n";
    }

    let font = format!("^A0N,{FONT_H},{FONT_W}");
    match align {
        Some(Align::Right) => {
            let width = (end * char_dots).min(PRINTABLE);
            zpl += &format!("^FO{LEFT},5{font}^FB{width},1,0,R^FD{words}^FS");
        }
        Some(Align::Centre) => {
            let middle = ((start + end) as f64 / 2.0 * cd).min(PRINTABLE as f64);
            let half = round(middle.min(PRINTABLE as f64 - middle));
            let x = LEFT + round(middle) - half;
            zpl += &format!("^FO{x},5{font}^FB{},1,0,C^FD{words}^FS", 2 * half);
        }
        None => {
            let x = LEFT + (start * char_dots).min(PRINTABLE);
            zpl += &format!("^FO{x},5{font}^FD{words}^FS");
        }
    }
    zpl + "^XZ\n"
}

/// settings for the zebra, from the config or the command line
#[derive(Clone, Debug, Default)]
pub struct ZebraOptions {
    /// device node; None finds one automatically
    pub device: Option<String>,
    /// ignore label gaps and print straight across them
    pub continuous: bool,
    /// false prints upside down so torn-off strips read in order
    pub upright: bool,
    /// 0 (lightest) to 30 (darkest); None leaves the printer's own setting
    pub darkness: Option<i64>,
    pub char_dots: Option<i64>,
    pub font_file: Option<String>,
    pub font_size: Option<f64>,
}

impl ZebraOptions {
    pub fn from_config(c: &Config) -> Self {
        ZebraOptions {
            device: c.str("zebra_device").map(String::from),
            continuous: c.bool("zebra_continuous").unwrap_or(false),
            upright: c.bool("zebra_upright").unwrap_or(false),
            darkness: c.int("zebra_darkness"),
            char_dots: c.int("zebra_char_dots"),
            font_file: c.str("zebra_font").filter(|s| !s.is_empty()).map(String::from),
            font_size: c.float("zebra_font_size"),
        }
    }
}

enum Output {
    File(File),
    Usb(UsbOut),
}

/// holds the printer connection open and sends one line at a time
pub struct ZebraLinePrinter {
    out: Output,
    font: Option<ZebraFont>,
    char_dots: i64,
    upright: bool,
    closed: bool,
    /// description of how we're connected
    pub connection: String,
}

impl ZebraLinePrinter {
    pub fn open(opts: ZebraOptions) -> Result<Self, String> {
        let font = match &opts.font_file {
            Some(f) => Some(ZebraFont::load(Path::new(f), opts.font_size)?),
            None => None,
        };
        if let Some(d) = opts.darkness {
            if !(0..=30).contains(&d) {
                return Err(format!("darkness must be 0-30, got {d}"));
            }
        }
        let (out, connection) = Self::connect(opts.device.as_deref())?;
        let mut p = ZebraLinePrinter {
            out,
            font,
            char_dots: opts.char_dots.filter(|&d| d > 0).unwrap_or(CHAR_DOTS),
            upright: opts.upright,
            closed: false,
            connection,
        };
        // ^MNN = continuous media (no gap sensing), ^MMT = tear-off mode
        let media = if opts.continuous { "^MNN" } else { "" };
        p.write(format!("^XA^PW{WIDTH_DOTS}^LH0,0{media}^MMT^XZ\n").as_bytes())
            .map_err(|e| e.to_string())?;
        if let Some(d) = opts.darkness {
            // ~SD sets print darkness; lasts until the printer is switched off
            p.write(format!("~SD{d:02}\n").as_bytes()).map_err(|e| e.to_string())?;
        }
        Ok(p)
    }

    /// a /dev/usb/lp* node if the kernel's usblp driver has the printer,
    /// otherwise straight over USB
    fn connect(device: Option<&str>) -> Result<(Output, String), String> {
        let nodes: Vec<String> = match device {
            Some(d) => vec![d.to_string()],
            None => {
                let mut nodes: Vec<String> = std::fs::read_dir("/dev/usb")
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("lp"))
                    .map(|e| e.path().display().to_string())
                    .collect();
                nodes.sort();
                nodes
            }
        };
        for node in nodes {
            let opened = OpenOptions::new().write(true).create(true).truncate(true).open(&node);
            if let Ok(f) = opened {
                return Ok((Output::File(f), node));
            }
        }
        let usb = UsbOut::open(VID, PID)?;
        Ok((Output::Usb(usb), "usb".into()))
    }

    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        match &mut self.out {
            Output::File(f) => f.write_all(data),
            Output::Usb(u) => u.write(data),
        }
    }

    /// restore the printer's saved settings. printing a line at a time
    /// leaves the printer set to 5mm labels (and continuous media if that
    /// option is on), so its FEED button would only feed a line. ^JUR
    /// reloads the settings saved in the printer, putting label length,
    /// media type and darkness back. also happens when the printer is dropped
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _ = self.write(b"^XA^JUR^XZ\n"); // printer unplugged: nothing to restore
    }
}

impl LinePrinter for ZebraLinePrinter {
    fn print_line(&mut self, text: &str, spacing: f64, align: Option<Align>) -> io::Result<()> {
        let zpl = line_zpl(text, spacing, align, self.upright, self.char_dots, self.font.as_ref());
        self.write(zpl.as_bytes())
    }
}

impl Drop for ZebraLinePrinter {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROBOTO: &str = "/usr/share/fonts/truetype/roboto/unhinted/RobotoCondensed-Regular.ttf";

    fn zpl(text: &str, align: Option<Align>, char_dots: i64) -> String {
        line_zpl(text, 1.0, align, false, char_dots, None)
    }

    #[test]
    fn right_justify_ends_at_margin_column() {
        let z = zpl(&format!("{}end", " ".repeat(60)), Some(Align::Right), 12);
        // block from the left edge to column 63, text right aligned in it
        assert!(z.contains(&format!("^FO{LEFT},5")));
        assert!(z.contains("^FB756,1,0,R^FDend^FS"));
    }

    #[test]
    fn indent_uses_column_width_not_spaces() {
        let z = zpl("     hello", None, 12);
        assert!(z.contains(&format!("^FO{},5", LEFT + 60)));
        assert!(z.contains("^FDhello^FS"));
    }

    #[test]
    fn centre_is_centred_on_text_middle() {
        // text occupies columns 20-30, so its middle is column 25
        let z = zpl(&format!("{}{}", " ".repeat(20), "x".repeat(10)), Some(Align::Centre), 10);
        assert!(z.contains(&format!("^FO{LEFT},5"))); // block 0..500 dots
        assert!(z.contains("^FB500,1,0,C"));
    }

    #[test]
    fn blank_line_still_feeds_paper() {
        let z = zpl("     ", None, 12);
        assert!(!z.contains("^FD"));
        assert!(z.contains("^GB1,1,1,W^FS")); // invisible field so it prints
        assert!(z.contains("^LL40"));
    }

    #[test]
    fn right_justify_clamped_to_paper() {
        let z = zpl(&format!("{}end", " ".repeat(90)), Some(Align::Right), 12);
        assert!(z.contains("^FB772,1,0,R"));
    }

    #[test]
    fn spacing_and_orientation() {
        assert!(line_zpl("a", 1.5, None, false, 12, None).starts_with("^XA^POI^LL60"));
        assert!(line_zpl("a", 2.0, None, true, 12, None).starts_with("^XA^PON^LL80"));
    }

    #[test]
    fn command_characters_are_removed() {
        assert!(zpl("a^b~c", None, 12).contains("^FDabc^FS"));
    }

    #[test]
    fn close_restores_saved_printer_settings() {
        let dir = tempfile::tempdir().unwrap();
        let dev = dir.path().join("lp0"); // stands in for /dev/usb/lp0
        let mut p = ZebraLinePrinter::open(ZebraOptions {
            device: Some(dev.display().to_string()),
            continuous: true,
            ..Default::default()
        })
        .unwrap();
        p.print_line("hello", 1.0, None).unwrap();
        p.close();
        drop(p); // dropping after close sends nothing more
        let sent = std::fs::read(&dev).unwrap();
        assert!(sent.starts_with(b"^XA^PW")); // setup first
        assert_eq!(String::from_utf8_lossy(&sent).matches("^JUR").count(), 1);
        assert!(sent.trim_ascii_end().ends_with(b"^XA^JUR^XZ"));
    }

    #[test]
    fn bad_darkness_rejected() {
        let opts = ZebraOptions { darkness: Some(31), device: Some("/dev/null".into()), ..Default::default() };
        assert!(ZebraLinePrinter::open(opts).is_err());
    }

    #[test]
    fn font_line_is_a_picture_in_the_right_place() {
        if !Path::new(ROBOTO).exists() {
            return; // font not installed here
        }
        let font = ZebraFont::load(Path::new(ROBOTO), None).unwrap();
        let z = line_zpl("     hello", 1.0, None, false, 12, Some(&font));
        assert!(z.contains(&format!("^FO{},2^GFA,", LEFT + 60)), "{z}");
        assert!(!z.contains("^FD"));
        // the picture has some black in it, and its size adds up
        let gf = z.split("^GFA,").nth(1).unwrap();
        let parts: Vec<&str> = gf.trim_end_matches("^FS^XZ\n").split(',').collect();
        let total: usize = parts[0].parse().unwrap();
        let row_bytes: usize = parts[2].parse().unwrap();
        assert_eq!(parts[3].len(), total * 2);
        assert_eq!(total % row_bytes, 0);
        assert!(parts[3].chars().any(|c| c != '0'));
    }

    #[test]
    fn auto_font_size_matches_builtin_cap_height() {
        if !Path::new(ROBOTO).exists() {
            return;
        }
        let font = ZebraFont::load(Path::new(ROBOTO), None).unwrap();
        let h = ZebraFont::cap_height(&font.font, font.scale);
        assert!((CAP_HEIGHT - 2.0..=CAP_HEIGHT).contains(&h), "{h}");
    }

    #[test]
    fn font_right_align_ends_at_column() {
        if !Path::new(ROBOTO).exists() {
            return;
        }
        let font = ZebraFont::load(Path::new(ROBOTO), None).unwrap();
        let z = line_zpl(&format!("{}end", " ".repeat(60)), 1.0, Some(Align::Right), false, 12, Some(&font));
        let width = round(font.length("end") as f64) + 2;
        assert!(z.contains(&format!("^FO{},2", LEFT + 63 * 12 - width)), "{z}");
    }
}
