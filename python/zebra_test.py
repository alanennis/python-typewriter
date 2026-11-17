#!/usr/bin/env python3
"""Line-by-line ZPL test print for a Zebra GK420d over USB (0a5f:0080).

Usage:
    python3 zebra_test.py                      # prints a default test label
    python3 zebra_test.py "Hello" "Second line"
    python3 zebra_test.py --device /dev/usb/lp0 "Hello"

Tries, in order:
  1. a /dev/usb/lp* device node (works when the kernel 'usblp' driver is bound)
  2. pyusb talking directly to 0a5f:0080 (pip install pyusb)
"""
import argparse
import glob
import sys

VID, PID = 0x0A5F, 0x0080
WIDTH_DOTS = 812      # 4" at 203 dpi
LEFT = 20
TOP = 20
LINE_HEIGHT = 40
FONT_H = FONT_W = 30


def clean(text):
    # ^ and ~ are ZPL command characters; drop them from label text
    return text.replace("^", "").replace("~", "")


def build_zpl(lines):
    zpl = [f"^XA^PW{WIDTH_DOTS}^LH0,0"]
    for i, line in enumerate(lines):
        y = TOP + i * LINE_HEIGHT
        zpl.append(f"^FO{LEFT},{y}^A0N,{FONT_H},{FONT_W}^FD{clean(line)}^FS")
    # label length = enough room for all lines
    zpl.insert(1, f"^LL{TOP * 2 + len(lines) * LINE_HEIGHT}")
    zpl.append("^XZ")
    return ("\n".join(zpl) + "\n").encode("utf-8")


def send_devnode(path, data):
    with open(path, "wb", buffering=0) as f:
        f.write(data)
    print(f"Sent {len(data)} bytes to {path}")


def send_pyusb(data):
    import usb.core
    import usb.util

    dev = usb.core.find(idVendor=VID, idProduct=PID)
    if dev is None:
        raise RuntimeError("Printer 0a5f:0080 not found on USB")

    if dev.is_kernel_driver_active(0):
        dev.detach_kernel_driver(0)
    dev.set_configuration()
    intf = dev.get_active_configuration()[(0, 0)]
    ep = usb.util.find_descriptor(
        intf,
        custom_match=lambda e: usb.util.endpoint_direction(e.bEndpointAddress)
        == usb.util.ENDPOINT_OUT,
    )
    if ep is None:
        raise RuntimeError("No OUT endpoint found")
    ep.write(data)
    usb.util.dispose_resources(dev)
    print(f"Sent {len(data)} bytes via pyusb")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("lines", nargs="*", help="text lines to print")
    ap.add_argument("--device", help="e.g. /dev/usb/lp0")
    args = ap.parse_args()

    lines = args.lines or ["Zebra GK420d test", "Line 2: USB OK", "Line 3: ZPL OK"]
    data = build_zpl(lines)

    nodes = [args.device] if args.device else sorted(glob.glob("/dev/usb/lp*"))
    for node in nodes:
        try:
            send_devnode(node, data)
            return
        except OSError as e:
            print(f"{node}: {e}", file=sys.stderr)

    try:
        send_pyusb(data)
    except Exception as e:
        print(f"pyusb failed: {e}", file=sys.stderr)
        print("Hint: run with sudo, add a udev rule for 0a5f:0080, "
              "or load usblp (sudo modprobe usblp).", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
