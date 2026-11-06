"""Read/write ~/.config/darkmount/config.toml ([actions] table)."""
from __future__ import annotations

from .actions import CONFIG_PATH, load_actions

HEADER = """\
# Dark Mount key actions, run by the darkmount background service.
# Key names: `darkmount keys names` (prefix with "Fn+" for the Fn layer).
# A command here runs whenever the keyboard reports that key's action.
# The file is reloaded automatically when saved.
"""


def _toml_str(s: str) -> str:
    if "\n" in s and "'''" not in s:
        return "'''\n" + s.rstrip("\n") + "\n'''"
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def _toml_key(k: str) -> str:
    return k if k.replace("_", "").replace("-", "").isalnum() else f'"{k}"'


def read_actions() -> dict[str, str]:
    """Actions keyed by the label as written (case preserved where possible)."""
    import tomllib
    try:
        with open(CONFIG_PATH, "rb") as f:
            return {k: str(v) for k, v in tomllib.load(f).get("actions", {}).items()}
    except (FileNotFoundError, tomllib.TOMLDecodeError):
        return {}


def write_actions(actions: dict[str, str]):
    CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
    lines = [HEADER, "[actions]"]
    for k, v in sorted(actions.items()):
        lines.append(f"{_toml_key(k)} = {_toml_str(v)}")
    tmp = CONFIG_PATH.with_suffix(".tmp")
    tmp.write_text("\n".join(lines) + "\n")
    tmp.replace(CONFIG_PATH)


def set_action(label: str, command: str | None):
    acts = read_actions()
    for k in [k for k in acts if k.lower() == label.lower()]:
        del acts[k]
    if command:
        acts[label] = command
    write_actions(acts)


__all__ = ["read_actions", "write_actions", "set_action", "load_actions"]
