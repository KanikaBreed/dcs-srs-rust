#!/usr/bin/env python3
"""Synthetic terminal events plus native null PCM inside the stock-server namespace."""
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import tempfile
import termios
import time

ROOT = Path(__file__).resolve().parent.parent


def verify_keyboard(address):
    with tempfile.TemporaryDirectory(prefix="toc-keyboard-", dir=ROOT / ".scratch") as temporary:
        config = Path(temporary) / "alsa.conf"
        config.write_text("pcm.!default { type null }\n", encoding="utf-8")
        env = dict(os.environ, ALSA_CONFIG_PATH=str(config), SRS_EAM_PASSWORD="fixture-blue")
        base = [str(ROOT / "target/debug/toc_console"), "--server", address,
                "--id", "PPPPPPPPPPPPPPPPPPPPPP", "--name", "Synthetic keyboard",
                "--mhz", "251", "--mode", "am", "--input", "default", "--output", "default",
                "--seconds", "20", "--ptt", "keyboard"]
        # Also verify rejection/restoration on a legacy terminal, before connect/audio.
        for supported in (False, True):
            master, slave = pty.openpty()
            original = termios.tcgetattr(slave)
            process = subprocess.Popen(base, cwd=ROOT, env=env, stdin=slave, stdout=slave,
                                       stderr=subprocess.PIPE, start_new_session=True)
            buffer = bytearray()

            def receive(marker, seconds=4):
                deadline = time.monotonic() + seconds
                while marker not in buffer:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0 or not select.select([master], [], [], remaining)[0]:
                        raise RuntimeError("Keyboard probe did not reach " + repr(marker))
                    data = os.read(master, 4096)
                    if not data:
                        raise RuntimeError("Keyboard probe exited before " + repr(marker))
                    buffer.extend(data)
                    if len(buffer) > 65536:
                        raise RuntimeError("Keyboard probe output exceeded bound")
                end = buffer.index(marker) + len(marker)
                consumed = bytes(buffer[:end])
                del buffer[:end]
                return consumed

            def send(data):
                os.write(master, data)

            press = b"\x1b[19;1:1~"
            release = b"\x1b[19;1:3~"
            repeat = b"\x1b[19;1:2~"
            try:
                receive(b"\x1b[?u\x1b[c")
                send((b"\x1b[?0u" if supported else b"") + b"\x1b[?1;2c")
                if not supported:
                    process.wait(timeout=4)
                    if process.returncode == 0:
                        raise RuntimeError("Legacy terminal unexpectedly accepted keyboard mode")
                else:
                    receive(b"RX READY")
                    send(press + release)
                    receive(b"PTT ARMED")
                    send(press)
                    receive(b"TX LOCAL")
                    time.sleep(0.1)
                    send(release)
                    receive(b"RX READY")
                    send(press)
                    receive(b"TX LOCAL")
                    send(b"\x1b[O")  # focus loss while key remains held
                    receive(b"RX READY")
                    send(b"\x1b[I" + repeat)  # focus regain never resumes a held key
                    time.sleep(0.05)
                    send(release)
                    receive(b"PTT ARMED")
                    send(press)
                    receive(b"TX LOCAL")
                    send(release)
                    receive(b"RX READY")
                    send(b"2")
                    receive(b"RADIO 2:")
                    send(b"f253.125\r")
                    receive(b"RADIO 2: 253.125000 MHz AM")
                    # Wait for accumulated input to drain after the asynchronous edit.
                    receive(b"INPUT READY")
                    send(release)
                    receive(b"PTT ARMED")
                    send(press)
                    receive(b"TX LOCAL")
                    send(repeat)
                    receive(b"PTT RELEASED", seconds=7)  # hard cap even with lost key-up
                    receive(b"RX READY")
                    send(repeat)
                    time.sleep(0.05)
                    send(release)
                    receive(b"PTT ARMED")
                    send(press)
                    receive(b"TX LOCAL")
                    # A whole-process interruption now terminates the audio
                    # session. Queued keys must not restart TX on resume.
                    os.kill(process.pid, signal.SIGSTOP)
                    time.sleep(0.35)
                    send(release + press + repeat)
                    os.kill(process.pid, signal.SIGCONT)
                    stopped = receive(b"STOPPED sent=")
                    if b"TX LOCAL" in stopped:
                        raise RuntimeError("Suspended session resumed queued TX")
                    process.wait(timeout=4)
                    if process.returncode == 0:
                        raise RuntimeError("Suspended worker did not report interruption")
                if termios.tcgetattr(slave) != original:
                    raise RuntimeError("Keyboard console did not restore terminal attributes")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                process.stderr.close()
                os.close(master)
                os.close(slave)
    print("PASS: simulated terminal release calibration, held F8/release, focus loss, whole-process interruption without queued TX, live tuning, hold cap, legacy rejection and terminal restoration; native null audio only.", flush=True)


if __name__ == "__main__":
    raise SystemExit("Run scripts/test_stock_server.py; this helper needs its private namespace.")
