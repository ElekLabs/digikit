#!/usr/bin/env python3
"""One queryable SQLite database per ColdFire (MAIN OS) image: functions,
call/jump edges, data references, strings and symbols, imported from a
tools/coldfire/ghidradump.py dump directory (manifest.json, xrefs.sqlite, decomp/,
disasm/).

    uv run python -m tools.coldfire.cfdb build out/ghidra/dt2-1.16-emac
        [--name dt2-1.16] [--out out/cfdb/dt2-1.16.sqlite] [--force]

This exists for the same reason tools/sharc_dev/sharcdb/ does for the SHARC+ side
(see that file's module docstring): "who calls 0x400caf48" or "what writes
0x80004704" should be a `sqlite3` query in milliseconds against a database
built once, not a fresh xrefs.sqlite scan and a throwaway script every time.
It is a separate tool from tools/sharc_dev/sharcdb/ (which also has an
`import-ghidra` subcommand, built for the same xrefs.sqlite/manifest.json
shape) rather than a second image type sharing that file's schema and
DB_VERSION: sharcdb.py's DB_VERSION and code-block bookkeeping exist for the
SHARC+ side's own decode concerns, and a ColdFire image has no `sw` (SHARC
sequencer word) addressing, no code-block selection and no basic-block/
reaching-definitions layer -- keeping this schema and version number
separate means a SHARC-only schema change never forces a ColdFire rebuild
and vice versa. tools/coldfire/cf.py is the query layer, mirroring tools/sharc_dev/sharc.py.

Ghidra's call/data-ref tables miss code outside functions (small trampolines,
raw branch targets never turned into a function) -- an empty `callers()`
here is not evidence of dead code; confirm with tools/coldfire/refscan.py on the raw
image before recording "no caller" (see CLAUDE.md's Ghidra section and
tools/sharc_dev/sharcdb/'s own "empty Ghidra caller list" note).

The database also holds the function texts and their full-text indexes.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import sqlite3
import sys
import time
from contextlib import closing

from tools.firmware.artifacts import FirmwareArtifacts, coldfire_database_path
from tools.firmware.query_db import FUNCTION_NAMES_SQL

DB_VERSION = 2
DEFAULT_DB_DIR = "out/cfdb"

SCHEMA = """
CREATE TABLE meta (image TEXT, key TEXT, value TEXT, PRIMARY KEY (image, key));

CREATE TABLE functions (
  image TEXT, entry INTEGER, end INTEGER, name TEXT, namespace TEXT,
  size INTEGER, signature TEXT, decomp_path TEXT, disasm_path TEXT,
  decomp_error TEXT,
  PRIMARY KEY (image, entry)
);
CREATE INDEX functions_range ON functions(image, entry, end);
CREATE INDEX functions_name ON functions(image, name);

-- kind: normalized ('call'/'jump'/'cond_jump'); raw_kind: Ghidra's own
-- RefType name (UNCONDITIONAL_CALL, COMPUTED_JUMP, ...).
CREATE TABLE edges (
  image TEXT, site INTEGER, from_func INTEGER, to_func INTEGER,
  to_addr INTEGER, kind TEXT, raw_kind TEXT
);
CREATE INDEX edges_to_func ON edges(image, to_func);
CREATE INDEX edges_to_addr ON edges(image, to_addr);
CREATE INDEX edges_from_func ON edges(image, from_func);

CREATE TABLE datarefs (
  image TEXT, site INTEGER, func INTEGER, to_addr INTEGER, kind TEXT,
  label TEXT, block TEXT
);
CREATE INDEX datarefs_addr ON datarefs(image, to_addr);
CREATE INDEX datarefs_func ON datarefs(image, func);

CREATE TABLE strings (image TEXT, addr INTEGER, text TEXT);
CREATE INDEX strings_addr ON strings(image, addr);

CREATE TABLE symbols (
  image TEXT, addr INTEGER, name TEXT, namespace TEXT, type TEXT,
  is_primary INTEGER
);
CREATE INDEX symbols_addr ON symbols(image, addr);
CREATE INDEX symbols_name ON symbols(image, name);

-- Version 2: cross-version matching facts (tools/coldfire/cffacts.py, tools/coldfire/cfmatch.py).
-- h_*: masked instruction hashes of the function body; mnem_seq: its mnemonics.
CREATE TABLE func_hash (
  image TEXT, entry INTEGER, ninsn INTEGER, nbytes INTEGER,
  h_strict TEXT, h_loose TEXT, h_mnem TEXT, mnem_seq TEXT,
  PRIMARY KEY (image, entry)
);
CREATE INDEX func_hash_strict ON func_hash(image, h_strict);
CREATE INDEX func_hash_loose ON func_hash(image, h_loose);

-- RTTI from the dump (tools/coldfire/rttiscan.py applied): typeinfo objects and one row
-- per vtable slot.
CREATE TABLE typeinfos (
  image TEXT, addr INTEGER, kind TEXT, name TEXT, mangled TEXT, bases TEXT
);
CREATE INDEX typeinfos_name ON typeinfos(image, name);
CREATE TABLE vtables (
  image TEXT, addr INTEGER, class TEXT, typeinfo INTEGER, offset_to_top INTEGER,
  slot INTEGER, target INTEGER, owner TEXT
);
CREATE INDEX vtables_class ON vtables(image, class);
CREATE INDEX vtables_target ON vtables(image, target);

-- Names carried from another image by tools/coldfire/cfmatch.py port.
CREATE TABLE ported (
  image TEXT, entry INTEGER, name TEXT, from_image TEXT, from_entry INTEGER,
  method TEXT, score REAL,
  PRIMARY KEY (image, entry, name)
);
"""

_CALL_KIND = {
    "UNCONDITIONAL_CALL": "call",
    "COMPUTED_CALL": "call",
    "UNCONDITIONAL_JUMP": "jump",
    "COMPUTED_JUMP": "jump",
    "CONDITIONAL_JUMP": "cond_jump",
}


ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def _cffacts():
    from tools.coldfire import cffacts

    return cffacts


def _find_image(manifest, sha):
    """The MAIN OS file whose SHA-256 is the dump's (the manifest's recorded
    path can name a later image: each image has its own out/sections/<image>/)."""
    import glob
    import hashlib

    cands = [manifest.get("sections_image"), manifest.get("image_path")]
    cands += sorted(
        glob.glob(os.path.join(ROOT, "out/sections/*/section_3_MAIN_OS.bin"))
    )
    for cand in cands:
        if not cand:
            continue
        for path in (cand, os.path.join(ROOT, cand)):
            if os.path.isfile(path):
                with open(path, "rb") as f:
                    if hashlib.sha256(f.read()).hexdigest() == sha:
                        return path
    return None


def open_db(path):
    db = sqlite3.connect(path)
    db.executescript(SCHEMA)
    return db


def read_meta(path):
    if not os.path.exists(path):
        return {}
    db = sqlite3.connect(path)
    try:
        image = db.execute("SELECT DISTINCT image FROM meta").fetchone()
        if image is None:
            return {}
        return dict(
            db.execute("SELECT key, value FROM meta WHERE image=?", image).fetchall()
        )
    finally:
        db.close()


def _table_exists(db, name):
    return (
        db.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (name,)
        ).fetchone()
        is not None
    )


def _function_end(entry, size, ranges_by_func):
    """The entry's own contiguous end address: the range whose lo == entry
    when the dump recorded one (a function can have disjoint chunks, e.g. a
    switch's case bodies far from the entry -- see tools/sharc_dev/sharcdb/'s
    build_ghidra_database for the same situation on the SHARC side), else a
    size-based fallback."""
    spans = ranges_by_func.get(entry)
    if not spans:
        return entry + max(size or 1, 1)
    for lo, hi in spans:
        if lo == entry:
            return hi + 1
    return max(hi for _lo, hi in spans) + 1


def build_database(dump_dir, out_path, name=None, force=False):
    """Import a tools/coldfire/ghidradump.py dump directory into out_path. Returns a
    stats dict with skipped=True (and no other keys but name/path/seconds/
    size) when the existing database's image_sha256+DB_VERSION already match
    the dump's manifest and `force` wasn't given."""
    t0 = time.time()
    manifest_path = os.path.join(dump_dir, "manifest.json")
    if not os.path.exists(manifest_path):
        raise SystemExit("cfdb build: no manifest.json in %s" % dump_dir)
    with open(manifest_path) as f:
        manifest = json.load(f)
    if not manifest.get("complete"):
        raise SystemExit(
            "cfdb build: %s is not a complete dump (manifest 'complete' is false)"
            % dump_dir
        )
    sha = manifest.get("image_sha256")
    if not sha:
        raise SystemExit(
            "cfdb build: %s has no image_sha256 in its manifest" % dump_dir
        )
    if name is None:
        name = os.path.basename(os.path.normpath(dump_dir))

    if not force and os.path.exists(out_path):
        existing = read_meta(out_path)
        if existing.get("image_sha256") == sha and existing.get("db_version") == str(
            DB_VERSION
        ):
            build_fts(out_path, dump_dir=dump_dir)
            return {
                "name": name,
                "path": out_path,
                "skipped": True,
                "seconds": time.time() - t0,
                "size": os.path.getsize(out_path),
            }

    xrefs_path = os.path.join(dump_dir, "xrefs.sqlite")
    if not os.path.exists(xrefs_path):
        raise SystemExit("cfdb build: no xrefs.sqlite in %s" % dump_dir)
    src = sqlite3.connect(xrefs_path)
    try:
        functions = src.execute(
            "SELECT entry, name, namespace, size, signature, decomp, disasm, decomp_error "
            "FROM functions ORDER BY entry"
        ).fetchall()
        ranges_by_func = collections.defaultdict(list)
        for func, lo, hi in src.execute("SELECT func, lo, hi FROM function_ranges"):
            ranges_by_func[func].append((lo, hi))
        calls = src.execute(
            "SELECT from_func, to_func, to_addr, site, kind FROM calls"
        ).fetchall()
        data_refs = src.execute(
            "SELECT site, func, to_addr, kind, label, block FROM data_refs"
        ).fetchall()
        strings = (
            src.execute("SELECT addr, text FROM strings").fetchall()
            if _table_exists(src, "strings")
            else []
        )
        symbols = (
            src.execute(
                "SELECT addr, name, namespace, type, is_primary FROM symbols"
            ).fetchall()
            if _table_exists(src, "symbols")
            else []
        )
        typeinfos = (
            src.execute(
                "SELECT addr, kind, name, mangled, bases FROM typeinfos"
            ).fetchall()
            if _table_exists(src, "typeinfos")
            else []
        )
        vtables = (
            src.execute(
                "SELECT addr, class, typeinfo, offset_to_top, slot, target, owner "
                "FROM vtables"
            ).fetchall()
            if _table_exists(src, "vtables")
            else []
        )
    finally:
        src.close()

    facts = {}
    image_file = _find_image(manifest, sha)
    if image_file is not None:
        cffacts = _cffacts()
        facts = cffacts.compute(
            cffacts.Path(image_file),
            cffacts.DEFAULT_BASE,
            {e: ranges_by_func[e] for e, *_ in functions if ranges_by_func.get(e)},
        )

    out_dir = os.path.dirname(os.path.abspath(out_path)) or "."
    os.makedirs(out_dir, exist_ok=True)
    tmp_path = os.path.join(
        out_dir,
        ".%s.tmp-%d-%d"
        % (os.path.basename(out_path), os.getpid(), int(time.time() * 1e6)),
    )
    try:
        db = open_db(tmp_path)
        try:
            db.executemany(
                "INSERT INTO meta VALUES (?,?,?)",
                [
                    (name, "image", name),
                    (name, "image_sha256", sha),
                    (name, "db_version", str(DB_VERSION)),
                    (
                        name,
                        "build_time",
                        time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    ),
                    (name, "ghidra_dump_dir", os.path.abspath(dump_dir)),
                    (name, "ghidra_project", manifest.get("project") or ""),
                    (name, "ghidra_program", manifest.get("program") or ""),
                    (name, "language", manifest.get("language") or ""),
                ],
            )

            func_rows = [
                (
                    name,
                    entry,
                    _function_end(entry, size, ranges_by_func),
                    fname,
                    namespace,
                    size,
                    signature,
                    decomp,
                    disasm,
                    decomp_error,
                )
                for entry, fname, namespace, size, signature, decomp, disasm, decomp_error in functions
            ]
            db.executemany(
                "INSERT INTO functions VALUES (?,?,?,?,?,?,?,?,?,?)", func_rows
            )

            edge_rows = [
                (
                    name,
                    site,
                    from_func,
                    to_func,
                    to_addr,
                    _CALL_KIND.get(kind, "jump"),
                    kind,
                )
                for from_func, to_func, to_addr, site, kind in calls
            ]
            db.executemany("INSERT INTO edges VALUES (?,?,?,?,?,?,?)", edge_rows)

            dataref_rows = [
                (name, site, func, to_addr, kind, label, block)
                for site, func, to_addr, kind, label, block in data_refs
            ]
            db.executemany("INSERT INTO datarefs VALUES (?,?,?,?,?,?,?)", dataref_rows)

            string_rows = [(name, addr, text) for addr, text in strings]
            db.executemany("INSERT INTO strings VALUES (?,?,?)", string_rows)

            symbol_rows = [
                (name, addr, sname, namespace, typ, is_primary)
                for addr, sname, namespace, typ, is_primary in symbols
            ]
            db.executemany("INSERT INTO symbols VALUES (?,?,?,?,?,?)", symbol_rows)

            db.executemany(
                "INSERT INTO func_hash VALUES (?,?,?,?,?,?,?,?)",
                [
                    (
                        name, entry, f["ninsn"], f["nbytes"], f["h_strict"],
                        f["h_loose"], f["h_mnem"], f["mnem_seq"],
                    )
                    for entry, f in facts.items()
                ],
            )  # fmt: skip
            db.executemany(
                "INSERT INTO typeinfos VALUES (?,?,?,?,?,?)",
                [(name, *row) for row in typeinfos],
            )
            db.executemany(
                "INSERT INTO vtables VALUES (?,?,?,?,?,?,?,?)",
                [(name, *row) for row in vtables],
            )

            _write_search_data(db, name, dump_dir)
            db.commit()
        finally:
            db.close()
        os.replace(tmp_path, out_path)
    except BaseException:
        if os.path.exists(tmp_path):
            os.remove(tmp_path)
        raise

    return {
        "name": name,
        "path": out_path,
        "skipped": False,
        "seconds": time.time() - t0,
        "size": os.path.getsize(out_path),
        "n_functions": len(func_rows),
        "n_edges": len(edge_rows),
        "n_datarefs": len(dataref_rows),
        "n_strings": len(string_rows),
        "n_symbols": len(symbol_rows),
    }


def cmd_build(args):
    name = args.name or os.path.basename(os.path.normpath(args.dump_dir))
    out = args.out or coldfire_database_path(name, args.out_dir)
    r = build_database(args.dump_dir, out, name=name, force=args.force)
    artifacts = FirmwareArtifacts(name, "cf")
    if str(artifacts.database) == os.path.abspath(out):
        artifacts.publish_legacy_alias()
    if r["skipped"]:
        print(
            "%-16s SKIPPED (up to date)  %s  %.1fs"
            % (r["name"], r["path"], r["seconds"])
        )
        return 0
    print(
        "%-16s built  %s  %.1fs  %.1f KB  functions=%d edges=%d datarefs=%d strings=%d symbols=%d"
        % (
            r["name"],
            r["path"],
            r["seconds"],
            r["size"] / 1024,
            r["n_functions"],
            r["n_edges"],
            r["n_datarefs"],
            r["n_strings"],
            r["n_symbols"],
        )
    )
    return 0


FTS_VERSION = 3


def fts_path(db_path):
    """Legacy sidecar path for read compatibility."""
    return db_path[: -len(".sqlite")] + ".fts.sqlite"


def _write_search_data(db, name, dump_dir):
    """Populate the text and FTS tables in the caller's transaction."""
    for table in (
        "function_text",
        "fts_meta",
        "fts_names",
        "fts_strings",
        "fts_decomp",
        "string_rows",
    ):
        db.execute(f"DROP TABLE IF EXISTS {table}")
    for sql in (
        "CREATE TABLE function_text (image TEXT, path TEXT, body TEXT, PRIMARY KEY (image, path))",
        "CREATE TABLE fts_meta (key TEXT PRIMARY KEY, value TEXT)",
        "CREATE VIRTUAL TABLE fts_names USING fts5(name, cf_name, klass, tokenize='trigram')",
        "CREATE VIRTUAL TABLE fts_strings USING fts5(text, tokenize='trigram')",
        "CREATE VIRTUAL TABLE fts_decomp USING fts5(body, content='', detail=column, "
        "tokenize=\"unicode61 tokenchars '_'\")",
        "CREATE TABLE string_rows (id INTEGER PRIMARY KEY, addr INTEGER)",
        FUNCTION_NAMES_SQL,
    ):
        db.execute(sql)
    names = _image_names(name)
    classes: dict[int, str] = {}
    for target, cls in db.execute(
        "SELECT target, class FROM vtables WHERE image=? ORDER BY addr, slot", (name,)
    ):
        classes.setdefault(target, cls)
    functions = db.execute(
        "SELECT entry, name, decomp_path, disasm_path FROM functions WHERE image=? ORDER BY entry",
        (name,),
    ).fetchall()
    db.execute(
        "DELETE FROM function_names WHERE image=? AND source='cf_names'", (name,)
    )
    db.executemany(
        "INSERT OR REPLACE INTO function_names VALUES (?,?,?,?)",
        (
            (name, entry, names[entry], "cf_names")
            for entry, *_ in functions
            if entry in names
        ),
    )
    db.executemany(
        "INSERT INTO fts_names(rowid, name, cf_name, klass) VALUES (?,?,?,?)",
        (
            (entry, fname, names.get(entry, ""), classes.get(entry, ""))
            for entry, fname, _, _ in functions
        ),
    )
    strings = db.execute(
        "SELECT addr, text FROM strings WHERE image=? ORDER BY rowid", (name,)
    ).fetchall()
    db.executemany(
        "INSERT INTO fts_strings(rowid, text) VALUES (?,?)",
        ((i, t) for i, (_, t) in enumerate(strings)),
    )
    db.executemany(
        "INSERT INTO string_rows VALUES (?,?)",
        ((i, addr) for i, (addr, _) in enumerate(strings)),
    )

    def texts():
        if dump_dir:
            for entry, _, decomp, disasm in functions:
                for rel in (decomp, disasm):
                    if not rel:
                        continue
                    try:
                        with open(os.path.join(dump_dir, rel), errors="replace") as fh:
                            body = fh.read()
                    except FileNotFoundError:
                        continue
                    yield entry, rel, body, rel == decomp

    n_docs = 0
    for entry, rel, body, is_decomp in texts():
        db.execute(
            "INSERT OR IGNORE INTO function_text VALUES (?,?,?)", (name, rel, body)
        )
        if is_decomp:
            db.execute(
                "INSERT INTO fts_decomp(rowid, body) VALUES (?,?)", (entry, body)
            )
            n_docs += 1
    db.executemany(
        "INSERT INTO fts_meta VALUES (?,?)",
        [("version", str(FTS_VERSION)), ("decomp_files", str(n_docs))],
    )


def build_fts(db_path, force=False, dump_dir=None):
    """Publish function texts and FTS5 indexes inside an existing ColdFire database."""
    meta = read_meta(db_path)
    if not meta.get("image"):
        raise ValueError(f"{db_path}: missing ColdFire database")
    dump_dir = dump_dir or meta.get("ghidra_dump_dir")
    if not dump_dir or not os.path.isdir(dump_dir):
        candidate = os.path.join("out", "ghidra", meta["image"])
        manifest = os.path.join(candidate, "manifest.json")
        if os.path.isfile(manifest):
            with open(manifest) as fh:
                info = json.load(fh)
            if info.get("complete") and info.get("image_sha256") == meta.get(
                "image_sha256"
            ):
                dump_dir = candidate
    with closing(sqlite3.connect(db_path)) as db, db:
        db.execute("BEGIN IMMEDIATE")
        if not force and _table_exists(db, "fts_meta"):
            version = db.execute(
                "SELECT value FROM fts_meta WHERE key='version'"
            ).fetchone()
            if version == (str(FTS_VERSION),):
                return db_path
        if not dump_dir or not os.path.isdir(dump_dir):
            raise ValueError(
                f"{db_path}: use the original Ghidra dump to embed function text"
            )
        _write_search_data(db, meta["image"], dump_dir)
    return db_path


def _image_names(name):
    """tools/coldfire/cf_names*.py names for an image name like dt2-1.17-emac."""
    import importlib

    short = name[: -len("-emac")] if name.endswith("-emac") else name
    mod = {
        "dt2-1.16": "tools.coldfire.cf_names",
        "dt2-1.17": "tools.coldfire.cf_names_dt2_1_17",
    }.get(short)
    if mod is None:
        return {}
    try:
        return dict(importlib.import_module(mod).NAME_BY_ADDR)
    except ImportError:
        return {}


def cmd_fts(args):
    path = args.db or coldfire_database_path(args.name, args.db_dir)
    t = time.time()
    try:
        out = build_fts(path, force=args.force)
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc
    print("%s (%.1f s, %.1f MB)" % (out, time.time() - t, os.path.getsize(out) / 1e6))
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    b = sub.add_parser(
        "build", help="build a ColdFire image database from a Ghidra dump"
    )
    b.add_argument("dump_dir", help="e.g. out/ghidra/dt2-1.16-emac")
    b.add_argument(
        "--name", help="image name; default is the dump directory's own name"
    )
    b.add_argument(
        "--out", help="output .sqlite path (defaults to the firmware database)"
    )
    b.add_argument("--out-dir", default=DEFAULT_DB_DIR)
    b.add_argument(
        "--force",
        action="store_true",
        help="rebuild even if sha256+DB_VERSION already match",
    )

    f = sub.add_parser(
        "fts", help="embed function texts and FTS5 indexes in a database"
    )
    f.add_argument("name", help="e.g. dt2-1.17-emac")
    f.add_argument("--db")
    f.add_argument("--db-dir", default=DEFAULT_DB_DIR)
    f.add_argument("--force", action="store_true")

    args = ap.parse_args(argv)
    if args.cmd == "build":
        return cmd_build(args)
    if args.cmd == "fts":
        return cmd_fts(args)
    return 1


if __name__ == "__main__":
    sys.exit(main())
