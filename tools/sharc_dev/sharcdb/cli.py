"""Command line: build, import-ghidra and analyze commands."""

from __future__ import annotations

import argparse
import os
import shutil
import sqlite3
import time

from tools.firmware.artifacts import FirmwareArtifacts, database_path
from tools.sharc_dev import sharcldr

from .analysis import analyze_image
from .build import build_database
from .ghidra import build_ghidra_database
from .schema import DB_VERSION, read_meta


def cmd_import_ghidra(args):
    name = args.name or os.path.basename(os.path.normpath(args.dump_dir))
    out = args.out or os.path.join(args.out_dir, name + ".sqlite")
    try:
        r = build_ghidra_database(args.dump_dir, out, name=name, force=args.force)
    except SystemExit as exc:
        print("FAILED  %s  %s" % (name, exc))
        return 1
    if r["skipped"]:
        print(
            "%-16s SKIPPED (up to date)  %s  %.1fs"
            % (r["name"], r["path"], r["seconds"])
        )
        return 0
    print(
        "%-16s built  %s  %.1fs  %.1f KB  functions=%d insn=%d edges=%d dataref=%d decomp=%d"
        % (
            r["name"],
            r["path"],
            r["seconds"],
            r["size"] / 1024,
            r["n_functions"],
            r["n_insn"],
            r["n_edges"],
            r["n_dataref"],
            r["n_decomp"],
        )
    )
    a = r.get("analyze") or {}
    if a:
        print(
            "    analyze  %.1fs  roots=%d reach=%d callgraph=%d idom=%d loops=%d unentered=%d ptr=%d"
            % (
                a["seconds"],
                a["n_roots"],
                a["n_reach"],
                a["n_callgraph"],
                a["n_idom"],
                a["n_loops"],
                a["n_unentered"],
                a.get("n_ptr", 0),
            )
        )
    return 0


def _build_one_cli(blob, out, name, min_depth, blocks, force):
    try:
        result = build_database(
            blob, out, name=name, min_depth=min_depth, blocks=blocks, force=force
        )
        artifacts = FirmwareArtifacts(name, "sharc")
        if str(artifacts.database) == os.path.abspath(out):
            artifacts.publish_legacy_alias()
        return result
    except Exception as exc:
        return {
            "name": name,
            "path": out,
            "skipped": False,
            "seconds": 0,
            "size": 0,
            "error": str(exc),
        }


def _int_list(text):
    return tuple(int(x, 0) for x in text.split(","))


def cmd_build(args):
    jobs = []
    for blob in args.blobs:
        single = len(args.blobs) == 1
        name = (
            args.name
            if (args.name and single)
            else (
                os.path.basename(os.path.dirname(os.path.abspath(blob)))
                or os.path.splitext(os.path.basename(blob))[0]
            )
        )
        out = (
            args.out
            if (args.out and single)
            else database_path(name, "sharc", args.out_dir)
        )
        jobs.append((blob, out, name))

    results = []
    if len(jobs) == 1:
        blob, out, name = jobs[0]
        results.append(
            _build_one_cli(blob, out, name, args.min_depth, args.blocks, args.force)
        )
    else:
        import concurrent.futures

        workers = args.jobs or min(len(jobs), os.cpu_count() or 1)
        with concurrent.futures.ProcessPoolExecutor(max_workers=workers) as ex:
            futs = [
                ex.submit(
                    _build_one_cli,
                    blob,
                    out,
                    name,
                    args.min_depth,
                    args.blocks,
                    args.force,
                )
                for blob, out, name in jobs
            ]
            results = [fut.result() for fut in futs]

    ok = True
    for r in results:
        if r.get("error"):
            ok = False
            print("FAILED  %-16s %s" % (r["name"], r["error"]))
        elif r["skipped"]:
            print(
                "%-16s SKIPPED (up to date)  %s  %.1fs"
                % (r["name"], r["path"], r["seconds"])
            )
        else:
            print(
                "%-16s built  %s  %.1fs  %.1f KB  insn=%d functions=%d edges=%d literals=%d "
                "mem_access=%d func_hash=%d bblocks=%d succ=%d dataref=%d regdef=%d reguse=%d"
                % (
                    r["name"],
                    r["path"],
                    r["seconds"],
                    r["size"] / 1024,
                    r.get("n_insn", 0),
                    r.get("n_functions", 0),
                    r.get("n_edges", 0),
                    r.get("n_literals", 0),
                    r.get("n_mem_access", 0),
                    r.get("n_func_hash", 0),
                    r.get("n_bblocks", 0),
                    r.get("n_succ", 0),
                    r.get("n_dataref", 0),
                    r.get("n_regdef", 0),
                    r.get("n_reguse", 0),
                )
            )
            a = r.get("analyze") or {}
            if a:
                print(
                    "    analyze  %.1fs  roots=%d reach=%d callgraph=%d idom=%d loops=%d unentered=%d ptr=%d"
                    % (
                        a["seconds"],
                        a["n_roots"],
                        a["n_reach"],
                        a["n_callgraph"],
                        a["n_idom"],
                        a["n_loops"],
                        a["n_unentered"],
                        a.get("n_ptr", 0),
                    )
                )
            if r.get("unknown_form_counts"):
                print(
                    "    unknown regdef/reguse forms: "
                    + ", ".join(
                        "%s=%d" % kv for kv in sorted(r["unknown_form_counts"].items())
                    )
                )
    return 0 if ok else 1


def cmd_analyze(args):
    """Re-run analyze_image() over one or more already-built databases,
    in place (temp-copy + os.replace, same atomicity as a build)."""
    ok = True
    for path in args.dbs:
        if not os.path.exists(path):
            print("FAILED  %s  no such file" % path)
            ok = False
            continue
        # Keep the compatibility link when re-analyzing a database in the new layout.
        if os.path.islink(path):
            resolved = os.path.realpath(path)
            if os.path.basename(resolved) == "firmware.sqlite":
                path = resolved
        meta = read_meta(path)
        name = meta.get("image")
        if not name:
            print("FAILED  %s  not a sharcdb database (no meta.image)" % path)
            ok = False
            continue
        if meta.get("db_version") != str(DB_VERSION):
            print(
                "FAILED  %s  database schema has changed; rebuild from its source"
                % path
            )
            ok = False
            continue

        mem, blocks = None, None
        blob_path = meta.get("blob_path")
        if blob_path and os.path.exists(blob_path):
            with open(blob_path, "rb") as fh:
                data = fh.read()
            blocks = sharcldr.parse_blocks(data)
            mem = sharcldr.LoadedMemory.from_stream(data, blocks)
        else:
            print(
                "    %-16s blob not found (%s); loader_entry/code_pointer_array/"
                "interrupt_vector roots skipped" % (name, blob_path)
            )

        out_dir = os.path.dirname(os.path.abspath(path)) or "."
        tmp_path = os.path.join(
            out_dir,
            ".%s.tmp-%d-%d"
            % (os.path.basename(path), os.getpid(), int(time.time() * 1e6)),
        )
        try:
            shutil.copy2(path, tmp_path)
            db = sqlite3.connect(tmp_path)
            try:
                stats = analyze_image(db, name, mem=mem, blocks=blocks)
                db.commit()
            finally:
                db.close()
            os.replace(tmp_path, path)
        except BaseException:
            if os.path.exists(tmp_path):
                os.remove(tmp_path)
            raise
        print(
            "%-16s analyzed  %s  %.1fs  roots=%d reach=%d callgraph=%d idom=%d loops=%d unentered=%d ptr=%d"
            % (
                name,
                path,
                stats["seconds"],
                stats["n_roots"],
                stats["n_reach"],
                stats["n_callgraph"],
                stats["n_idom"],
                stats["n_loops"],
                stats["n_unentered"],
                stats.get("n_ptr", 0),
            )
        )
    return 0 if ok else 1


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    b = sub.add_parser("build", help="build one or more image databases")
    b.add_argument("blobs", nargs="+", help="section_7_*.bin path(s)")
    b.add_argument(
        "--out", help="output .sqlite path (only meaningful with a single blob)"
    )
    b.add_argument(
        "--out-dir", default="out/sharcdb", help="directory for multi-blob builds"
    )
    b.add_argument(
        "--name",
        help="image name (only meaningful with a single blob); "
        "default is the blob's parent directory name",
    )
    b.add_argument("--min-depth", type=int, default=8)
    b.add_argument(
        "--blocks",
        type=_int_list,
        help="override the code-block index list (comma-separated)",
    )
    b.add_argument(
        "--force",
        action="store_true",
        help="rebuild even if source, analysis and options match",
    )
    b.add_argument(
        "--jobs",
        type=int,
        default=0,
        help="parallel worker processes for multiple blobs (default: one per blob)",
    )

    a = sub.add_parser(
        "analyze", help="re-run the roots/reach/callgraph/idom/loops/unentered pass"
    )
    a.add_argument("dbs", nargs="+", help="out/sharcdb/<image>.sqlite path(s)")

    g = sub.add_parser(
        "import-ghidra",
        help="import a tools/coldfire/ghidradump.py dump as a ColdFire image database",
    )
    g.add_argument("dump_dir", help="e.g. out/ghidra/dt2-1.16-emac")
    g.add_argument(
        "--name", help="image name; default is the dump directory's own name"
    )
    g.add_argument(
        "--out", help="output .sqlite path (default: --out-dir/<name>.sqlite)"
    )
    g.add_argument("--out-dir", default="out/sharcdb")
    g.add_argument(
        "--force",
        action="store_true",
        help="rebuild even if source, analysis and options match",
    )
    args = ap.parse_args(argv)

    if args.cmd == "build":
        return cmd_build(args)
    if args.cmd == "import-ghidra":
        return cmd_import_ghidra(args)
    if args.cmd == "analyze":
        return cmd_analyze(args)
    return 1
