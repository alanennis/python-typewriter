# rust typewriter

a rust port of `type2.py`. same keys, same screen, same `config.toml`.

## build and run

needs rust 1.85+ and libusb (`sudo apt install libusb-1.0-0-dev pkg-config`)

```
cd rust
cargo build --release
target/release/typewriter                 # uses config.toml next to the binary
target/release/typewriter -p zebra        # or -p epson, overrides config
target/release/typewriter False           # don't save to a file
target/release/typewriter --config ~/my.toml
```

## settings

the rust version has its own `config.toml`, in the same folder as the
binary (`target/release/config.toml` after a release build). it isn't
shared with the python version's `config.toml`.

if it's missing, the first run creates it from `config.default.toml`, which
is built into the binary. `@PROJECT_DIR@` in that file is replaced with this
project's folder, so the bell sound is still found wherever the binary
goes. delete the file to start again from the defaults. alt-l, alt-r,
alt-s and alt-b save back to it.

things to know:

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
cargo test
```

## layout

| file | what | python |
|---|---|---|
| `src/typewriter.rs` | the machine: buffer, margins, bells, alignment | `Typewriter` class |
| `src/main.rs` | screen, keys, command line, startup | curses functions, `run()` |
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
