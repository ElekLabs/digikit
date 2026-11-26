"""Project Cargo.lock onto the packages reachable from a workspace crate."""

from __future__ import annotations

import hashlib
import json
import tomllib
from pathlib import Path

PackageId = tuple[str, str, str]


def closure_hash(path: Path, crate: str) -> str:
    """Hash the reachable package records, including sources and checksums.

    Cargo shortens dependency references when a name or name/version is unique.
    Resolve those references before hashing so an unrelated package cannot change
    the key merely by making Cargo spell a dependency out.
    """
    return hashlib.sha256(_closure(path, crate)).hexdigest()


def _closure(path: Path, crate: str) -> bytes:
    lock = tomllib.loads(path.read_text())
    packages: dict[PackageId, dict] = {}
    aliases: dict[str, set[PackageId]] = {}
    for package in lock["package"]:
        name, version, source = (
            package["name"],
            package["version"],
            package.get("source", ""),
        )
        ident = (name, version, source)
        if ident in packages:
            raise ValueError(f"duplicate package in {path}: {ident!r}")
        packages[ident] = package
        refs = [name, f"{name} {version}"]
        if source:
            refs.append(f"{name} {version} ({source})")
        for ref in refs:
            aliases.setdefault(ref, set()).add(ident)

    def resolve(ref: str) -> PackageId:
        matches = aliases.get(ref, set())
        if len(matches) != 1:
            raise ValueError(f"missing or ambiguous package in {path}: {ref!r}")
        return next(iter(matches))

    roots = [i for i in packages if i[0] == crate and not i[2]]
    if len(roots) != 1:
        raise ValueError(f"expected one workspace package named {crate!r} in {path}")
    pending = roots
    closure: dict[PackageId, dict] = {}
    while pending:
        ident = pending.pop()
        if ident in closure:
            continue
        package = packages[ident].copy()
        deps = sorted(resolve(ref) for ref in package.pop("dependencies", []))
        if "replace" in package:
            replacement = resolve(package["replace"])
            package["replace"] = replacement
            pending.append(replacement)
        closure[ident] = {**package, "dependencies": deps}
        pending.extend(deps)
    record = {"version": lock["version"], "packages": sorted(closure.items())}
    return json.dumps(record, sort_keys=True, separators=(",", ":")).encode()
