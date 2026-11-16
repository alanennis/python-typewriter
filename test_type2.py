"""tests for the Typewriter class and config helpers

run with:  venv/bin/python -m pytest
"""
import tomllib

import pytest

from zebra_lines import LEFT, line_zpl

from type2 import (Typewriter, open_printer, parse_args, printer_name,
                   save_margins_to_config, save_settings_to_config,
                   sound_player)


# ----------------------------
# helpers / fixtures
# ----------------------------

class Bell:
    """stand-in for curses.beep that just counts dings"""
    def __init__(self):
        self.count = 0

    def __call__(self):
        self.count += 1


@pytest.fixture
def bell():
    return Bell()


@pytest.fixture
def make(bell):
    """build a Typewriter with no file or printer output"""
    def _make(**kwargs):
        kwargs.setdefault("width", 75)
        kwargs.setdefault("use_file", False)
        kwargs.setdefault("use_printer", False)
        return Typewriter(bell=bell, **kwargs)
    return _make


def type_text(machine, text):
    for ch in text:
        machine.char_add(ch)


def fill_to(machine, length):
    """put the machine at a given line length with the cursor at the end"""
    machine.buffer[:] = list("x" * length)
    machine.cursor = length


# ----------------------------
# setup
# ----------------------------

def test_first_line_starts_at_left_margin(make):
    t = make(left_margin=5)
    assert t.give_buffer() == " " * 5
    assert t.cursor == 5


def test_right_margin_defaults_to_width(make):
    assert make().right_margin == 75


def test_right_margin_clamped_to_width(make):
    assert make(right_margin=100).right_margin == 75


def test_tab_bar_shows_margins(make):
    bar = make(left_margin=5, right_margin=60).give_tab_bar()
    assert bar[5] == "!"
    assert bar[60] == "!"
    assert bar.count("!") == 2


def test_tab_bar_marks_hot_zone_start(make):
    bar = make(left_margin=5, right_margin=60, margin_bell=8).give_tab_bar()
    assert bar[52] == ":"
    assert bar.count(":") == 1
    assert set(bar) == {".", "!", ":"}


def test_hot_zone_mark_follows_right_margin(make):
    t = make(right_margin=60, margin_bell=8)
    t.cursor = 70
    t.margin_set_right()
    bar = t.give_tab_bar()
    assert bar[62] == ":" and bar[52] == "."


def test_hot_zone_mark_is_where_the_bell_rings(make, bell):
    t = make(right_margin=30, margin_bell=8)
    col = t.give_tab_bar().index(":")
    type_text(t, "x" * col)
    assert bell.count == 0
    t.char_add("x")                 # typing at the marked column
    assert bell.count == 1


def test_hot_zone_plays_ding_not_plain_bell(bell):
    ding = Bell()
    t = Typewriter(width=75, right_margin=30, margin_bell=8, use_file=False,
                   use_printer=False, bell=bell, ding=ding)
    type_text(t, "x" * 23)
    assert ding.count == 1 and bell.count == 0
    type_text(t, "x" * 7)                  # line full: error beep, no ding
    t.char_add("x")
    assert ding.count == 1 and bell.count == 1


def test_bell_off_silences_ding_and_beeps(bell):
    ding = Bell()
    t = Typewriter(width=75, right_margin=30, margin_bell=8, use_file=False,
                   use_printer=False, bell=bell, ding=ding)
    t.toggle_bell()
    assert not t.bell_on
    type_text(t, "x" * 31)                 # through the hot zone to full
    t.cursor_right()                        # at the end: would beep
    assert ding.count == 0 and bell.count == 0
    t.toggle_bell()
    t.cursor_right()
    assert bell.count == 1


def test_bell_can_start_off(bell):
    t = Typewriter(use_file=False, use_printer=False, bell=bell, bell_on=False)
    t.cursor_left()
    assert bell.count == 0


def test_save_bell_setting_as_toml_bool(tmp_path):
    cfg = tmp_path / "config.toml"
    cfg.write_text("bell = true # comment\nwidth = 75\n")
    save_settings_to_config({"bell": False}, cfg)
    text = cfg.read_text()
    assert "bell = false # comment" in text
    assert tomllib.loads(text)["bell"] is False
    save_settings_to_config({"bell": True}, cfg)
    assert tomllib.loads(cfg.read_text())["bell"] is True


def test_sound_player_falls_back_when_file_missing(tmp_path):
    beep = Bell()
    assert sound_player(str(tmp_path / "nope.wav"), beep) is beep
    assert sound_player("", beep) is beep


def test_sound_player_runs_player_in_background(tmp_path, monkeypatch):
    sound = tmp_path / "ding.ogg"           # not a wav, so no streaming
    sound.write_bytes(b"")
    calls = []
    monkeypatch.setattr("type2.shutil.which", lambda name: "/usr/bin/" + name)
    monkeypatch.setattr("type2.subprocess.Popen",
                        lambda args, **kw: calls.append(args))
    play = sound_player(str(sound), Bell())
    play()
    assert calls == [["paplay", str(sound)]]


class FakePipe:
    def __init__(self):
        self.data = b""

    def write(self, data):
        self.data += data

    def flush(self):
        pass


class FakePacat:
    def __init__(self, args, **kw):
        self.args = args
        self.stdin = FakePipe()
        self.returncode = None

    def poll(self):
        return self.returncode

    def terminate(self):
        self.returncode = -15


def test_wav_is_streamed_through_one_pacat(monkeypatch):
    started = []
    def popen(args, **kw):
        started.append(FakePacat(args, **kw))
        return started[-1]
    monkeypatch.setattr("type2.shutil.which", lambda name: "/usr/bin/" + name)
    monkeypatch.setattr("type2.subprocess.Popen", popen)
    play = sound_player("ding.wav", Bell())
    play()
    started_after_first = len(started)
    import time
    for _ in range(50):                     # let the writer thread finish
        if not started[0].stdin.data == b"":
            break
        time.sleep(0.01)
    play()
    assert started_after_first == 1 and len(started) == 1
    assert started[0].args[0] == "pacat"
    assert "--format=s16le" in started[0].args
    assert len(started[0].stdin.data) > 0


def test_dead_stream_falls_back_to_player(monkeypatch):
    started = []
    def popen(args, **kw):
        started.append(FakePacat(args, **kw))
        return started[-1]
    monkeypatch.setattr("type2.shutil.which", lambda name: "/usr/bin/" + name)
    monkeypatch.setattr("type2.subprocess.Popen", popen)
    play = sound_player("ding.wav", Bell())
    started[0].returncode = 1               # pacat has gone away
    play()
    assert started[1].args[0] == "paplay"


def test_margin_wins_if_hot_zone_starts_on_it(make):
    bar = make(left_margin=10, right_margin=18, margin_bell=8).give_tab_bar()
    assert bar[10] == "!"
    assert ":" not in bar


def test_tab_bar_right_marker_at_full_width(make):
    bar = make(right_margin=75).give_tab_bar()
    assert len(bar) == 75
    assert bar[74] == "!"


# ----------------------------
# typing and backspace
# ----------------------------

def test_typing_adds_chars_and_moves_cursor(make):
    t = make()
    type_text(t, "hello")
    assert t.give_buffer() == "hello"
    assert t.cursor == 5


def test_backspace_removes_last_char(make):
    t = make()
    type_text(t, "hello")
    t.backspace()
    assert t.give_buffer() == "hell"
    assert t.cursor == 4


def test_backspace_at_column_zero_does_nothing(make):
    t = make()
    t.backspace()
    assert t.give_buffer() == ""
    assert t.cursor == 0


def test_backspace_with_text_but_cursor_at_zero_keeps_text(make):
    t = make()
    type_text(t, "abc")
    t.cursor = 0
    t.backspace()
    assert t.give_buffer() == "abc"


def test_full_line_rejects_char_and_dings(make, bell):
    t = make(right_margin=60)
    fill_to(t, 60)
    t.char_add("q")
    assert len(t.buffer) == 60
    assert bell.count >= 1


def test_margin_release_allows_typing_past_right_margin(make):
    t = make(right_margin=60)
    fill_to(t, 60)
    t.toggle_margin_release()
    t.char_add("q")
    assert len(t.buffer) == 61


def test_never_types_past_printable_width(make):
    t = make(right_margin=75)
    t.toggle_margin_release()
    fill_to(t, 74)
    t.char_add("q")
    assert len(t.buffer) == 74


# ----------------------------
# margin bell and autoreturn
# ----------------------------

def test_margin_bell_rings_at_bell_point(make, bell):
    t = make(right_margin=60, margin_bell=8)
    fill_to(t, 51)
    t.char_add("a")
    assert bell.count == 0
    t.char_add("b")          # buffer was 52 = 60 - 8
    assert bell.count == 1
    assert t.margin_hot_zone


def test_hot_zone_survives_backspace_and_retype(make):
    t = make(right_margin=60, margin_bell=8)
    fill_to(t, 52)
    t.char_add("a")
    t.backspace()
    t.char_add("a")
    assert t.margin_hot_zone


def test_not_in_hot_zone_before_bell_point(make):
    t = make(right_margin=60, margin_bell=8)
    type_text(t, "short")
    assert not t.margin_hot_zone


def test_space_in_hot_zone_dings_without_autoreturn(make, bell):
    t = make(right_margin=60, margin_bell=8, autoreturn=False)
    fill_to(t, 55)
    before = bell.count
    t.char_add(" ")
    assert bell.count == before + 1
    assert t.line_count == 0


@pytest.mark.parametrize("key", [" ", "-"])
def test_space_or_hyphen_in_hot_zone_autoreturns(make, key):
    t = make(right_margin=60, margin_bell=8, autoreturn=True, left_margin=3)
    fill_to(t, 55)
    t.char_add(key)
    assert t.line_count == 1
    assert t.give_buffer() == " " * 3
    assert t.cursor == 3


def test_letters_in_hot_zone_do_not_autoreturn(make):
    t = make(right_margin=60, margin_bell=8, autoreturn=True)
    fill_to(t, 55)
    t.char_add("a")
    assert t.line_count == 0


def test_toggle_autoreturn(make):
    t = make(autoreturn=False)
    t.toggle_autoreturn()
    assert t.autoreturn
    t.toggle_autoreturn()
    assert not t.autoreturn


# ----------------------------
# carriage return and history
# ----------------------------

def test_carriage_return_resets_line(make):
    t = make(left_margin=4)
    type_text(t, "words here")
    t.carriage_return()
    assert t.give_buffer() == " " * 4
    assert t.cursor == 4
    assert t.line_count == 1
    assert t.word_count == 2


def test_carriage_return_clears_margin_release(make):
    t = make()
    t.toggle_margin_release()
    t.carriage_return()
    assert not t.margin_release


def test_previous_lines_ripple_up(make):
    t = make()
    for line in ("one", "two", "three", "four"):
        type_text(t, line)
        t.carriage_return()
    assert t.give_prev_buff_1() == "four"
    assert t.give_prev_buff_2() == "three"
    assert t.give_prev_buff_3() == "two"


def test_carriage_return_writes_line_to_file(make, tmp_path):
    t = make(use_file=True)
    t.save_folder = str(tmp_path)      # no trailing slash on purpose
    type_text(t, "first")
    t.carriage_return()
    type_text(t, "second")
    t.carriage_return()
    assert (tmp_path / t.file_name).read_text() == "first\nsecond\n"


def test_unwritable_save_folder_exits_with_message(make):
    t = make(use_file=True)
    t.save_folder = "/nonexistent/folder"
    with pytest.raises(SystemExit, match="problem writing to the file"):
        t.carriage_return()


# ----------------------------
# line spacing
# ----------------------------

def test_line_spacing_cycles(make):
    t = make(spacing_index=0)
    seen = [t.current_spacing]
    for _ in range(3):
        t.toggle_line_space()
        seen.append(t.current_spacing)
    assert seen == [1, 1.5, 2, 1]


# ----------------------------
# margins
# ----------------------------

def test_set_left_margin_at_cursor(make):
    t = make(left_margin=5, right_margin=60)
    t.cursor = 20
    t.margin_set_left()
    assert t.left_margin == 20
    bar = t.give_tab_bar()
    assert bar[20] == "!" and bar[5] == "."


def test_left_margin_cannot_pass_right_margin(make, bell):
    t = make(left_margin=5, right_margin=60)
    t.cursor = 60
    t.margin_set_left()
    assert t.left_margin == 5
    assert bell.count == 1


def test_set_right_margin_at_cursor(make):
    t = make(left_margin=5, right_margin=60)
    t.cursor = 70
    t.margin_set_right()
    assert t.right_margin == 70
    bar = t.give_tab_bar()
    assert bar[70] == "!" and bar[60] == "."


def test_right_margin_can_be_set_to_full_width(make):
    t = make(right_margin=60)
    t.cursor = 75
    t.margin_set_right()
    assert t.right_margin == 75
    assert t.give_tab_bar()[74] == "!"


@pytest.mark.parametrize("column", [5, 2, 76])
def test_invalid_right_margin_rejected(make, bell, column):
    t = make(left_margin=5, right_margin=60)
    t.cursor = column
    t.margin_set_right()
    assert t.right_margin == 60
    assert bell.count == 1


# ----------------------------
# centring
# ----------------------------

def test_centre_text_centres_between_margins(make):
    t = make(left_margin=10, right_margin=60)
    t.centre_text()
    type_text(t, "Chapter One")
    line = t.give_buffer()
    assert line.strip() == "Chapter One"
    assert line.index("C") == 10 + (50 - 11) // 2
    assert t.cursor == len(line)


def test_centre_picks_up_existing_text(make):
    t = make(left_margin=10, right_margin=60)
    type_text(t, "Title")
    t.centre_text()
    assert t.give_buffer().index("T") == 10 + (50 - 5) // 2


def test_centre_backspace_recentres(make):
    t = make(left_margin=0, right_margin=60)
    t.centre_text()
    type_text(t, "abcd")
    t.backspace()
    line = t.give_buffer()
    assert line.strip() == "abc"
    assert line.index("a") == (60 - 3) // 2


def test_centre_backspace_on_empty_does_nothing(make):
    t = make()
    t.centre_text()
    t.backspace()
    assert t.centre_mode
    assert t.give_buffer().strip() == ""


def test_centre_dings_when_full(make, bell):
    t = make(left_margin=0, right_margin=10)
    t.centre_text()
    type_text(t, "x" * 10)
    before = bell.count
    t.char_add("y")
    assert bell.count == before + 1
    assert t.give_buffer() == "x" * 10


def test_carriage_return_ends_centre_mode(make):
    t = make(left_margin=10, right_margin=60)
    t.centre_text()
    type_text(t, "Hi")
    t.carriage_return()
    assert not t.centre_mode
    assert t.give_prev_buff_1().strip() == "Hi"
    assert t.give_buffer() == " " * 10


def test_toggling_centre_off_returns_text_to_left_margin(make):
    t = make(left_margin=4, right_margin=60)
    t.centre_text()
    type_text(t, "Hi")
    t.centre_text()
    assert not t.centre_mode
    assert t.give_buffer() == "    Hi"
    assert t.cursor == 6
    t.char_add("!")
    assert t.give_buffer() == "    Hi!"


def test_pausing_printer_keeps_lines_in_file_only(make, tmp_path):
    printer = FakePrinter()
    t = make(use_printer=True, printer=printer, use_file=True)
    t.save_folder = str(tmp_path)
    type_text(t, "one"); t.carriage_return()
    t.toggle_printer()
    assert not t.use_printer
    type_text(t, "two"); t.carriage_return()
    t.toggle_printer()
    assert t.use_printer
    type_text(t, "three"); t.carriage_return()
    # paused line is never sent, not even after resuming
    assert [line for line, _ in printer.lines] == ["one", "three"]
    assert (tmp_path / t.file_name).read_text() == "one\ntwo\nthree\n"


# ----------------------------
# line editing
# ----------------------------

def test_arrows_move_cursor_and_typing_inserts(make):
    t = make()
    type_text(t, "helo")
    t.cursor_left()
    t.char_add("l")
    assert t.give_buffer() == "hello"
    assert t.cursor == 4


def test_cursor_stops_at_ends_and_dings(make, bell):
    t = make()
    type_text(t, "ab")
    t.cursor_right()
    assert t.cursor == 2 and bell.count == 1
    t.cursor_left(); t.cursor_left(); t.cursor_left()
    assert t.cursor == 0 and bell.count == 2


def test_backspace_mid_line(make):
    t = make()
    type_text(t, "abXc")
    t.cursor_left()
    t.backspace()
    assert t.give_buffer() == "abc"
    assert t.cursor == 2


def test_delete_under_cursor(make):
    t = make()
    type_text(t, "abXc")
    t.cursor_left(); t.cursor_left()
    t.delete()
    assert t.give_buffer() == "abc"
    assert t.cursor == 2
    t.cursor_end()
    t.delete()                      # nothing under the cursor at the end
    assert t.give_buffer() == "abc"


def test_home_goes_to_margin_then_line_start(make):
    t = make(left_margin=5)
    type_text(t, "hello")
    t.cursor_home()
    assert t.cursor == 5
    t.cursor_home()
    assert t.cursor == 0
    t.cursor_end()
    assert t.cursor == 10


def test_insert_on_full_line_dings_without_changing_it(make, bell):
    t = make(right_margin=10, autoreturn=True)
    type_text(t, "abcdefghij")
    t.cursor_home()
    bell.count = 0
    t.char_add("X")
    assert t.give_buffer() == "abcdefghij"
    assert t.line_count == 0            # no autoreturn mid-line
    assert bell.count >= 1


def test_space_mid_line_in_hot_zone_does_not_autoreturn(make):
    t = make(right_margin=20, margin_bell=8, autoreturn=True)
    type_text(t, "abcdefghijklmn")      # into the hot zone
    t.cursor_left(); t.cursor_left()
    t.char_add(" ")
    assert t.line_count == 0
    assert t.give_buffer() == "abcdefghijkl mn"


def test_edited_line_is_what_gets_printed(make):
    printer = FakePrinter()
    t = make(use_printer=True, printer=printer)
    type_text(t, "teh cat")
    t.cursor_home()
    t.cursor_right(); t.cursor_right()
    t.delete()
    t.cursor_left()
    t.char_add("h")
    t.carriage_return()
    assert printer.lines[0][0] == "the cat"


def test_delete_word_at_end(make):
    t = make()
    type_text(t, "hello big world")
    t.delete_word()
    assert t.give_buffer() == "hello big "
    t.delete_word()                 # skips the space, then deletes "big"
    assert t.give_buffer() == "hello "
    assert t.cursor == 6


def test_delete_word_mid_line(make):
    t = make()
    type_text(t, "one two three")
    for _ in range(6):
        t.cursor_left()             # cursor just after "two"
    t.delete_word()
    assert t.give_buffer() == "one  three"
    assert t.cursor == 4


def test_delete_word_stops_at_left_margin(make):
    t = make(left_margin=5)
    type_text(t, "hello")
    t.delete_word()
    assert t.give_buffer() == " " * 5
    assert t.cursor == 5
    t.delete_word()                 # at the margin: clears to line start
    assert t.give_buffer() == ""
    assert t.cursor == 0


def test_delete_word_on_empty_line_does_nothing(make):
    t = make()
    t.delete_word()
    assert t.give_buffer() == "" and t.cursor == 0


def test_delete_word_in_right_justify(make):
    t = make(right_margin=20)
    t.right_justify()
    type_text(t, "ab cd")
    t.delete_word()
    assert t.give_buffer() == " " * 17 + "ab "
    assert t.right_mode


def test_arrows_ignored_in_align_mode(make, bell):
    t = make(right_margin=20)
    t.right_justify()
    type_text(t, "ab")
    t.cursor_left()
    assert t.cursor == 20 and bell.count == 1


# ----------------------------
# right justify
# ----------------------------

def test_right_justify_sits_against_right_margin(make):
    t = make(left_margin=5, right_margin=40)
    t.right_justify()
    type_text(t, "hello")
    assert t.give_buffer() == " " * 35 + "hello"
    assert t.cursor == 40


def test_right_justify_picks_up_existing_text(make):
    t = make(right_margin=20)
    type_text(t, "abc")
    t.right_justify()
    assert t.give_buffer() == " " * 17 + "abc"


def test_right_justify_backspace(make):
    t = make(right_margin=20)
    t.right_justify()
    type_text(t, "abcd")
    t.backspace()
    assert t.give_buffer() == " " * 17 + "abc"


def test_right_justify_dings_when_full(make, bell):
    t = make(left_margin=0, right_margin=4)
    t.right_justify()
    type_text(t, "abcd")
    bell.count = 0
    t.char_add("e")
    assert t.give_buffer() == "abcd"
    assert bell.count == 1


def test_right_justify_line_is_printed_aligned(make):
    printer = FakePrinter()
    t = make(right_margin=10, use_printer=True, printer=printer)
    t.right_justify()
    type_text(t, "end")
    t.carriage_return()
    assert printer.lines[0][0] == " " * 7 + "end"
    assert printer.aligns == ["right"]
    assert not t.right_mode
    assert t.give_buffer() == ""


def test_switch_from_centre_to_right(make):
    t = make(right_margin=20)
    t.centre_text()
    type_text(t, "ab")
    t.right_justify()
    assert t.right_mode and not t.centre_mode
    assert t.give_buffer() == " " * 18 + "ab"


def test_toggling_right_off_returns_text_to_left_margin(make):
    t = make(left_margin=2, right_margin=20)
    t.right_justify()
    type_text(t, "ab")
    t.right_justify()
    assert not t.right_mode
    assert t.give_buffer() == "  ab"
    t.char_add("!")
    assert t.give_buffer() == "  ab!"


def test_toggling_off_after_switching_modes_returns_left(make):
    t = make(right_margin=20)
    t.centre_text()
    type_text(t, "ab")
    t.right_justify()
    t.right_justify()
    assert t.align is None
    assert t.give_buffer() == "ab"



# ----------------------------
# printer
# ----------------------------

def test_no_printer_ids_means_no_printer(make):
    t = make(use_printer=True)
    assert not t.printer_found


class FakePrinter:
    """records what would have been printed"""
    def __init__(self):
        self.lines = []
        self.aligns = []

    def print_line(self, text, spacing=1, align=None):
        self.lines.append((text, spacing))
        self.aligns.append(align)


def test_carriage_return_sends_line_and_spacing_to_printer(make):
    printer = FakePrinter()
    t = make(use_printer=True, printer=printer)
    assert t.printer_found
    type_text(t, "hello")
    t.toggle_line_space()   # 1.5 -> 2
    t.carriage_return()
    assert printer.lines == [("hello", 2)]


def test_printer_not_used_when_use_printer_off(make):
    printer = FakePrinter()
    t = make(use_printer=False, printer=printer)
    type_text(t, "hello")
    t.carriage_return()
    assert printer.lines == []


def test_unknown_printer_type_exits():
    with pytest.raises(SystemExit):
        open_printer({"printer": "laser"})


def test_missing_epson_ids_gives_no_printer():
    assert open_printer({"printer": "epson"}) is None


def test_printer_option_on_command_line():
    assert parse_args(["--printer", "zebra"]).printer == "zebra"
    assert parse_args(["-p", "epson"]).printer == "epson"
    assert parse_args([]).printer is None   # fall back to config


def test_printer_name_defaults_to_epson():
    assert printer_name({}) == "epson"
    assert printer_name({"printer": "Zebra"}) == "zebra"


def test_bad_printer_option_rejected():
    with pytest.raises(SystemExit):
        parse_args(["--printer", "laser"])


def test_use_file_argument_still_works():
    assert parse_args(["False", "-p", "zebra"]).use_file == "False"
    assert parse_args([]).use_file is None


# ----------------------------
# config saving
# ----------------------------

CONFIG_WITH_COMMENTS = """\
width = 75
margin_bell = 8 # numbers of spaces from left margin to bell
right_margin = 75 # set the distance to the right margin
left_margin = 0
save_folder = "/somewhere/"
"""


def test_save_margins_updates_values_and_keeps_comments(tmp_path):
    cfg = tmp_path / "config.toml"
    cfg.write_text(CONFIG_WITH_COMMENTS)
    save_margins_to_config(10, 62, cfg)
    text = cfg.read_text()
    assert "right_margin = 62 # set the distance to the right margin" in text
    assert "left_margin = 10\n" in text
    assert "margin_bell = 8 # numbers" in text
    data = tomllib.loads(text)
    assert data["left_margin"] == 10
    assert data["right_margin"] == 62
    assert data["width"] == 75


def test_save_margins_adds_missing_keys(tmp_path):
    cfg = tmp_path / "config.toml"
    cfg.write_text('width = 75\nsave_folder = "x"')     # no trailing newline
    save_margins_to_config(3, 70, cfg)
    data = tomllib.loads(cfg.read_text())
    assert data["left_margin"] == 3
    assert data["right_margin"] == 70
    assert data["save_folder"] == "x"


def test_save_margins_does_not_touch_similar_keys(tmp_path):
    cfg = tmp_path / "config.toml"
    cfg.write_text("left_margin = 0\nright_margin = 75\nold_left_margin = 9\n")
    save_margins_to_config(4, 50, cfg)
    data = tomllib.loads(cfg.read_text())
    assert data["old_left_margin"] == 9
    assert data["left_margin"] == 4


def test_save_line_spacing_keeps_comment(tmp_path):
    cfg = tmp_path / "config.toml"
    cfg.write_text("width = 75\nspacing_index = 1 # 1.5 line spacing\n")
    save_settings_to_config({"spacing_index": 2}, cfg)
    text = cfg.read_text()
    assert "spacing_index = 2 # 1.5 line spacing" in text
    assert tomllib.loads(text)["width"] == 75


def test_saved_spacing_index_restores_spacing(make):
    t = make()
    t.toggle_line_space()           # 1.5 -> 2
    restored = make(spacing_index=t.spacing_index)
    assert restored.current_spacing == 2


# ----------------------------
# zebra line layout
# ----------------------------

def test_zebra_right_justify_ends_at_margin_column():
    zpl = line_zpl(" " * 60 + "end", align="right", char_dots=12)
    # block from the left edge to column 63, text right aligned in it
    assert f"^FO{LEFT},5" in zpl
    assert "^FB756,1,0,R^FDend^FS" in zpl


def test_zebra_indent_uses_column_width_not_spaces():
    zpl = line_zpl("     hello", char_dots=12)
    assert f"^FO{LEFT + 60},5" in zpl
    assert "^FDhello^FS" in zpl


def test_zebra_centre_is_centred_on_text_middle():
    # text occupies columns 20-30, so its middle is column 25
    zpl = line_zpl(" " * 20 + "x" * 10, align="centre", char_dots=10)
    assert f"^FO{LEFT},5" in zpl            # block 0..500 dots
    assert "^FB500,1,0,C" in zpl


def test_zebra_blank_line_still_feeds_paper():
    zpl = line_zpl("     ")
    assert "^FD" not in zpl
    assert "^GB1,1,1,W^FS" in zpl           # invisible field so it prints
    assert "^LL40" in zpl


def test_blank_carriage_return_sends_blank_line_to_printer(make):
    printer = FakePrinter()
    t = make(left_margin=4, use_printer=True, printer=printer)
    t.carriage_return()
    t.toggle_line_space()   # 1.5 -> 2
    t.carriage_return()
    assert printer.lines == [("    ", 1.5), ("    ", 2)]


def test_zebra_right_justify_clamped_to_paper():
    zpl = line_zpl(" " * 90 + "end", align="right", char_dots=12)
    assert "^FB772,1,0,R" in zpl
