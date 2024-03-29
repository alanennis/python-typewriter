import curses
from curses import wrapper
from os import environ, get_terminal_size
from time import strftime
from escpos.printer import Usb
import tomllib
import logging
from sys import argv



def set_shorter_esc_delay_in_os(delay_ms):
    environ.setdefault('ESCDELAY', delay_ms)

class Typewriter():
    def __init__(self, width = 75, spacing_index = 1, 
                autoreturn=False, margin_bell=8,
                left_margin = 0):
        self.width = width
        self.autoreturn = autoreturn
        self.spacing_choices = (1, 1.5, 2)
        self.spacing_index = spacing_index
        self.current_spacing = self.spacing_choices[self.spacing_index]
        self.buffer = []
        self.margin_bell = margin_bell
        self.right_margin = width
        self.left_margin = left_margin
        self.tab_bar = []
        self.tab_bar.extend('.' * self.width)
        self.margin_release = False
        self.tab_bar[self.left_margin] = "!"
        self.tab_bar[self.right_margin - 1] = "!"
        self.prev_buf_1 = ''
        self.prev_buf_2 = ''
        self.prev_buf_3 = ''
        self.today = strftime("%Y-%m-%d-%H%M%S")
        self.file_name = f"{self.today}.txt"
        self.use_file = True
        self.use_printer = True
        self.margin_hot_zone = False
        self.help_wanted = False
        self.word_count = 0
        self.line_count = 0
        self.save_folder = "./"
        try:
            self.p = Usb(0x04b8, 0x0047, 0)
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

    def char_add(self, stdscr, key, pos):
        """add a character to buffer but check margins etc"""
        logging.debug(f"buff len = {len(self.buffer)}")
        logging.debug(f"autortn = {self.autoreturn}")
        # if the buffer is at the margin bell point then 
        # make the bell ring and set the hot zone flag
        if (len(self.buffer) == (self.right_margin - self.margin_bell)):
            curses.beep()
            self.margin_hot_zone = not self.margin_hot_zone

        # if the buffer is less than the right margin but not longer
        # than the printable width then add character to the buffer
        if ((len(self.buffer) < self.right_margin or self.margin_release) and (len(self.buffer) < self.width - 1)):
            # if (len(self.buffer) < self.width):
            self.buffer.insert(pos, key)
        
        # if the key is in the hot zone and autoreturn is on and 
        # you hit space or hypen or reach the end of line 
        # then you will get an autoreturn
        # if you reach the end of the line and auto is not on then just ding
        if (self.margin_hot_zone and key in (" ", "-") or pos == len(self.buffer)):
            if self.autoreturn:
              self.carriage_return(stdscr)
            else:
                curses.beep()
                

    def backspace(self, stdscr, pos):
        """move the cursor back one space and delete last char in buffer"""
        # handle moving the print head cursor back one space
        # should this be in its own def though?
        self.clear_line_start(stdscr)

        if (len(self.buffer) > 0):
            logging.debug(f'length is {len(self.buffer)}')
            logging.debug(f'position is {pos}')
            self.buffer.pop(pos - 1)
    
    def buffer_ripple(self):
        """ bump all the buffers contents up the chain"""
        self.prev_buf_3 = self.prev_buf_2
        self.prev_buf_2 = self.prev_buf_1
        self.prev_buf_1 = self.give_buffer()


    def carriage_return(self, stdscr):
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
            try:
                with open(self.save_folder + self.file_name, 'a') as self.f1:
                    self.f1.write(self.line_for_file + "\n")
            except:
                print("problem with writing to the file")
                quit()

        #send contents of line to printer
        if (self.use_printer and self.printer_found):
            self.send_to_printer(self.buffer)

        # stdscr.addstr(10,0, "send to printer")
        self.buffer_ripple()
        self.buffer.clear()
        self.buffer.extend(" " * self.left_margin)
        self.clear_line_start(  stdscr)
        self.margin_release = False
        

    def clear_line_start(self, stdscr):
        """clear the cursor and text back to line start"""
        stdscr.move(3, 0)
        stdscr.clrtoeol()

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

    def margin_set_left(self, stdscr):
        """ set the current cursor position as the left margin
        check that it is not further right than the right margin"""
        
        self.tab_bar[self.left_margin] = "."
        self.left_margin = stdscr.getyx()[1]
        self.tab_bar[self.left_margin] = "!"
        # self.left_margin = 10
        stdscr.refresh()

    def margin_set_right(self, stdscr):
        """ set the current cursor position as the right margin
        check that it is not further left than the left margin"""
        if (self.right_margin >= self.width):
            self.tab_bar[self.right_margin - 1] = "."
        else:        
            self.tab_bar[self.right_margin] = "."
        
        self.right_margin = stdscr.getyx()[1]
        self.tab_bar[self.right_margin] = "!"
        # self.left_margin = 10
        stdscr.refresh()
    
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
        """ move cursor to the centre of the buffer and centre align typed chars"""
        pass

    def right_margin_flush(self):
        """ move cursor to right margin and print text flush with right margin"""
    
    def toggle_help(self):
        self.help_wanted = not self.help_wanted

# ----------------------------
# start of non class functions
# ----------------------------

def show_help(stdscr):
    help_row = 11
    # stdscr.erase()
    help_text = """alt + letter below:
l = set left margin  r = set right margin       s = toggle line spacing
a = set autoreturn   n = toggle margin release  q = quit
"""
    if (my_machine.help_wanted):
        stdscr.addstr(help_row, 0, f"{help_text}", curses.color_pair(5))

def show_handy_settings(stdscr, machine):
    handy_row = 6
    stdscr.erase()
    stdscr.addstr(handy_row, 0, f"Width={my_machine.width}", curses.color_pair(5))
    stdscr.addstr(handy_row, 10, f"LS={my_machine.current_spacing}", curses.color_pair(5))
    stdscr.addstr(handy_row, 17, f"A-RTN={my_machine.autoreturn}", curses.color_pair(5))
    stdscr.addstr(handy_row, 30, f"LM={my_machine.left_margin}", curses.color_pair(5))
    stdscr.addstr(handy_row, 36, f"RM={my_machine.right_margin}", curses.color_pair(5))
    stdscr.addstr((handy_row +1), 0, f"File={my_machine.file_name}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 0, f"Printer Found={my_machine.printer_found}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 20, f"LC={my_machine.line_count}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 27, f"WC={my_machine.word_count}", curses.color_pair(5))
    stdscr.addstr((handy_row + 2), 37, f"File={my_machine.use_file}", curses.color_pair(2))
    stdscr.addstr((handy_row + 4), 0, "alt-h for help", curses.color_pair(5))

def display(screen, my_machine):
    print(my_machine)

def key_check(stdscr, key):
    if 32 <= key <= 126:
        _, cursy = stdscr.getyx()

        my_machine.char_add(stdscr, chr(key), cursy)
        # not sure if this will be kept, gives column number
        stdscr.erase()
        # stdscr.addstr(15,0, str(cursy))
    
    elif key in (curses.KEY_BACKSPACE, '\b', '\x7f', 127, 263):
        _, cursx = stdscr.getyx()
        logging.debug(f'cursor is at {cursx}')
        # stdscr.addstr(15, 0, str(cursy))
        my_machine.backspace(stdscr, cursx)
        # not sure if this will be kept, gives column number
        stdscr.erase()
        # stdscr.addstr(15,0, str(cursy))


    elif key in (curses.KEY_ENTER, 13, 10):
        my_machine.carriage_return(stdscr)

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
        if key2 == -1: # if there is no key after alt then esc was pressed
            # quit()
            pass
        
        match(key2):
            case 9: # tab pressed
                pass
            case 97: # alt+a toggle autoreturn
                # stdscr.addstr(16, 0, "you pressed alt+a")
                my_machine.toggle_autoreturn()
            case 98:  # alt+b
                # stdscr.addstr(16, 0, "you pressed alt+b")
                pass
            case 104: # alt+h toggle help
                # invert the help wanted bool   
                my_machine.toggle_help()
                pass
            case 108: # alt+l set left margin
                my_machine.margin_set_left(stdscr)
                stdscr.erase()
                # stdscr.addstr(16, 0, "you pressed alt+l")
            case 110:
                my_machine.toggle_margin_release()
            case 113: #alt+q pressed so quit
                quit()
            case 114: #alt+r set right margin
                my_machine.margin_set_right(stdscr)
            case 115: # alt-s line spacing toggle
                my_machine.toggle_line_space()
                
def start_screen(stdscr):
    stdscr.addstr("Typewriter starting up, press any key to go", curses.color_pair(2))
    stdscr.refresh()
    stdscr.getch()
    stdscr.erase()  

def show_buffers(stdscr, my_machine):
    stdscr.addstr(4, 0, my_machine.give_tab_bar(), curses.color_pair(5))
    stdscr.addstr(0, 0, my_machine.give_prev_buff_3(), curses.color_pair(5))
    stdscr.clrtoeol()
    stdscr.addstr(1, 0, my_machine.give_prev_buff_2(), curses.color_pair(5))
    stdscr.clrtoeol()
    stdscr.addstr(2, 0, my_machine.give_prev_buff_1(), curses.color_pair(5))
    stdscr.clrtoeol()
    stdscr.addstr(3, 0, my_machine.give_buffer(), curses.color_pair(1))

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
   
def main(stdscr, my_machine):
  

    setup_curses(stdscr)
    start_screen(stdscr)

    while True:
        
        show_handy_settings(stdscr, my_machine)
        if (my_machine.help_wanted):
            show_help(stdscr)

        show_buffers(stdscr, my_machine)
        # show_cursor(stdscr)

        try:
            key = stdscr.getch()
        except: 
            continue
        
        key_check(stdscr, key)

        stdscr.refresh()
    
    # start_screen(stdscr)


if __name__ == "__main__":
    logging.basicConfig(filename='type.log', encoding='utf-8', level=logging.INFO)
    logging.debug("starting...")
    term_col, term_row = get_terminal_size()
    if (term_row < 15) or (term_col < 81):
        print("terminal is too small please make it at least 16 lines by 81 columns")
        quit()
    
    # use the toml config file, returns it as a dict
    with open("./config.toml", mode="rb") as fp:
        config = tomllib.load(fp)

    my_machine = Typewriter(
        width=config['width'],
        autoreturn=config['autoreturn'],
        spacing_index=config['spacing_index'],
        margin_bell=config['margin_bell'],
        left_margin=config['left_margin'])
    
    my_machine.save_folder = config['save_folder']
    if (len(argv) == 2):
        if (argv[1] == "True"):
            my_machine.use_file = True
        else:
            my_machine.use_file = False

    # curses delays the esc key in case you are about to
    # use it for an esc sequence the line below calls 
    # a function that sets an env variable to adjust the delay
    set_shorter_esc_delay_in_os('150')  
    curses,wrapper(main, my_machine)
    # wrapper(main, my_machine)


# TODO in line editing of line before sending to the printer/file
# TODO tabs
# TODO file handling to rename file 
# TODO command line startup options to set attributes without having
# to edit the script directly
# TODO convert this to rust as a project and so that it is BLAZING fast
# TODO centering text
# TODO underline text
# TODO right margin flush

