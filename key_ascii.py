import curses
from os import environ

def set_shorter_esc_delay_in_os(delay_ms):
    environ.setdefault('ESCDELAY', delay_ms)

def Main(screen):
    while True:
        ch = screen.getch()
        if ch == ord('q'):
            print("escape out")
            # curses.beep()
            curses.napms(1000)
            break
        
        # elif ch == 27:
        #     print("escape out")
        #     curses.beep()
        #     curses.napms(1000)
        #     break

        elif ch == 27: # ALT was pressed
            # curses.beep()
            screen.nodelay(True)
            ch2 = screen.getch() # get the key pressed after ALT
            if ch2 == -1:
                break
            else:
                if ch2 == 97:
                    screen.addstr(4, 0, "you pressed alt+a")
                    
                screen.erase()
                screen.addstr(5, 5, 'ALT+'+str(ch2))
                screen.refresh()
                screen.nodelay(False)
        else:
            screen.erase()
            screen.addstr(5, 5, str(ch))


set_shorter_esc_delay_in_os('15') 
curses.wrapper(Main)