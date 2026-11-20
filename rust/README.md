# rust typewriter

a rust port of `type2.py`. same keys, same screen, same `config.toml`.

## build and run

needs rust 1.85+ and libusb (`sudo apt install libusb-1.0-0-dev pkg-config`)

```
cargo build --release                     # from the top folder (or any folder in it)
target/release/typewriter                 # uses config.toml next to the binary
target/release/typewriter -p zebra        # or -p epson, overrides config
target/release/typewriter False           # don't save to a file
target/release/typewriter -f letter.txt   # write to letter.txt in save_folder, carrying on if it's there
target/release/typewriter -f ./notes.txt  # a path is from the current folder
target/release/typewriter --config ~/my.toml
target/release/typewriter --continue      # carry on with the newest dated file in save_folder
target/release/typewriter --lines 7       # show 7 finished lines above the typing (0 to 7), from now on
```

## extras over the python version

- **tab stops.** alt-t sets a tab stop at the cursor (or clears the one
  there), alt-x clears them all, and the tab key jumps to the next one,
  filling with spaces past the end of the text. stops show as a `,` on the tab
  bar and are saved to `tab_stops` in the config. tab dings if there's no
  stop ahead before the right margin.
- **carrying on with a file.** `-f letter.txt` on a file that's already
  there carries on with it: new lines go on the end, its last lines show
  above the typing, and LC and WC count the whole file. `--continue`
  does the same with the newest dated file (`2026-11-19-210000.txt` style)
  in `save_folder`, so you don't need to know its name.
- **how many finished lines show** above the typing: `previous_lines = 3`
  in the config, 0 to 7. `--lines N` changes it and saves it to the config,
  so it stays that way next time. the rest of the screen moves down to make
  room, so the terminal needs 13 lines plus one for each (16 for the usual
  3, 20 for 7).
- **session stats** on the bottom row of the settings: `Time` since the
  start in whole minutes (`12m`, `1h05m`), `WPM` (words in finished lines per
  minute of typing; gaps of over a minute between keys are left out, and it
  shows `-` until there's half a minute of typing) and the words written
  this session. with `word_target = 500` in the config that shows as
  `Goal=123/500`, and the margin bell rings once when it's reached.
  alt-m hides or shows the time and alt-w the words per minute; they're
  saved as `show_timer` and `show_wpm` in the config.

## settings

the rust version has its own `config.toml`, in the same folder as the
binary (`target/release/config.toml` in the top folder after a release
build). the window version (`../rust-gui`) is built next to it, so it
uses the same file. it isn't shared with the python version's `config.toml`.

if it's missing, the first run creates it from `config.default.toml`, which
is built into the binary. `@RUST_DIR@` in that file is replaced with this
`rust/` folder (where its copies of the bell sounds are), so the bell is found wherever the binary
goes. delete the file to start again from the defaults. alt-l, alt-r,
alt-s, alt-b, alt-t, alt-x, alt-m and alt-w save back to it.

things to know:

- this folder and `../rust-gui` are one cargo workspace (`../Cargo.toml`),
  so the build goes in the top `target/` folder, not `rust/target/`.
- `cargo clean` deletes `target/`, and this config with it. copy the
  binary somewhere else (`cp target/release/typewriter ~/bin/`) to keep its
  config out of harm's way; it makes a fresh one there on the first run.
- debug builds (`target/debug/`) get a config of their own.
- edit `config.default.toml` to change the starting settings for new
  configs; it doesn't touch ones that already exist.

a relative `bell_sound` is looked up next to the config file. warnings go
to `type.log` in the current folder, as before.

`target/release/zebra-lines` is the line-at-a-time zebra test
(`--device`, `--continuous`, `--upright`, `--darkness`, `--font`, `--width`).

## tests

```
cargo test                # tests this and the window version
```

## layout

| file | what | python |
|---|---|---|
| `src/typewriter.rs` | the machine: buffer, margins, bells, alignment | `Typewriter` class |
| `src/main.rs` | terminal screen and keys | curses functions, `run()` |
| `src/front.rs` | command line, startup, alt commands, panel layout (shared with `../rust-gui`) | `key_check`, `show_handy_settings`, `run()` |
| `src/config.rs` | load config, save settings keeping comments | `save_settings_to_config` |
| `src/printer.rs` | epson ESC/POS, picking a printer | `EpsonPrinter`, `open_printer` |
| `src/zebra.rs` | ZPL lines, font drawing, zebra connection | `zebra_lines.py` |
| `src/sound.rs` | margin bell through pacat or a player | `StreamingSound`, `sound_player` |
| `src/usb.rs` | raw USB writes (libusb) | python-escpos / pyusb |

## differences from the python version

- the epson is opened at startup, so `Printer Found` is accurate. (python-escpos
  only connects on the first print, so python showed `True` even when the
  printer was unplugged.)
- a failed print is logged to `type.log` instead of crashing the program;
  the line is still in the file.
- ctrl-c quits cleanly, restoring the zebra's settings.
- with `zebra_font`, letters can differ from python's by the odd dot on
  curved edges (pillow uses freetype's hinting, ab_glyph doesn't). size,
  position and spacing are the same.
- the highlight colour assumes a 256 colour terminal (python fell back to
  cyan on 8 colour terminals).
