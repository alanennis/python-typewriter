# rust typewriter, in a window

the same typewriter as `../rust`, in its own window instead of a terminal.
it uses the `rust/` crate as a library, so the typing, margins, bells,
centring, the epson and zebra printers and the config saving are the very
same code, as are the command line, the alt commands and the settings
panel (`../rust/src/front.rs`). only the window and the keys are new (`src/main.rs`, using
[egui](https://github.com/emilk/egui)).

runs on wayland (KDE, GNOME…) and X11.

## build and run

needs rust 1.85+ and libusb (`sudo apt install libusb-1.0-0-dev pkg-config`),
the same as `../rust`.

```
cargo build --release                         # from the top folder; builds both versions
target/release/typewriter-gui                 # uses config.toml next to the binary
target/release/typewriter-gui -p zebra        # same options as the terminal version
target/release/typewriter-gui False
target/release/typewriter-gui -f letter.txt
target/release/typewriter-gui --config ~/my.toml
target/release/typewriter-gui --continue
target/release/typewriter-gui --lines 7
```

## keys

all the same as the terminal version: type, enter, tab, arrows/home/end, del,
backspace, alt+backspace, and alt with l r s t x m w a n q p c j b h (alt-h shows
them). ctrl-c or alt-q quits, as does closing the window.

extras for the window:

- a settings panel: ctrl+, (or click `Width`, `LM`, `RM`, `File` or
  `ctrl+, settings` at the bottom right) opens it. it has the width,
  margins, margin bell, line spacing, previous lines, tab stops, word
  target, save folder and the on/off switches. save puts them to work
  straight away and writes them to the config; cancel or esc leaves things
  as they were. a new left margin starts on the next line unless nothing's
  typed on this one yet. the printer is still chosen in the config or with
  `-p`, since changing it needs a restart
- the text grows and shrinks with the window; ctrl `+` / ctrl `-` / ctrl `0`
  zoom as well
- `LS`, `A-RTN`, `Printing`, `Bell`, `Time`, `WPM` and `alt-h for help` can
  be clicked
- the bell beeps (a short tone) and flashes the line, since there's no
  terminal bell
- follows the desktop's light or dark theme
- a startup problem (bad config, say) is shown in a window as well as on
  the terminal, so it's seen when started from a launcher

## settings

it keeps its `config.toml` next to the binary (`target/release/config.toml`
in the top folder), made from `../rust/config.default.toml` on the first
run. the terminal version is built into the same folder, so the two share
it: settings changed in one show up in the other. see
`../rust/README.md` for the details. warnings go to `type.log` in the
current folder.

## tests

```
cargo test        # tests this and the terminal version
```
