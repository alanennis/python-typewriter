import curses
from curses import wrapper
from os import environ, get_terminal_size, path
from time import strftime
from escpos.printer import Usb
#from escpos import POSPrinter
import tomllib
import logging
import re
from pathlib import Path
from sys import argv

CONFIG_PATH = Path(__file__).resolve().parent / "config.toml"



def save_margins_to_config(left_margin, right_margin, config_path=CONFIG_PATH):
    """write the margin values back to the config file, leaving the
    rest of the file (including comments) untouched"""
    text = config_path.read_text()
    for key, value in (("left_margin", left_margin), ("right_margin", right_margin)):
        pattern = rf"^({key}\s*=\s*)\d+"
        if re.search(pattern, text, flags=re.MULTILINE):
            text = re.sub(pattern, rf"\g<1>{value}", text, count=1, flags=re.MULTILINE)
        else:
            if not text.endswith("\n"):
                text += "\n"
            text += f"{key} = {value}\n"
    config_path.write_text(text)


def set_shorter_esc_delay_in_os(delay_ms):
    environ.setdefault('ESCDELAY', delay_ms)

class Typewriter():
    def __init__(self, width = 75, spacing_index = 1, 
                autoreturn=False, margin_bell=8,
                left_margin = 0, right_margin = None,
                use_file = True, use_printer = True,
                printer_vendor_id = None, printer_product_id = None,
                bell = None):
        # bell is called whenever the machine wants to ding. the class knows
        # nothing about the screen, so the caller decides what a ding is
        # (curses.beep in the real program, a no-op by default)
        self.bell = bell if bell is not None else (lambda: None)
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
        self.tab_bar = []
        self.tab_bar.extend('.' * self.width)
        self.margin_release = False
        self.tab_bar[self.left_margin] = "!"
        self.tab_bar[self._right_marker_index()] = "!"
        self.prev_buf_1 = ''
        self.prev_buf_2 = ''
        self.prev_buf_3 = ''
        self.today = strftime("%Y-%m-%d-%H%M%S")
        self.file_name = f"{self.today}.txt"
        self.use_file = use_file
        self.use_printer = use_printer
        self.margin_hot_zone = False
        self.help_wanted = False
        self.centre_mode = False
        self.centre_chars = []
        self.word_count = 0
        self.line_count = 0
        self.save_folder = "./"
        self.buffer.extend(" " * self.left_margin)
        # column the next character will be typed at
        self.cursor = len(self.buffer)
        self.printer_found = False
        if printer_vendor_id is not None and printer_product_id is not None:
            try:
                self.p = Usb(printer_vendor_id, printer_product_id, 0)
                self.printer_found = True
            except:
                self.printer_found = False


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
    
    def give_tab_bar(self):
        """ return the current contents of tab bar as string"""
        return ("".join(self.tab_bar))

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
        if self.centre_mode:
            self.centre_add(key)
            return
        # if the buffer is at the margin bell point then 
        # make the bell ring and set the hot zone flag
        if (len(self.buffer) == (self.right_margin - self.margin_bell)):
            self.bell()
        self.margin_hot_zone = len(self.buffer) >= (self.right_margin - self.margin_bell)

        # if the buffer is less than the right margin but not longer
        # than the printable width then add character to the buffer
        fits = ((len(self.buffer) < self.right_margin or self.margin_release)
                and (len(self.buffer) < self.width - 1))
        if fits:
            self.buffer.insert(self.cursor, key)
            self.cursor += 1

        # if the key is in the hot zone and autoreturn is on and
        # you hit space or hypen or reach the end of line
        # then you will get an autoreturn
        # if you reach the end of the line and auto is not on then just ding
        if ((self.margin_hot_zone and key in (" ", "-")) or not fits):
            if self.autoreturn:
                self.carriage_return()
            else:
                self.bell()
                

    def backspace(self):
        """move the cursor back one space and delete the char before it"""
        if self.centre_mode:
            if self.centre_chars:
                self.centre_chars.pop()
                self._layout_centre()
            return

        if (len(self.buffer) > 0 and self.cursor > 0):
            logging.debug(f'length is {len(self.buffer)}')
            logging.debug(f'position is {self.cursor}')
            self.buffer.pop(self.cursor - 1)
            self.cursor -= 1
    
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
        self.centre_mode = False
        self.centre_chars = []
        

    def send_to_printer(self, buffer):
        """ send buffer to the printer"""
        if self.current_spacing == 1: self.raw_spacing = 30
        if self.current_spacing == 1.5: self.raw_spacing = 40
        if self.current_spacing == 2: self.raw_spacing = 60

        if self.printer_found:
            self.p.line_spacing(self.raw_spacing)
            self.printer_line = "".join(buffer)
            self.p.text(self.printer_line)
            self.p.text("\n")

    def tab(self):
        """send cursor to the next tab or to the right margin if no tabs"""
        pass

    def tab_clear(self):
        """ clear all tabs but not margins"""
        pass

    def tab_set(self):
        """add thius cursor position to the tab stop locations"""
        pass

    def line_feed(self):
        """ just move the carriage down the current line spacing amount"""
        pass

    def margin_set_left(self):
        """ set the current cursor position as the left margin
        check that it is not further right than the right margin"""
        new_left = self.cursor
        if new_left >= self.right_margin:
            self.bell()
            return
        self.tab_bar[self.left_margin] = "."
        self.left_margin = new_left
        self.tab_bar[self.left_margin] = "!"

    def margin_set_right(self):
        """ set the current cursor position as the right margin
        check that it is not further left than the left margin"""
        new_right = self.cursor
        if new_right <= self.left_margin or new_right > self.width:
            self.bell()
            return
        self.tab_bar[self._right_marker_index()] = "."
        self.right_margin = new_right
        self.tab_bar[self._right_marker_index()] = "!"
    
    def toggle_margin_release(self):
        """ allow the cursor to move further than the margins"""
        # toggle the margin release
        self.margin_release = not self.margin_release

    def underline_all_toggle(self):
        """ enable underline an all chars including spaces but not tabs"""
        pass

    def underline_word_toggle(self):
        """ enable underline an all characters but not spaces"""
        pass

    def centre_text(self):
        """ move cursor to the centre of the buffer and centre align typed chars

        toggles centre mode. any text already typed on the line is picked up
        and centred. while in centre mode every character typed re-centres
        the line between the margins. a carriage return ends centre mode.
        """
        if self.centre_mode:
            # leave the centred text where it is and carry on typing normally
            self.centre_mode = False
            self.centre_chars = []
            return

        self.centre_mode = True
        self.centre_chars = list("".join(self.buffer).strip())
        self._layout_centre()

    def _centre_span(self):
        """number of columns available between the margins"""
        return self.right_margin - self.left_margin

    def centre_add(self, key):
        """add a character to the centred text, ding if it will not fit"""
        if len(self.centre_chars) >= self._centre_span():
            self.bell()
            return
        self.centre_chars.append(key)
        self._layout_centre()

    def _layout_centre(self):
        """rebuild the buffer so the centre text sits midway between margins"""
        text = "".join(self.centre_chars)
        start = self.left_margin + (self._centre_span() - len(text)) // 2
        self.buffer.clear()
        self.buffer.extend(" " * start + text)
        self.cursor = len(self.buffer)

    def right_margin_flush(self):
        """ move cursor to right margin and print text flush with right margin"""
    
    def toggle_help(self):
        self.help_wanted = not self.help_wanted

# ----------------------------
# start of non class functions
# ----------------------------

def show_help(stdscr, machine):
    help_row = 11
    # stdscr.erase()
    help_text = f"""alt + letter below:
l = set left margin  r = set right margin       s = toggle line spacing
a = set autoreturn   n = toggle margin release  q = quit
c = centre text (enter to finish)
file path:{machine.save_folder}
"""
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
    stdscr.addstr((handy_row +1), 0, f"File={machine.file_name}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 0, f"Printer Found={machine.printer_found}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 20, f"LC={machine.line_count}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 27, f"WC={machine.word_count}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 37, f"File={machine.use_file}", curses.color_pair(2))
    stdscr.addstr((handy_row + 4), 0, "alt-h for help", curses.color_pair(5))

def display(machine):
    print(machine)

def key_check(stdscr, key, machine):
    if 32 <= key <= 126:
        machine.char_add(chr(key))

    elif key in (curses.KEY_BACKSPACE, '\b', '\x7f', 127, 263):
        machine.backspace()

    elif key in (curses.KEY_ENTER, 13, 10):
        machine.carriage_return()

    elif key in (curses.KEY_LEFT, 260):
        pass

    elif key == curses.KEY_RIGHT:
        pass
    
    elif key == curses.KEY_UP:
        pass
    elif key == curses.KEY_DOWN:
        pass

    elif key == 27:
        # curses.beep()
        stdscr.nodelay(True)
        key2 = stdscr.getch() # get the key pressed after ALT
        stdscr.nodelay(False)
        if key2 == -1: # if there is no key after alt then esc was pressed
            # quit()
            pass
        
        match(key2):
            case 9: # tab pressed
                pass
            case 97: # alt+a toggle autoreturn
                # stdscr.addstr(16, 0, "you pressed alt+a")
                machine.toggle_autoreturn()
            case 98:  # alt+b
                # stdscr.addstr(16, 0, "you pressed alt+b")
                pass
            case 99: # alt+c toggle centre text
                machine.centre_text()
            case 104: # alt+h toggle help
                # invert the help wanted bool   
                machine.toggle_help()
                pass
            case 108: # alt+l set left margin
                machine.margin_set_left()
                save_margins_to_config(machine.left_margin, machine.right_margin)
            case 110:
                machine.toggle_margin_release()
            case 113: #alt+q pressed so quit
                quit()
            case 114: #alt+r set right margin
                machine.margin_set_right()
                save_margins_to_config(machine.left_margin, machine.right_margin)
            case 115: # alt-s line spacing toggle
                machine.toggle_line_space()
                
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

def setup_curses(stdscr):
    curses.start_color()
    curses.use_default_colors()
    curses.curs_set(1)
    curses.noecho()
    curses.init_pair(1, curses.COLOR_GREEN, -1)
    curses.init_pair(2, curses.COLOR_RED, -1)
    curses.init_pair(3, curses.COLOR_WHITE, -1)
    curses.init_pair(4, curses.COLOR_CYAN, -1)
    curses.init_pair(5, 241, -1)
    stdscr.erase()

def show_cursor(stdscr):
    # stdscr.move(5,0)
    sh_cursy, sh_cursx = stdscr.getyx()
    if (sh_cursx == 0):
        stdscr.addstr(5, sh_cursx, "^")
    else:
        # sh_cursx = sh_cursx - 1
        stdscr.addstr(5, sh_cursx - 1, "^")
    # stdscr.clrtoeol()
   
def main(stdscr, machine):
  

    setup_curses(stdscr)
    start_screen(stdscr)

    while True:
        
        show_handy_settings(stdscr, machine)
        if (machine.help_wanted):
            show_help(stdscr, machine)

        show_buffers(stdscr, machine)
        # show_cursor(stdscr)

        try:
            key = stdscr.getch()
        except: 
            continue
        
        key_check(stdscr, key, machine)

        stdscr.refresh()
    
    # start_screen(stdscr)


def run():
    """load the config, build the machine and start the curses interface"""
    logging.basicConfig(filename='type.log', encoding='utf-8', level=logging.INFO)
    logging.debug("starting...")
    term_col, term_row = get_terminal_size()
    if (term_row < 16) or (term_col < 81):
        print("terminal is too small please make it at least 16 lines by 81 columns")
        quit()
    
    # use the toml config file, returns it as a dict
    with open(CONFIG_PATH, mode="rb") as fp:
        config = tomllib.load(fp)

    machine = Typewriter(
        width=config['width'],
        autoreturn=config['autoreturn'],
        spacing_index=config['spacing_index'],
        margin_bell=config['margin_bell'],
        left_margin=config['left_margin'],
        right_margin=config.get('right_margin', config['width']),
        use_file=config.get('use_file', True),
        use_printer=config.get('use_printer', True),
        printer_vendor_id=config.get('printer_vendor_id'),
        printer_product_id=config.get('printer_product_id'),
        bell=curses.beep)
    
    machine.save_folder = config['save_folder']
    if (len(argv) == 2):
        if (argv[1] == "True"):
            machine.use_file = True
        else:
            machine.use_file = False

    # curses delays the esc key in case you are about to
    # use it for an esc sequence the line below calls 
    # a function that sets an env variable to adjust the delay
    set_shorter_esc_delay_in_os('150')  
    wrapper(main, machine)


if __name__ == "__main__":
    run()


# TODO in line editing of line before sending to the printer/file
# TODO tabs
# TODO file handling to rename file 
# TODO command line startup options to set attributes without having
# to edit the script directly
# TODO convert this to rust as a project and so that it is BLAZING fast
# TODO centering text
# TODO underline text
# TODO right margin flush

