# typewriter

write to a line printer like a typewriter: type a line, press enter, and
it goes to a file and/or the printer (an epson ESC/POS dot matrix or a
zebra GK420d thermal printer).

there are three versions with the same keys and screen:

- [`python/`](./python/README.md): the original, using curses
- [`rust/`](./rust/README.md): a port that builds to a single binary, no venv needed
- [`rust-gui/`](./rust-gui/README.md): the rust version in a window instead of a terminal

the python version has its own `config.toml`. the two rust versions are one
cargo workspace (`Cargo.toml` at the top): they build together into the top
`target/` folder and share the `config.toml` there.

```
cd python && venv/bin/python type2.py        # see python/README.md for setup
cargo build --release && target/release/typewriter    # builds both rust versions
target/release/typewriter-gui
```

![screenshot](./python/screenshot.png)
