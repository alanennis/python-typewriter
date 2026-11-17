import curses
from curses import wrapper
from os import environ, get_terminal_size, path
from time import strftime
from escpos.printer import Usb
import tomllib
import logging
import re
from pathlib import Path
import argparse
import atexit
import shutil
import subprocess
import threading
import wave

CONFIG_PATH = Path(__file__).resolve().parent / "config.toml"


def save_settings_to_config(settings, config_path=CONFIG_PATH):
    """write whole number or true/false settings (a dict of key: value)
    back to the config file, leaving the rest of the file (including
    comments) untouched"""
    text = config_path.read_text()
    for key, value in settings.items():
        if isinstance(value, bool):
            value = "true" if value else "false"
        pattern = rf"^({key}\s*=\s*)(?:\d+|true|false)"
        if re.search(pattern, text, flags=re.MULTILINE):
            text = re.sub(pattern, rf"\g<1>{value}", text, count=1, flags=re.MULTILINE)
        else:
            if not text.endswith("\n"):
                text += "\n"
            text += f"{key} = {value}\n"
    config_path.write_text(text)


def save_margins_to_config(left_margin, right_margin, config_path=CONFIG_PATH):
    """write the margin values back to the config file"""
    save_settings_to_config({"left_margin": left_margin,
                             "right_margin": right_margin}, config_path)


class EpsonPrinter():
    """dot matrix printer that speaks ESC/POS over USB"""
    # line_spacing values for 1, 1.5 and double spacing
    RAW_SPACING = {1: 30, 1.5: 40, 2: 60}

    def __init__(self, vendor_id, product_id):
        self.p = Usb(vendor_id, product_id, 0)

    def print_line(self, text, spacing=1, align=None):
        # every character is the same width on the dot matrix, so the
        # spaces in the line already put it in the right place
        self.p.line_spacing(self.RAW_SPACING[spacing])
        self.p.text(text)
        self.p.text("\n")


def printer_name(config):
    """which printer the config asks for, epson if it doesn't say"""
    return config.get('printer', 'epson').lower()


def open_printer(config):
    """build the printer named by `printer` in the config ("epson" or
    "zebra"). returns None if it can't be found, so the typewriter
    still runs without one"""
    kind = printer_name(config)
    try:
        if kind == 'epson':
            return EpsonPrinter(config['printer_vendor_id'],
                                config['printer_product_id'])
        if kind == 'zebra':
            from zebra_lines import ZebraLinePrinter
            return ZebraLinePrinter(
                device=config.get('zebra_device'),
                continuous=config.get('zebra_continuous', False),
                upright=config.get('zebra_upright', False),
                darkness=config.get('zebra_darkness'),
                char_dots=config.get('zebra_char_dots'),
                font_file=config.get('zebra_font') or None,
                font_size=config.get('zebra_font_size'))
    except Exception as e:
        logging.warning(f"could not open {kind} printer: {e}")
        return None
    raise SystemExit(f'unknown printer "{kind}" in config, use "epson" or "zebra"')


SOUND_PLAYERS = ("paplay", "pw-play", "aplay")
# pacat sample format names for each wav sample width in bytes
PACAT_FORMATS = {1: "u8", 2: "s16le", 3: "s24le", 4: "s32le"}


class StreamingSound():
    """plays a wav file with almost no delay.

    starting a player program for every ding takes around a tenth of a
    second, and the sound card goes to sleep when nothing is playing,
    which adds more. instead one pacat is started up front with an open
    stream, and each ding just writes the sound into it."""

    def __init__(self, path):
        with wave.open(str(path), "rb") as w:
            fmt = PACAT_FORMATS[w.getsampwidth()]
            rate, channels = w.getframerate(), w.getnchannels()
            self.frames = w.readframes(w.getnframes())
        self.proc = subprocess.Popen(
            ["pacat", "--playback", "--raw", f"--format={fmt}",
             f"--rate={rate}", f"--channels={channels}", "--latency-msec=20",
             "--client-name=typewriter", "--stream-name=margin bell"],
            stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL)
        self.playing = threading.Event()
        self.alive = True
        atexit.register(self.close)

    def _write(self):
        # runs in a thread because the write only finishes as the sound
        # plays, and typing mustn't wait for that
        try:
            self.proc.stdin.write(self.frames)
            self.proc.stdin.flush()
        except (OSError, ValueError):
            self.alive = False
        finally:
            self.playing.clear()

    def play(self):
        """start the sound; returns False if the player has died"""
        if not self.alive or self.proc.poll() is not None:
            self.alive = False
            return False
        if not self.playing.is_set():       # still ringing: don't stack up
            self.playing.set()
            threading.Thread(target=self._write, daemon=True).start()
        return True

    def close(self):
        if self.proc.poll() is None:
            self.proc.terminate()


def sound_player(sound_file, fallback):
    """return a function that plays sound_file in the background without
    waiting for it to finish. a relative path is taken from the folder
    this script is in. wav files are streamed through pacat for the
    quickest response; other files start a player each time. if the file
    or a player program can't be found the fallback (the terminal beep)
    is returned instead"""
    if not sound_file:
        return fallback
    path = Path(sound_file).expanduser()
    if not path.is_absolute():
        path = CONFIG_PATH.parent / path
    if not path.is_file():
        logging.warning(f"can't find {path}, using terminal beep")
        return fallback

    stream = None
    if path.suffix.lower() == ".wav" and shutil.which("pacat"):
        try:
            stream = StreamingSound(path)
        except (OSError, wave.Error, KeyError, EOFError) as e:
            logging.warning(f"can't stream {path} ({e}), starting a player each time")

    player = next((p for p in SOUND_PLAYERS if shutil.which(p)), None)
    if stream is None and player is None:
        logging.warning(f"no sound player found, using terminal beep")
        return fallback

    def play():
        if stream is not None and stream.play():
            return
        if player is None:
            fallback()
            return
        try:
            subprocess.Popen([player, str(path)], stdin=subprocess.DEVNULL,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except OSError:
            fallback()
    return play


def set_shorter_esc_delay_in_os(delay_ms):
    environ.setdefault('ESCDELAY', delay_ms)

class Typewriter():
    def __init__(self, width = 75, spacing_index = 1,
                autoreturn=False, margin_bell=8,
                left_margin = 0, right_margin = None,
                use_file = True, use_printer = True,
                printer = None, printer_name = "none", bell = None,
                ding = None, bell_on = True):
        # bell is called whenever the machine wants to beep (line full,
        # cursor at the end of the line...). the class knows nothing about
        # the screen, so the caller decides what a beep is (curses.beep in
        # the real program, a no-op by default)
        self._bell_sound = bell if bell is not None else (lambda: None)
        # ding is the margin bell when you reach the hot zone, like the
        # bell on a real typewriter. it uses the plain bell unless given
        self._ding_sound = ding if ding is not None else self._bell_sound
        # bell_on False silences both
        self.bell_on = bell_on
        self.width = width
        self.autoreturn = autoreturn
        self.spacing_choices = (1, 1.5, 2)
        self.spacing_index = spacing_index
        self.current_spacing = self.spacing_choices[self.spacing_index]
        self.buffer = []
        self.margin_bell = margin_bell
        if right_margin is None:
            right_margin = width
        self.right_margin = min(right_margin, width)
        self.left_margin = left_margin
        self.margin_release = False
        self.prev_buf_1 = ''
        self.prev_buf_2 = ''
        self.prev_buf_3 = ''
        self.today = strftime("%Y-%m-%d-%H%M%S")
        self.file_name = f"{self.today}.txt"
        self.use_file = use_file
        self.use_printer = use_printer
        self.margin_hot_zone = False
        self.help_wanted = False
        # None, "centre" or "right" while aligning the line being typed
        self.align = None
        self.align_chars = []
        self.word_count = 0
        self.line_count = 0
        self.save_folder = "./"
        self.buffer.extend(" " * self.left_margin)
        # column the next character will be typed at
        self.cursor = len(self.buffer)
        # printer is any object with print_line(text, spacing, align), or None.
        # every carriage return calls it, blank lines included, and the
        # printer must feed a line for a blank one just as for text
        self.printer = printer
        self.printer_found = printer is not None
        # name of the chosen printer, shown on screen even if not found
        self.printer_name = printer_name


    def __str__(self):
        specs = f"line width = {self.width}\n" \
                f"line spacing = {self.current_spacing}\n" \
                f"Autoretun = {self.autoreturn}\n" \
                f"Margin bell = {self.margin_bell}\n" \
                f"file name = {self.file_name}\n" \
                f"use file = {self.use_file}\n" \
                f"use printer = {self.use_printer}\n"

        return specs

    def _right_marker_index(self):
        """tab bar column used to show the right margin"""
        return min(self.right_margin, self.width - 1)

    def give_buffer(self):
        return_string = "".join(self.buffer)
        return return_string

    def give_prev_buff_1(self):
        return self.prev_buf_1

    def give_prev_buff_2(self):
        return self.prev_buf_2

    def give_prev_buff_3(self):
        return self.prev_buf_3

    def bell(self):
        """beep for an error, unless the bell is switched off"""
        if self.bell_on:
            self._bell_sound()

    def ding(self):
        """ring the margin bell, unless the bell is switched off"""
        if self.bell_on:
            self._ding_sound()

    def toggle_bell(self):
        """switch all bell sounds off or on"""
        self.bell_on = not self.bell_on

    def hot_zone_start(self):
        """column where the margin bell rings and the hot zone begins"""
        return self.right_margin - self.margin_bell

    def give_tab_bar(self):
        """ the margin line: '!' at each margin, ':' where the hot zone
        starts and '.' everywhere else"""
        bar = ["."] * self.width
        hot = self.hot_zone_start()
        if 0 <= hot < self.width:
            bar[hot] = ":"
        bar[self.left_margin] = "!"
        bar[self._right_marker_index()] = "!"
        return "".join(bar)

    def toggle_line_space(self):
        self.spacing_index += 1
        if (self.spacing_index >= len(self.spacing_choices)):
            self.spacing_index = 0

        self.current_spacing = self.spacing_choices[self.spacing_index]

    def toggle_autoreturn(self):
            self.autoreturn = not self.autoreturn

    def char_add(self, key):
        """add a character to buffer at the cursor but check margins etc"""
        logging.debug(f"buff len = {len(self.buffer)}")
        logging.debug(f"autortn = {self.autoreturn}")
        if self.align:
            self.align_add(key)
            return
        # if the buffer is at the margin bell point then
        # make the bell ring and set the hot zone flag
        if (len(self.buffer) == self.hot_zone_start()):
            self.ding()
        self.margin_hot_zone = len(self.buffer) >= self.hot_zone_start()

        # if the buffer is less than the right margin but not longer
        # than the printable width then add character to the buffer
        fits = ((len(self.buffer) < self.right_margin or self.margin_release)
                and (len(self.buffer) < self.width - 1))
        # typing in the middle of the line (after moving back with the
        # arrow keys) inserts, and must never trigger an autoreturn
        at_end = self.cursor == len(self.buffer)
        if fits:
            self.buffer.insert(self.cursor, key)
            self.cursor += 1

        # if the key is in the hot zone and autoreturn is on and
        # you hit space or hypen or reach the end of line
        # then you will get an autoreturn
        # if you reach the end of the line and auto is not on then just ding
        if ((self.margin_hot_zone and key in (" ", "-")) or not fits):
            if self.autoreturn and at_end:
                self.carriage_return()
            else:
                self.bell()


    def backspace(self):
        """move the cursor back one space and delete the char before it"""
        if self.align:
            if self.align_chars:
                self.align_chars.pop()
                self._layout_align()
            return

        if (len(self.buffer) > 0 and self.cursor > 0):
            logging.debug(f'length is {len(self.buffer)}')
            logging.debug(f'position is {self.cursor}')
            self.buffer.pop(self.cursor - 1)
            self.cursor -= 1

    @staticmethod
    def _word_start(chars, end, floor=0):
        """index where the word before position end starts, skipping any
        spaces between it and end first. never goes below floor"""
        i = end
        while i > floor and chars[i - 1] == " ":
            i -= 1
        while i > floor and chars[i - 1] != " ":
            i -= 1
        return i

    def delete_word(self):
        """delete back to the start of the word before the cursor. stops at
        the left margin, unless the cursor is already at or left of it"""
        if self.align:
            start = self._word_start(self.align_chars, len(self.align_chars))
            del self.align_chars[start:]
            self._layout_align()
            return

        floor = self.left_margin if self.cursor > self.left_margin else 0
        start = self._word_start(self.buffer, self.cursor, floor)
        del self.buffer[start:self.cursor]
        self.cursor = start

    def delete(self):
        """delete the character under the cursor"""
        if self.align:
            return
        if self.cursor < len(self.buffer):
            self.buffer.pop(self.cursor)

    def cursor_left(self):
        """move the cursor one place left, ding at the start of the line"""
        if self.align or self.cursor == 0:
            self.bell()
            return
        self.cursor -= 1

    def cursor_right(self):
        """move the cursor one place right, ding at the end of the text"""
        if self.align or self.cursor >= len(self.buffer):
            self.bell()
            return
        self.cursor += 1

    def cursor_home(self):
        """move the cursor to the left margin, or to the very start of
        the line if it is already at (or left of) the margin"""
        if self.align:
            return
        margin = min(self.left_margin, len(self.buffer))
        self.cursor = margin if self.cursor > margin else 0

    def cursor_end(self):
        """move the cursor to the end of the text"""
        if self.align:
            return
        self.cursor = len(self.buffer)

    def buffer_ripple(self):
        """ bump all the buffers contents up the chain"""
        self.prev_buf_3 = self.prev_buf_2
        self.prev_buf_2 = self.prev_buf_1
        self.prev_buf_1 = self.give_buffer()


    def carriage_return(self):
        """record a carriage return and then depending on return mode
        do what needs to be done
        """
        self.margin_hot_zone = False

        # split the line based on spaces and then count the size of the result
        self.line = "".join(self.buffer)
        self.word_count += len(self.line.split())

        # add one to the line count.
        self.line_count += 1

        #send contents of line to file if using file
        if (self.use_file):
            self.line_for_file = "".join(self.buffer)
            file_path = path.join(self.save_folder, self.file_name)
            try:
                with open(file_path, 'a') as self.f1:
                    self.f1.write(self.line_for_file + "\n")
            except OSError as e:
                raise SystemExit(f"problem writing to the file {file_path}: {e}")

        #send contents of line to printer
        if (self.use_printer and self.printer_found):
            self.send_to_printer(self.buffer)

        self.buffer_ripple()
        self.buffer.clear()
        self.buffer.extend(" " * self.left_margin)
        self.cursor = len(self.buffer)
        self.margin_release = False
        self.align = None
        self.align_chars = []


    def send_to_printer(self, buffer):
        """ send buffer to the printer"""
        if self.printer_found:
            self.printer.print_line("".join(buffer), self.current_spacing,
                                    self.align)

    def margin_set_left(self):
        """ set the current cursor position as the left margin
        check that it is not further right than the right margin"""
        new_left = self.cursor
        if new_left >= self.right_margin:
            self.bell()
            return
        self.left_margin = new_left

    def margin_set_right(self):
        """ set the current cursor position as the right margin
        check that it is not further left than the left margin"""
        new_right = self.cursor
        if new_right <= self.left_margin or new_right > self.width:
            self.bell()
            return
        self.right_margin = new_right

    def toggle_margin_release(self):
        """ allow the cursor to move further than the margins"""
        self.margin_release = not self.margin_release

    @property
    def centre_mode(self):
        return self.align == "centre"

    @property
    def right_mode(self):
        return self.align == "right"

    def centre_text(self):
        """ toggle centre mode: typed text stays midway between the margins"""
        self._toggle_align("centre")

    def right_justify(self):
        """ toggle right justify: typed text stays flush with the right margin"""
        self._toggle_align("right")

    def _toggle_align(self, mode):
        """switch an alignment mode on or off

        any text already typed on the line is picked up and aligned. while
        the mode is on every character typed re-aligns the line. turning it
        off moves the text back to the left margin so you can carry on
        typing normally. a carriage return also ends it, leaving the line
        aligned. switching straight from one mode to the other re-aligns
        the text.
        """
        if self.align == mode:
            text = "".join(self.align_chars)
            self.align = None
            self.align_chars = []
            self.buffer.clear()
            self.buffer.extend(" " * self.left_margin + text)
            self.cursor = len(self.buffer)
            return

        if self.align is None:
            self.align_chars = list("".join(self.buffer).strip())
        self.align = mode
        self._layout_align()

    def _align_span(self):
        """number of columns available between the margins"""
        return self.right_margin - self.left_margin

    def align_add(self, key):
        """add a character to the aligned text, ding if it will not fit"""
        if len(self.align_chars) >= self._align_span():
            self.bell()
            return
        self.align_chars.append(key)
        self._layout_align()

    def _layout_align(self):
        """rebuild the buffer so the text sits centred or against the
        right margin"""
        text = "".join(self.align_chars)
        gap = self._align_span() - len(text)
        if self.align == "centre":
            start = self.left_margin + gap // 2
        else:
            start = self.left_margin + gap
        self.buffer.clear()
        self.buffer.extend(" " * start + text)
        self.cursor = len(self.buffer)


    def toggle_printer(self):
        """pause or resume sending lines to the printer. lines typed while
        paused still go to the file but are never sent to the printer"""
        self.use_printer = not self.use_printer

    def toggle_help(self):
        self.help_wanted = not self.help_wanted

# ----------------------------
# start of non class functions
# ----------------------------

def short_path(text, width):
    """a path cut down to fit in width columns for the info panel: the home
    folder becomes ~, then if it's still too long the start is replaced
    with ... (at a folder boundary where possible) so the file name shows"""
    home = path.expanduser("~")
    if text == home or text.startswith(home + "/"):
        text = "~" + text[len(home):]
    if len(text) <= width:
        return text
    if width <= 3:
        return text[-width:] if width > 0 else ""
    tail = text[-(width - 3):]
    slash = tail.find("/")
    if slash > 0:
        tail = tail[slash:]
    return "..." + tail


def screen_width(stdscr):
    """columns that can be written without wrapping onto the next line"""
    return stdscr.getmaxyx()[1] - 1


def show_help(stdscr, machine):
    help_row = 11
    folder = short_path(machine.save_folder, screen_width(stdscr) - len("file path:"))
    help_text = f"""arrows/home/end move, del deletes, alt+backspace deletes a word. alt+ below:
l = set left margin  r = set right margin       s = toggle line spacing
a = set autoreturn   n = toggle margin release  q = quit  p = printer on/off
c = centre text      j = right justify (enter to finish)  b = bell on/off
file path:{folder}"""
    # no newline at the end: on a 16 line terminal this is the bottom row,
    # and curses gives an error if the cursor goes off the screen
    if (machine.help_wanted):
        stdscr.addstr(help_row, 0, help_text, curses.color_pair(5))

def show_handy_settings(stdscr, machine):
    handy_row = 6
    stdscr.erase()
    stdscr.addstr(handy_row, 0, f"Width={machine.width}", curses.color_pair(5))
    stdscr.addstr(handy_row, 10, f"LS={machine.current_spacing}", curses.color_pair(5))
    stdscr.addstr(handy_row, 17, f"A-RTN={machine.autoreturn}", curses.color_pair(5))
    stdscr.addstr(handy_row, 30, f"LM={machine.left_margin}", curses.color_pair(5))
    stdscr.addstr(handy_row, 36, f"RM={machine.right_margin}", curses.color_pair(5))
    if machine.centre_mode:
        stdscr.addstr(handy_row, 44, "CENTRE", curses.color_pair(2))
    elif machine.right_mode:
        stdscr.addstr(handy_row, 44, "RIGHT", curses.color_pair(2))
    file_name = short_path(machine.file_name, screen_width(stdscr) - len("File="))
    stdscr.addstr((handy_row +1), 0, f"File={file_name}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 0, f"Printer Found={machine.printer_found}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 20, f"LC={machine.line_count}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 27, f"WC={machine.word_count}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 37, f"File={machine.use_file}", curses.color_pair(2))
    stdscr.addstr((handy_row + 3), 0, f"Printer={machine.printer_name}", curses.color_pair(5))
    stdscr.addstr((handy_row + 3), 20, f"Printing={machine.use_printer}", curses.color_pair(2))
    stdscr.addstr((handy_row + 3), 37, f"Bell={machine.bell_on}", curses.color_pair(2))
    stdscr.addstr((handy_row + 4), 0, "alt-h for help", curses.color_pair(5))

def key_check(stdscr, key, machine):
    if 32 <= key <= 126:
        machine.char_add(chr(key))

    elif key in (curses.KEY_BACKSPACE, 127):
        machine.backspace()

    elif key in (curses.KEY_ENTER, 13, 10):
        machine.carriage_return()

    elif key == curses.KEY_LEFT:
        machine.cursor_left()

    elif key == curses.KEY_RIGHT:
        machine.cursor_right()

    elif key == curses.KEY_HOME:
        machine.cursor_home()

    elif key == curses.KEY_END:
        machine.cursor_end()

    elif key == curses.KEY_DC:
        machine.delete()

    elif key == 27:
        # alt+key arrives as esc followed by the key. a lone esc gives -1
        # here and matches nothing below
        stdscr.nodelay(True)
        key2 = stdscr.getch()
        stdscr.nodelay(False)

        match(key2):
            case 97: # alt+a toggle autoreturn
                machine.toggle_autoreturn()
            case 99: # alt+c toggle centre text
                machine.centre_text()
            case 127 | 8 | curses.KEY_BACKSPACE: # alt+backspace delete word
                machine.delete_word()
            case 98: # alt+b bell on / off
                machine.toggle_bell()
                save_settings_to_config({"bell": machine.bell_on})
            case 112: # alt+p pause / resume printing
                machine.toggle_printer()
            case 106: # alt+j toggle right justify
                machine.right_justify()
            case 104: # alt+h toggle help
                machine.toggle_help()
            case 108: # alt+l set left margin
                machine.margin_set_left()
                save_margins_to_config(machine.left_margin, machine.right_margin)
            case 110: # alt+n toggle margin release
                machine.toggle_margin_release()
            case 113: #alt+q pressed so quit
                quit()
            case 114: #alt+r set right margin
                machine.margin_set_right()
                save_margins_to_config(machine.left_margin, machine.right_margin)
            case 115: # alt-s line spacing toggle
                machine.toggle_line_space()
                save_settings_to_config({"spacing_index": machine.spacing_index})

def start_screen(stdscr):
    stdscr.addstr("Typewriter starting up, press any key to go", curses.color_pair(2))
    stdscr.refresh()
    stdscr.getch()
    stdscr.erase()

def show_buffers(stdscr, machine):
    stdscr.addstr(4, 0, machine.give_tab_bar(), curses.color_pair(5))
    stdscr.addstr(0, 0, machine.give_prev_buff_3(), curses.color_pair(5))
    stdscr.clrtoeol()
    stdscr.addstr(1, 0, machine.give_prev_buff_2(), curses.color_pair(5))
    stdscr.clrtoeol()
    stdscr.addstr(2, 0, machine.give_prev_buff_1(), curses.color_pair(5))
    stdscr.clrtoeol()
    stdscr.addstr(3, 0, machine.give_buffer(), curses.color_pair(1))
    # put the screen cursor where the machine says the next character goes
    stdscr.move(3, machine.cursor)

# terminal colour number for highlighted settings (0-255). 109 is a soft
# grey-teal; try 144 (khaki), 146 (lavender grey) or 248 (light grey)
HIGHLIGHT_COLOUR = 109


def setup_curses(stdscr):
    curses.start_color()
    curses.use_default_colors()
    curses.curs_set(1)
    curses.noecho()
    curses.init_pair(1, curses.COLOR_GREEN, -1)
    # pair 2 marks settings and modes (File=, Printing=, Bell=, CENTRE...).
    # a soft colour so it's noticeable without being distracting
    highlight = HIGHLIGHT_COLOUR if curses.COLORS >= 256 else curses.COLOR_CYAN
    curses.init_pair(2, highlight, -1)
    curses.init_pair(3, curses.COLOR_WHITE, -1)
    curses.init_pair(4, curses.COLOR_CYAN, -1)
    curses.init_pair(5, 241, -1)
    stdscr.erase()

def main(stdscr, machine):
    setup_curses(stdscr)
    start_screen(stdscr)

    while True:
        show_handy_settings(stdscr, machine)
        if (machine.help_wanted):
            show_help(stdscr, machine)

        show_buffers(stdscr, machine)

        try:
            key = stdscr.getch()
        except:
            continue

        key_check(stdscr, key, machine)

        stdscr.refresh()


def file_arg_path(name):
    """where --file NAME writes. a plain name goes in save_folder (it's
    returned as it is, to be joined on later); a path with a folder in it,
    like ./test.txt, is taken from the current folder, so it's made into a
    full path now"""
    if "/" in name:
        return path.abspath(name)
    return name


def parse_args(args=None):
    """command line options. anything given here overrides config.toml"""
    ap = argparse.ArgumentParser(description="typewriter for a line printer")
    ap.add_argument("use_file", nargs="?", choices=("True", "False"),
                    help="save typed lines to a file (overrides use_file)")
    ap.add_argument("-p", "--printer", choices=("epson", "zebra"),
                    help="printer to use (overrides printer in config.toml)")
    ap.add_argument("-f", "--file", metavar="NAME",
                    help="file to write to instead of a dated one. a plain name "
                    "goes in save_folder; a path (./test.txt, ~/notes.txt) is "
                    "taken from the current folder. lines are added to the end "
                    "if it already exists. turns on saving to a file")
    parsed = ap.parse_args(args)
    if parsed.file is not None and parsed.use_file == "False":
        ap.error("--file and False don't go together: False turns off the file")
    return parsed


def run():
    """load the config, build the machine and start the curses interface"""
    args = parse_args()
    logging.basicConfig(filename='type.log', encoding='utf-8', level=logging.INFO)
    logging.debug("starting...")
    term_col, term_row = get_terminal_size()
    if (term_row < 16) or (term_col < 81):
        print("terminal is too small please make it at least 16 lines by 81 columns")
        quit()

    # use the toml config file, returns it as a dict
    with open(CONFIG_PATH, mode="rb") as fp:
        config = tomllib.load(fp)

    if args.printer:
        config['printer'] = args.printer

    use_printer = config.get('use_printer', True)
    machine = Typewriter(
        width=config['width'],
        autoreturn=config['autoreturn'],
        spacing_index=config['spacing_index'],
        margin_bell=config['margin_bell'],
        left_margin=config['left_margin'],
        right_margin=config.get('right_margin', config['width']),
        use_file=config.get('use_file', True),
        use_printer=use_printer,
        # always open the printer so alt-p can turn printing on even if
        # use_printer starts off false in the config
        printer=open_printer(config),
        printer_name=printer_name(config),
        bell=curses.beep,
        ding=sound_player(config.get('bell_sound', 'ding.wav'), curses.beep),
        bell_on=config.get('bell', True))

    machine.save_folder = config['save_folder']
    if args.use_file is not None:
        machine.use_file = args.use_file == "True"
    if args.file is not None:
        machine.file_name = file_arg_path(args.file)
        machine.use_file = True

    # curses delays the esc key in case you are about to
    # use it for an esc sequence the line below calls
    # a function that sets an env variable to adjust the delay
    set_shorter_esc_delay_in_os('150')
    wrapper(main, machine)


if __name__ == "__main__":
    run()


# TODO tabs: alt+tab to jump to the next tab stop (or right margin if none),
#      set a tab stop at the cursor, clear all tabs but not margins
# TODO line feed: move the carriage down without a carriage return
# TODO underline text: underline everything including spaces, or words only
# TODO file handling to rename file
# TODO command line startup options to set attributes without having
#      to edit the script directly
