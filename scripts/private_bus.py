"""Private test D-Bus without desktop service activation or inherited sessions."""
from contextlib import contextmanager
from pathlib import Path
import tempfile


@contextmanager
def private_bus():
    with tempfile.TemporaryDirectory(prefix="srs-test-bus-") as directory:
        config = Path(directory) / "bus.conf"
        config.write_text('''<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen>
<policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy>
</busconfig>
''', encoding="utf-8")
        yield ["dbus-run-session", "--config-file", str(config), "--"]
