#!/usr/bin/env python3
"""Use the Zebra GK420d as a line printer: type a line, press Enter, it prints.

Each line is sent as its own tiny ZPL "label" just one text line tall, so the
paper advances one line at a time like the dot-matrix printer in type2.py.

Usage:
    python3 zebra_lines.py
    python3 zebra_lines.py --device /dev/usb/lp0
    python3 zebra_lines.py --continuous     # for continuous (gapless) roll media
    python3 zebra_lines.py --upright        # don't invert the text

Text is printed upside down by default, so when the strip is torn off and
turned round the lines read top-to-bottom in the order they were typed.

Empty line = blank line. Ctrl-D, Ctrl-C or 'quit' to exit.
"""
import argparse
import atexit
import glob
import sys
import textwrap

from zebra_test import VID, PID, WIDTH_DOTS, LEFT, LINE_HEIGHT, FONT_H, FONT_W, clean

CHARS_PER_LINE = 58   # rough fit for font A0 30x30 across 4"
CHAR_DOTS = 12        # width of one typewriter column on the paper, in dots
PRINTABLE = WIDTH_DOTS - 2 * LEFT
CAP_HEIGHT = 22       # capital letter height of font A0 30x30, in dots


def load_font(path, size=None):
    """load a font file for drawing lines as images. without a size the
    font is scaled so its capitals match the built-in font's height"""
    from PIL import ImageFont      # only needed when a font file is used
    if size:
        return ImageFont.truetype(path, size)
    size = 10
    while True:
        bigger = ImageFont.truetype(path, size + 1)
        top, bottom = bigger.getbbox("H")[1::2]
        if bottom - top > CAP_HEIGHT:
            return ImageFont.truetype(path, size)
        size += 1


def image_field(words, x_for_width, font):
    """ZPL graphic field with words drawn in font. x_for_width(width)
    gives the left edge for a drawing that many dots wide"""
    from PIL import Image, ImageDraw
    width = min(round(font.getlength(words)) + 2, PRINTABLE)
    x = max(LEFT, min(round(x_for_width(width)), LEFT + PRINTABLE - width))
    ascent, descent = font.getmetrics()
    img = Image.new("1", (width, ascent + descent), 1)
    ImageDraw.Draw(img).text((0, 0), words, font=font, fill=0)
    # ZPL graphics are 1 bit per dot, 1 = black, rows padded to whole bytes;
    # PIL's "1" mode is the same packing with 1 = white, so just invert
    row_bytes = (width + 7) // 8
    data = bytes(b ^ 0xFF for b in img.tobytes())
    # clear the padding bits at the end of each row that inverting set
    pad_mask = (0xFF << (row_bytes * 8 - width)) & 0xFF
    rows = [bytearray(data[i:i + row_bytes]) for i in range(0, len(data), row_bytes)]
    for row in rows:
        row[-1] &= pad_mask
    hexdata = b"".join(rows).hex().upper()
    total = row_bytes * len(rows)
    return f"^FO{x},2^GFA,{total},{total},{row_bytes},{hexdata}^FS"


def line_zpl(text, spacing=1, align=None, upright=False, char_dots=CHAR_DOTS,
             font=None):
    """ZPL for one printed line.

    with font (from load_font) the line is drawn on the computer in that
    font and sent as a picture; otherwise the printer's own font 0 is used.

    font A0 is proportional (a space is much narrower than a letter), so
    leading spaces can't be used to position text. instead each typewriter
    column is char_dots wide on the paper and the printer places the text:
      align None/"left": text starts at the column of its first character
      align "right":     text ends exactly at the column of its last character
      align "centre":    text is centred on the middle of where it sits
    """
    # ^POI rotates the line 180 degrees so the torn-off strip reads
    # top-to-bottom in typing order; ^PON is normal orientation.
    orient = "^PON" if upright else "^POI"
    zpl = f"^XA{orient}^LL{round(LINE_HEIGHT * spacing)}"
    words = clean(text.strip())
    if words:
        start = len(text) - len(text.lstrip())   # first column with text
        end = len(text.rstrip())                 # column just after the text
        if font is not None:
            if align == "right":
                place = lambda w: LEFT + end * char_dots - w
            elif align == "centre":
                place = lambda w: LEFT + (start + end) / 2 * char_dots - w / 2
            else:
                place = lambda w: LEFT + start * char_dots
            return zpl + image_field(words, place, font) + "^XZ\n"
        font = f"^A0N,{FONT_H},{FONT_W}"
        if align == "right":
            width = min(end * char_dots, PRINTABLE)
            zpl += f"^FO{LEFT},5{font}^FB{width},1,0,R^FD{words}^FS"
        elif align == "centre":
            middle = min((start + end) / 2 * char_dots, PRINTABLE)
            half = round(min(middle, PRINTABLE - middle))
            x = LEFT + round(middle) - half
            zpl += f"^FO{x},5{font}^FB{2 * half},1,0,C^FD{words}^FS"
        else:
            x = LEFT + min(start * char_dots, PRINTABLE)
            zpl += f"^FO{x},5{font}^FD{words}^FS"
    else:
        # the printer skips a label with nothing on it, so a blank line
        # gets an invisible 1 dot white box to make it feed the paper
        zpl += "^FO0,0^GB1,1,1,W^FS"
    return zpl + "^XZ\n"


class ZebraLinePrinter:
    """Holds the printer connection open and sends one line at a time."""

    def __init__(self, device=None, continuous=False, upright=False,
                 darkness=None, char_dots=None, font_file=None,
                 font_size=None):
        self.continuous = continuous
        # a font file draws lines as pictures instead of the built-in font
        self.font = load_font(font_file, font_size) if font_file else None
        self.char_dots = char_dots or CHAR_DOTS
        self.upright = upright
        # 0 (lightest) to 30 (darkest); None leaves the printer's own setting
        if darkness is not None and not 0 <= darkness <= 30:
            raise ValueError(f"darkness must be 0-30, got {darkness}")
        self.darkness = darkness
        self._file = None
        self._ep = None
        self._dev = None
        self.connection = None   # description of how we're connected
        self._open(device)
        self._setup()
        self._closed = False
        # put the printer back how it was however the program ends
        atexit.register(self.close)

    def _open(self, device):
        # no printing to the screen in here: type2.py runs this under curses
        nodes = [device] if device else sorted(glob.glob("/dev/usb/lp*"))
        for node in nodes:
            try:
                self._file = open(node, "wb", buffering=0)
                self.connection = node
                return
            except OSError:
                pass

        import usb.core
        import usb.util

        dev = usb.core.find(idVendor=VID, idProduct=PID)
        if dev is None:
            raise RuntimeError("Printer 0a5f:0080 not found on USB")
        self._detached = False
        if dev.is_kernel_driver_active(0):
            dev.detach_kernel_driver(0)
            self._detached = True     # give it back to the kernel on close
        dev.set_configuration()
        intf = dev.get_active_configuration()[(0, 0)]
        ep = usb.util.find_descriptor(
            intf,
            custom_match=lambda e: usb.util.endpoint_direction(e.bEndpointAddress)
            == usb.util.ENDPOINT_OUT,
        )
        if ep is None:
            raise RuntimeError("No OUT endpoint found")
        self._dev, self._ep = dev, ep
        self.connection = "pyusb"

    def _write(self, data):
        if self._file:
            self._file.write(data)
        else:
            self._ep.write(data)

    def _setup(self):
        # ^MNN = continuous media (no gap sensing), ^MMT = tear-off mode.
        media = "^MNN" if self.continuous else ""
        self._write(f"^XA^PW{WIDTH_DOTS}^LH0,0{media}^MMT^XZ\n".encode())
        if self.darkness is not None:
            # ~SD sets print darkness; lasts until the printer is switched off
            self._write(f"~SD{self.darkness:02d}\n".encode())

    def print_line(self, text, spacing=1, align=None):
        """print one line; spacing (1, 1.5, 2) scales the paper feed and
        align (None, "centre", "right") says how the typewriter laid it out"""
        zpl = line_zpl(text, spacing, align, self.upright, self.char_dots,
                       self.font)
        self._write(zpl.encode("utf-8"))

    def close(self):
        """restore the printer's saved settings and let go of it.

        printing a line at a time leaves the printer set to 5mm labels
        (and continuous media if that option is on), so its FEED button
        would only feed a line. ^JUR reloads the settings saved in the
        printer, putting label length, media type and darkness back."""
        if self._closed:
            return
        self._closed = True
        try:
            self._write(b"^XA^JUR^XZ\n")
        except Exception:
            pass                       # printer unplugged: nothing to restore
        if self._file:
            self._file.close()
        elif self._dev:
            import usb.util
            usb.util.dispose_resources(self._dev)
            if self._detached:
                try:
                    self._dev.attach_kernel_driver(0)
                except Exception:
                    pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--device", help="e.g. /dev/usb/lp0")
    ap.add_argument("--continuous", action="store_true",
                    help="set continuous media mode (no label gaps)")
    ap.add_argument("--width", type=int, default=CHARS_PER_LINE,
                    help="wrap lines longer than this many characters")
    ap.add_argument("--upright", action="store_true",
                    help="print text the normal way up (default is inverted)")
    ap.add_argument("--darkness", type=int,
                    help="print darkness 0 (lightest) to 30 (darkest)")
    ap.add_argument("--font", help="font file to draw text with instead "
                    "of the printer's built-in font")
    args = ap.parse_args()

    try:
        printer = ZebraLinePrinter(args.device, args.continuous, args.upright,
                                   args.darkness, font_file=args.font)
    except Exception as e:
        print(f"Could not open printer: {e}", file=sys.stderr)
        print("Hint: run with sudo, add a udev rule for 0a5f:0080, "
              "or load usblp (sudo modprobe usblp).", file=sys.stderr)
        sys.exit(1)

    print(f"Using {printer.connection}")
    print("Type a line and press Enter to print it. Ctrl-D or 'quit' to exit.")
    try:
        while True:
            try:
                line = input("> ")
            except EOFError:
                break
            if line.strip().lower() == "quit":
                break
            for chunk in textwrap.wrap(line, args.width) or [""]:
                printer.print_line(chunk)
    except KeyboardInterrupt:
        pass
    finally:
        printer.close()
        print()


if __name__ == "__main__":
    main()
