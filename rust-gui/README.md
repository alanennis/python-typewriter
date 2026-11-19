# rust typewriter, in a window

the same typewriter as `../rust`, in its own window instead of a terminal.
it uses the `rust/` crate as a library, so the typing, margins, bells,
centring, the epson and zebra printers and the config saving are the very
same code. only the window and the keys are new (`src/main.rs`, using
[egui](https://github.com/emilk/egui)).

runs on wayland (KDE, GNOME…) and X11.

## build and run

needs rust 1.85+ and libusb (`sudo apt install libusb-1.0-0-dev pkg-config`),
the same as `../rust`.

```
cd rust-gui
cargo build --release
target/release/typewriter-gui                 # uses config.toml next to the binary
target/release/typewriter-gui -p zebra        # same options as the terminal version
target/release/typewriter-gui False
target/release/typewriter-gui -f letter.txt
target/release/typewriter-gui --config ~/my.toml
```

## keys

all the same as the terminal version: type, enter, arrows/home/end, del,
backspace, alt+backspace, and alt with l r s a n q p c j b h (alt-h shows
them). ctrl-c or alt-q quits, as does closing the window.

extras for the window:

- the text grows and shrinks with the window; ctrl `+` / ctrl `-` / ctrl `0`
  zoom as well
- `LS`, `A-RTN`, `Printing`, `Bell` and `alt-h for help` can be clicked
- the bell beeps (a short tone) and flashes the line, since there's no
  terminal bell
- follows the desktop's light or dark theme
- a startup problem (bad config, say) is shown in a window as well as on
  the terminal, so it's seen when started from a launcher

## settings

like the terminal version, it keeps its own `config.toml` next to the
binary (`target/release/config.toml`), made from `../rust/config.default.toml`
on the first run. it isn't shared with the terminal version's config. see
`../rust/README.md` for the details. warnings go to `type.log` in the
current folder.

## tests

```
cargo test
```
