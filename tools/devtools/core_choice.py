"""Which core library a run of a paired A/B uses, and the per-label `--env LABEL:NAME=VALUE` option.

A host holds no generated core (docs: packages/emulator/README.md "Core libraries"), so an A/B of two builds compares
the cores only when each label gets its own `DIGI_SHARC_CORE_LIB_<DEV>`. One rule everywhere: a variable that is set
explicitly (the label's `--env`, then the environment of the process) wins; without one the store provides the
library of the device (a miss builds it). `announce` prints which core each label runs, so a core change can never be
compared with itself unseen. `sharc_cells.py`, `coupled_ab.py` and `sharc_matrix.py` share this module.
"""

from __future__ import annotations

import os
from collections.abc import Iterable, Mapping
from pathlib import Path

from tools.devtools import core_store


def parse_label_env(
    items: Iterable[str], labels: Iterable[str], *, allow_global: bool = False
) -> tuple[dict[str, str], dict[str, dict[str, str]]]:
    """`LABEL:NAME=VALUE` items as (global env, env per label). With ALLOW_GLOBAL an item without a known label is a
    plain `NAME=VALUE` for every label (coupled_ab's older option). Raises ValueError on a malformed item or an unknown
    label."""
    known = set(labels)
    shared: dict[str, str] = {}
    per_label: dict[str, dict[str, str]] = {label: {} for label in known}
    for item in items:
        head, colon, rest = item.partition(":")
        labelled = bool(colon) and "=" not in head and "=" in rest
        if labelled:
            if head not in known:
                raise ValueError(f"unknown label {head!r} in {item!r}")
            name, _, value = rest.partition("=")
            target = per_label[head]
        elif allow_global and "=" in item:
            name, _, value = item.partition("=")
            target = shared
        else:
            raise ValueError(f"expected LABEL:NAME=VALUE, got {item!r}")
        if not name:
            raise ValueError(f"expected LABEL:NAME=VALUE, got {item!r}")
        target[name] = value
    return shared, per_label


def core_path(device: str, env: Mapping[str, str], root: Path | None = None) -> str:
    """The library a process with environment ENV loads for DEVICE: the explicit variable, else the store's (restored or
    built). Raises core_store.CoreStoreError when none can be provided."""
    var = core_store.CORE_LIB_VAR.format(device.upper())
    if env.get(var):
        return env[var]
    return core_store.library_env([device], root, environ=env)[var]


def describe(path: str) -> str:
    """A store library by its key, any other by its path."""
    parent = Path(path).parent
    in_store = parent.parent.name in ("dn2", "dt2") and parent.parent.parent.name == (
        core_store.STORE_NAME
    )
    return f"{parent.parent.name}/{parent.name[:12]}" if in_store else path


def announce(
    device: str,
    envs: Mapping[str, Mapping[str, str]],
    environ: Mapping[str, str] | None = None,
) -> str:
    """One line: the core of DEVICE each label runs (ENVS: label -> its environment overrides). `both labels use core
    <key>` when they are the same library, so a core change cannot silently compare the core with itself."""
    environ = os.environ if environ is None else environ
    chosen = {
        label: core_path(device, {**environ, **env}) for label, env in envs.items()
    }
    if len(set(chosen.values())) == 1:
        word = "both labels" if len(chosen) == 2 else "all labels"
        return f"{device}: {word} use core {describe(next(iter(chosen.values())))}"
    return f"{device}: " + "; ".join(
        f"{label} uses core {describe(p)}" for label, p in chosen.items()
    )
