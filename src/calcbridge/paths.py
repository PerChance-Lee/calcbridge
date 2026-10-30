"""Per-user locations for CalcBridge's own state."""
import os
import sys


def config_dir() -> str:
    if sys.platform == "win32":
        base = os.environ.get("APPDATA") or os.path.expanduser("~")
        return os.path.join(base, "CalcBridge")
    if sys.platform == "darwin":
        return os.path.expanduser("~/Library/Application Support/CalcBridge")
    return os.path.join(os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config"), "calcbridge")


def seq_state_file() -> str:
    return os.path.join(config_dir(), "link-state.json")
