"""Write dist/bytes/daemon-test.key: the cache key part of a device test for the daemon, without the sources that only patchkit reads.

    uv run python -m tools.devtools.daemon_test_key [--daemons dist/daemons.json] [--out dist/bytes/daemon-test.key]

The daemon links patchkit and patches the stock image at boot, so `emud_key.key` (the store key of the daemon binary) moves with
every patchkit edit. A device test result follows the daemon's own behaviour and the patched image bytes
(dist/bytes/images.sha256, tools/nx/device-images.mjs), so the merge key of a test hashes this file and not dist/daemons.json.
It is `emud_key.key_of` over the same inputs minus the files in `EXCLUDED`, one line `<device> <sha256>` per daemon that
`verify_build daemons` published. The store key itself is not touched here.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
from pathlib import Path

from tools.devtools import emud_key

# What only patchkit reads: its sources, and the files it embeds with include_str (sdk/sdk.toml in sdkgen.rs, the toolchain pins in
# build/toolchain.rs), plus the cargo test sources of a package, which no binary build reads. The patched image bytes carry what
# these change for a test (dist/bytes/images.sha256).
EXCLUDED = ("packages/patchkit/", "sdk/sdk.toml", "tools/patchkit/toolchain/")


def excluded(name: str) -> bool:
    return name.startswith(EXCLUDED) or (
        name.startswith("packages/") and "/tests/" in name
    )


def test_key(device: str) -> str:
    inputs = emud_key.inputs(device)
    kept = {n: p for n, p in inputs.files.items() if not excluded(n)}
    return emud_key.key_of(dataclasses.replace(inputs, files=kept))


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("--daemons", type=Path, default=emud_key.ROOT / "dist/daemons.json")
    ap.add_argument(
        "--out", type=Path, default=emud_key.ROOT / "dist/bytes/daemon-test.key"
    )
    args = ap.parse_args(argv)
    published = json.loads(args.daemons.read_text())
    # the keys are `<variable prefix>_<DEVICE>`: one per daemon that `verify_build daemons` published
    devices = sorted(k.rsplit("_", 1)[-1].lower() for k in published)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("".join(f"{d} {test_key(d)}\n" for d in devices))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
