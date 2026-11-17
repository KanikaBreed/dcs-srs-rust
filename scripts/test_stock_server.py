#!/usr/bin/env python3
"""Test the Rust session against stock SRS 2.4.1.0 in an isolated Linux network."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

from test_toc_console import verify_console
from test_keyboard import verify_keyboard
from test_desktop import verify_desktop

ROOT = Path(__file__).resolve().parent.parent
SHA256 = "46268dd35204d04426974f66d25eaddaebbaa6b133a7bd227675fae30ed88127"
URL = "https://github.com/ciribob/DCS-SimpleRadioStandalone/releases/download/2.4.1.0/SRS-Server-Commandline-Linux"


def verified_binary(path):
    with path.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    if digest != SHA256:
        raise SystemExit("Stock server SHA-256 differs from the pinned official release")


def measured_probe(command):
    """Reap this exact Linux child and report its CPU/RSS, excluding the server."""
    process = subprocess.Popen(command, cwd=ROOT)
    began = time.monotonic()
    try:
        while True:
            pid, status, usage = os.wait4(process.pid, os.WNOHANG)
            if pid:
                process.returncode = os.waitstatus_to_exitcode(status)
                if process.returncode:
                    raise subprocess.CalledProcessError(process.returncode, command)
                print(f"Session audio probe: wall={time.monotonic()-began:.3f}s "
                      f"user_cpu={usage.ru_utime:.3f}s system_cpu={usage.ru_stime:.3f}s "
                      f"peak_rss={usage.ru_maxrss}KiB (two synthetic endpoints; no devices)", flush=True)
                return
            if time.monotonic() - began > 30:
                raise subprocess.TimeoutExpired(command, 30)
            time.sleep(0.01)
    finally:
        if process.returncode is None:
            process.kill()
            process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", type=Path, help="existing official Linux binary; default downloads to .scratch")
    parser.add_argument("--isolated-from", help=argparse.SUPPRESS)
    parser.add_argument("--desktop-test-binary", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--desktop-only", action="store_true", help="run only the isolated desktop GUI/session check")
    args = parser.parse_args()
    if sys.platform != "linux":
        raise SystemExit("This stock-server runner requires Linux network namespaces")
    scratch = ROOT / ".scratch"
    scratch.mkdir(exist_ok=True)
    binary = (args.server or scratch / "reference-server/SRS-Server-Commandline-Linux").resolve()
    probe = ROOT / "target/debug/examples/stock_probe"
    if not args.isolated_from:
        if not binary.exists():
            if args.server:
                raise SystemExit("Specified server binary does not exist")
            binary.parent.mkdir(parents=True, exist_ok=True)
            temporary = binary.with_suffix(".download")
            print("Downloading pinned official SRS 2.4.1.0 Linux server...", flush=True)
            with urllib.request.urlopen(URL, timeout=60) as source, temporary.open("wb") as target:
                while block := source.read(1024 * 1024):
                    target.write(block)
            verified_binary(temporary)
            temporary.replace(binary)
        verified_binary(binary)
        binary.chmod(binary.stat().st_mode | 0o100)
        subprocess.run(["cargo", "build", "--locked", "-p", "srs-client", "-p", "srs-toc", "-p", "srs-desktop", "--examples", "--bins"], cwd=ROOT, check=True)
        build = subprocess.run(["cargo", "test", "--locked", "-p", "srs-desktop", "--lib", "--no-run", "--message-format=json"], cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True)
        artifacts = [json.loads(line) for line in build.stdout.splitlines()]
        test_binary = next(Path(item["executable"]) for item in artifacts
                           if item.get("reason") == "compiler-artifact" and item.get("executable")
                           and item["target"]["name"] == "srs_desktop")
        namespace = os.readlink("/proc/self/ns/net")
        command = ["unshare", "--net"]
        if os.geteuid() != 0:
            command += ["--user", "--map-root-user"]
        command += [sys.executable, str(Path(__file__).resolve()), "--server", str(binary), "--isolated-from", namespace]
        # Some hosted runners forbid unprivileged user namespaces. Use sudo only
        # for the child namespace when the caller explicitly opts in through CI.
        if os.environ.get("SRS_TEST_SUDO_NAMESPACE") == "1":
            command = ["sudo", "--non-interactive", "unshare", "--net", sys.executable,
                       str(Path(__file__).resolve()), "--server", str(binary), "--isolated-from", namespace]
        if args.desktop_only:
            command += ["--desktop-only"]
        command += ["--desktop-test-binary", str(test_binary)]
        subprocess.run(command, cwd=ROOT, check=True, timeout=120)
        return
    if os.readlink("/proc/self/ns/net") == args.isolated_from:
        raise SystemExit("Refusing to start stock server in the parent's network namespace")
    subprocess.run(["ip", "link", "set", "lo", "up"], check=True)
    # A fresh namespace must have only loopback, with no route to the host/LAN.
    interfaces = {name for _, name in socket.if_nameindex()}
    if interfaces != {"lo"}:
        raise SystemExit("Unexpected network interfaces in isolated test namespace")
    verified_binary(binary)
    for enabled in (True, False):
        with tempfile.TemporaryDirectory(prefix="stock-srs-", dir=scratch) as temporary:
            directory = Path(temporary)
            with socket.socket() as reserve:
                reserve.bind(("127.0.0.1", 0))
                port = reserve.getsockname()[1]
            (directory / "server.cfg").write_text(
                "[General Settings]\n"
                f"EXTERNAL_AWACS_MODE = {str(enabled).lower()}\n"
                "COALITION_AUDIO_SECURITY = true\nALLOW_RADIO_ENCRYPTION = true\n"
                "CLIENT_EXPORT_ENABLED = false\nLOTATC_EXPORT_ENABLED = false\nTRANSMISSION_LOG_ENABLED = false\n"
                f"[Server Settings]\nSERVER_PORT = {port}\nSERVER_IP = 127.0.0.1\n"
                "UPNP_ENABLED = false\nHTTP_SERVER_ENABLED = false\nCHECK_FOR_BETA_UPDATES = false\n"
                "[External AWACS Mode Settings]\n"
                "EXTERNAL_AWACS_MODE_BLUE_PASSWORD = fixture-blue\n"
                "EXTERNAL_AWACS_MODE_RED_PASSWORD = fixture-red\n", encoding="utf-8")
            # Stock startup prints its config/passwords. Keep its output private
            # and ephemeral, even though these test credentials are synthetic.
            with (directory / "console.log").open("wb") as output:
                process = subprocess.Popen([str(binary), "--cfg", str(directory / "server.cfg")],
                                           cwd=directory, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    deadline = time.monotonic() + 15
                    while True:
                        if process.poll() is not None:
                            raise SystemExit("Stock server exited before opening TCP")
                        try:
                            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                                break
                        except OSError:
                            if time.monotonic() >= deadline:
                                raise SystemExit("Stock server did not open TCP in 15 seconds")
                            time.sleep(0.1)
                    if args.desktop_only:
                        if enabled:
                            verify_desktop(f"127.0.0.1:{port}", args.desktop_test_binary)
                        continue
                    subprocess.run([str(probe), f"127.0.0.1:{port}", "enabled" if enabled else "disabled"],
                                   cwd=ROOT, check=True, timeout=30)
                    if enabled:
                        subprocess.run([str(ROOT / "target/debug/examples/voice_probe"), f"127.0.0.1:{port}"],
                                       cwd=ROOT, check=True, timeout=30)
                        measured_probe([str(ROOT / "target/debug/examples/session_audio_probe"), f"127.0.0.1:{port}"])
                        verify_console(f"127.0.0.1:{port}")
                        verify_keyboard(f"127.0.0.1:{port}")
                        verify_desktop(f"127.0.0.1:{port}", args.desktop_test_binary)
                finally:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGTERM)
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait()
    print("Verified official binary SHA-256; selected isolated stock-server checks passed.")


if __name__ == "__main__":
    main()
