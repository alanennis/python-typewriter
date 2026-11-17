# python typewriter
 write to epson line printer like typewriter

using python and curses to create a typewriter like interface\
and sending the final line to a file and/or a line printer\
that uses the ESC protocol

requires:\
python 3.11+ ( uses the match statement and tomllib )\
python-escpos 3.1 from https://pypi.org/project/python-escpos/ \
docs: https://python-escpos.readthedocs.io/en/latest/

curses and tomllib (built in)

## setup

```
python3 -m venv venv
venv/bin/pip install -r requirements.txt
venv/bin/python type2.py
venv/bin/python type2.py --printer zebra   # or -p epson, overrides config
```

settings live in `config.toml`. left and right margins are saved
back to it when you set them.

set `printer` to `"epson"` for the ESC/POS dot matrix printer or `"zebra"`
for the Zebra GK420d thermal printer. the zebra prints each line as a tiny
label, upside down by default so a torn-off strip reads in order
(`zebra_upright = true` turns that off). `zebra_lines.py` on its own
is a quick line-at-a-time test for the zebra.

## rust version

there's a rust port in [`rust/`](./rust/README.md): same keys, same screen,
same `config.toml`, one binary with no venv.

```
cd rust && cargo build --release && target/release/typewriter
```

## tests

```
venv/bin/pip install -r requirements-dev.txt
venv/bin/python -m pytest
```

[Screenshot](./screenshot.png)

At this point I have added all i need, the rest is just polish and\
stupidity checks, there are some more features that could be added\
like tabs and some housekeeping stuff, ini file loading so that\
the state is persistent when the program is started but they are\
relatively trivial and i will not learn much from doing them again
