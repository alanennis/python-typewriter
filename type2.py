import curses
from curses import wrapper
from os import environ
from time import strftime
from escpos.printer import Usb



def set_shorter_esc_delay_in_os(delay_ms):
    environ.setdefault('ESCDELAY', delay_ms)

class Typewriter():
    def __init__(self, width = 80, spacing_index = 1, 
                autoreturn=False, margin_bell=8,
                left_margin = 0, margin_release = False):
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
        self.margin_release = margin_release
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
        try:
            self.p = Usb(0x04b8, 0x0047, 0)
            self.printer_found = True
        except:
            self.printer_found = False
        # self.p.text("printer online \n")


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
        # test_buffer = ["1", "b", "house"]
        # return_string = "".join(test_buffer)
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
        # you hit space or hypen then you will get an autoreturn
        if (self.margin_hot_zone and self.autoreturn and key in (" ", "-")):
            self.carriage_return(stdscr)

    def backspace(self, stdscr, pos):
        """move the cursor back one space and delete last char in buffer"""
        # handle moving the print head cursor back one space
        # should this be in its own def though?
        self.clear_line_start(stdscr)

        if (len(self.buffer) > 0):
            self.buffer.pop(pos -1)
    
    def buffer_ripple(self):
        self.prev_buf_3 = self.prev_buf_2
        self.prev_buf_2 = self.prev_buf_1
        self.prev_buf_1 = self.give_buffer()


    def carriage_return(self, stdscr):
        """record a carriage return and then depending on return mode
        do what needs to be done
        """
        self.margin_hot_zone = False
        #send contents of line to file if using file
        if (self.use_file):
            self.line_for_file = "".join(self.buffer)
            with open(self.file_name, 'a') as self.f1:
                self.f1.write(self.line_for_file + "\n")

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
        if self.current_spacing == 1: self.raw_spacing = 30
        if self.current_spacing == 1.5: self.raw_spacing = 40
        if self.current_spacing == 2: self.raw_spacing = 60

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
        pass


# start of non class functions
def show_handy_settings(stdscr, machine):
    stdscr.erase()
    stdscr.addstr(10, 0, f"Width={my_machine.width}", curses.color_pair(5))
    stdscr.addstr(10, 10, f"LS={my_machine.current_spacing}", curses.color_pair(5))
    stdscr.addstr(10, 17, f"A-RTN={my_machine.autoreturn}", curses.color_pair(5))
    stdscr.addstr(10, 30, f"LM={my_machine.left_margin}", curses.color_pair(5))
    stdscr.addstr(10, 36, f"RM={my_machine.right_margin}", curses.color_pair(5))
    stdscr.addstr(11, 0, f"File={my_machine.file_name}", curses.color_pair(5))
    stdscr.addstr(12, 0, f"Printer Found={my_machine.printer_found}", curses.color_pair(5))
    stdscr.addstr(13, 0, "alt-h for help", curses.color_pair(5))

def display(screen, my_machine):
    print(my_machine)

def key_check(stdscr, key):
    if 32 <= key <= 126:
        _, cursy = stdscr.getyx()

        my_machine.char_add(stdscr, chr(key), cursy)
        # not sure if this will be kept, gives column number
        stdscr.erase()
        stdscr.addstr(15,0, str(cursy))
    
    elif key in (curses.KEY_BACKSPACE, '\b', '\x7f', 127, 263):
        _, cursy = stdscr.getyx()
        # stdscr.addstr(15, 0, str(cursy))
        my_machine.backspace(stdscr, cursy)
        # not sure if this will be kept, gives column number
        stdscr.erase()
        stdscr.addstr(15,0, str(cursy))


    elif key in (curses.KEY_ENTER, 13, 10):
        my_machine.carriage_return(stdscr)

    elif key == curses.KEY_LEFT:
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
            quit()
        
        match(key2):
            case 97: # alt+a
                # stdscr.addstr(16, 0, "you pressed alt+a")
                my_machine.toggle_autoreturn()
            case 98:  # alt+b
                # stdscr.addstr(16, 0, "you pressed alt+b")
                pass
            case 108: # alt+l
                my_machine.margin_set_left(stdscr)
                stdscr.erase()
                # stdscr.addstr(16, 0, "you pressed alt+l")
            case 110:
                my_machine.toggle_margin_release()
            case 114: #alt+r
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
    curses.init_pair(1, curses.COLOR_GREEN, curses.COLOR_BLACK)
    curses.init_pair(2, curses.COLOR_RED, curses.COLOR_BLACK)
    curses.init_pair(3, curses.COLOR_WHITE, curses.COLOR_BLACK)
    curses.init_pair(4, curses.COLOR_CYAN, curses.COLOR_BLACK)
    curses.init_pair(5, 241, -1)
    stdscr.erase()
    

def main(stdscr, my_machine):
    setup_curses(stdscr)
    start_screen(stdscr)

    while True:
        
        show_handy_settings(stdscr, my_machine)
        show_buffers(stdscr, my_machine)
  
        try:
            key = stdscr.getch()
        except: 
            continue
        
        key_check(stdscr, key)

        stdscr.refresh()
    
    # start_screen(stdscr)


if __name__ == "__main__":
    my_machine = Typewriter()
    print(my_machine)  

    # curses delays the esc key in case you are about to
    # use it for an esc sequence the line below calls 
    # a function that sets an env variable to adjust the delay
    set_shorter_esc_delay_in_os('150')  
    wrapper(main, my_machine)


# TODO the help screen with the command shorcuts
# TODO in line editing of line before sending to the printer/file
# TODO tabs
# TODO file handling to rename file 
# TODO command line startup options to set attributes without having
# to edit the script directly
# TODO convert this to rust as a project and so that it is BLAZING fast
# TODO centering text
# TODO underline text
# TODO right margin flush

