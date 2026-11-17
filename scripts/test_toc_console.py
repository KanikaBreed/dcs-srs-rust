#!/usr/bin/env python3
"""Native null-PCM console check, called inside test_stock_server's namespace."""
import collections
import os
from pathlib import Path
import re
import select
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent


def verify_console(address):
    with tempfile.TemporaryDirectory(prefix="toc-null-", dir=ROOT / ".scratch") as temporary:
        config = Path(temporary) / "alsa.conf"
        config.write_text("pcm.!default { type null }\n", encoding="utf-8")
        env = dict(os.environ, ALSA_CONFIG_PATH=str(config), SRS_EAM_PASSWORD="fixture-blue")
        command = [str(ROOT / "target/debug/toc_console"), "--server", address,
                   "--id", "NNNNNNNNNNNNNNNNNNNNNN", "--name", "Synthetic console",
                   "--mhz", "251", "--mode", "am", "--input", "default", "--output", "default", "--seconds", "5"]
        process = subprocess.Popen(command, cwd=ROOT, env=env, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        buffered = bytearray()
        lines = collections.deque()
        transcript = []

        def wait_for(prefix):
            deadline = time.monotonic() + 4
            while True:
                while lines:
                    line = lines.popleft()
                    transcript.append(line)
                    if line.startswith(prefix):
                        return line
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not select.select([process.stdout], [], [], remaining)[0]:
                    raise RuntimeError("Console did not reach expected state: " + prefix)
                data = os.read(process.stdout.fileno(), 4096)
                if not data:
                    raise RuntimeError("Console exited before expected state: " + prefix)
                buffered.extend(data)
                while b"\n" in buffered:
                    line, _, remainder = buffered.partition(b"\n")
                    buffered[:] = remainder
                    lines.append(line.decode("utf-8"))

        def send(command):
            process.stdin.write((command + "\n").encode("ascii"))
            process.stdin.flush()

        try:
            wait_for("RX READY")
            send("tx 300")
            wait_for("TX LOCAL")
            wait_for("RX READY")  # timed release, without an rx command
            send("tx 5000")
            wait_for("TX LOCAL")
            send("rx")
            wait_for("RX READY")
            send("tune 2 253.125 fm")
            wait_for("RADIO 2: 253.125000 MHz FM")
            send("guard 2 243")
            wait_for("RADIO 2: 253.125000 MHz FM; guard 243.000000 MHz")
            send("key 2 7")
            wait_for("RADIO 2: 253.125000 MHz FM; guard 243.000000 MHz; key 7")
            send("select 2")
            wait_for("RADIO 2: 253.125000 MHz FM")
            send("tx 100")
            wait_for("TX LOCAL")
            wait_for("RX READY")
            send("off 2")
            wait_for("RADIO 2: 0.000001 MHz OFF")
            send("quit")
            final = wait_for("STOPPED sent=")
            process.wait(timeout=4)
            if process.returncode:
                raise RuntimeError("Console returned an error")
            match = re.search(r"sent=(\d+)", final)
            if match is None or int(match[1]) == 0:
                raise RuntimeError("Console did not encode/send a null capture frame")
            print("PASS: native null console authenticated; receive-only start, timed TX, explicit RX, live tuning/guard/key/selection/off, quit, and stream cleanup. No hardware.", flush=True)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            process.stdin.close()
            process.stdout.close()
            process.stderr.close()


if __name__ == "__main__":
    raise SystemExit("Run python3 scripts/test_stock_server.py; this helper requires its private namespace.")
