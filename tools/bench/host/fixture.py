#!/usr/bin/env python3
"""Private PTY workload; runs no shell and reads no user configuration."""

import os
import tty


def write_all(data):
    remaining = memoryview(data)
    while remaining:
        remaining = remaining[os.write(1, remaining):]


tty.setraw(0)
write_all(b"READY")
pending = b""
counter = 0
while True:
    data = os.read(0, 256)
    if not data:
        break
    pending += data
    while b"\n" in pending:
        line, pending = pending.split(b"\n", 1)
        command, value = line.split()
        value = int(value)
        if command == b"E":
            counter += 1
            # The complete marker is the last output: observing it is a quiescence fence.
            write_all(f"\x1b[2J\x1b[HCafe\u0301 中\r\nACK:{counter}:{value}".encode())
        elif command == b"B" and 1024 <= value <= 64 * 1024 * 1024:
            chunk = (b"0123456789" * 7 + b"\r\n") * 64
            remaining = value
            while remaining:
                part = chunk[:remaining]
                write_all(part)
                remaining -= len(part)
            write_all(f"\r\nDONE:{value}".encode())
        else:
            raise ValueError("invalid benchmark command")
