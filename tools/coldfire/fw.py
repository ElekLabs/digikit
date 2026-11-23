#!/usr/bin/env python3
"""fw.py: one compact query CLI over the firmware databases, for agents.

    uv run python -m tools.coldfire.fw fn 0x401be1be -i dt2-1.17
    uv run python -m tools.coldfire.fw batch queries.txt -i dt2-1.17      # or '-' for stdin

Select a release with --device and --firmware, or use an image name such as
dt2-1.17. --core selects the main or DSP program from the device profile.
Without a core selector, the address, declaration name or --cpu selects the
instruction set. Set FW_IMAGE to skip
-i. Text output is capped at --max lines (default 60; 0 = no cap) with a
"... N more" tail. The stores command instead returns uncapped JSON.

Subcommands: fn decomp disasm callers callees xref str class path port sql
search atlas catalog dossier stores build batch. In a batch file each line is one command line (without
"fw.py"); lines starting with # are comments; every result block starts with
"## <line>". The call graph is cached on disk (out/cache/fw/), keyed by the
database hash, so a cold process only pays for opening SQLite.

Ghidra's call/data-ref tables miss code outside functions: an empty callers
list is not evidence of dead code (AGENTS.md; tools/coldfire/refscan.py confirms).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import sqlite3
import sys
from contextlib import closing
from typing import Any

from tools.coldfire import fw_query as coldfire
from tools.coldfire.fw_query import (
    CF as CF,
)
from tools.coldfire.fw_query import (
    _bases as _bases,
)
from tools.coldfire.fw_query import (
    anchors_for as anchors_for,
)
from tools.coldfire.fw_query import (
    call_setup as call_setup,
)
from tools.coldfire.fw_query import (
    hidden_return_ptr as hidden_return_ptr,
)
from tools.coldfire.fw_query import (
    parse_disasm as parse_disasm,
)
from tools.coldfire.fw_query import (
    tri_query as tri_query,
)
from tools.firmware.query_db import (
    ATLAS_DIR as ATLAS_DIR,
)
from tools.firmware.query_db import (
    CF_DIR as CF_DIR,
)
from tools.firmware.query_db import (
    ROOT as ROOT,
)
from tools.firmware.query_db import (
    SHARC_DIR as SHARC_DIR,
)
from tools.firmware.query_db import (
    FwError as FwError,
)
from tools.firmware.query_db import (
    _has,
    clip,
)
from tools.firmware.query_db import (
    capped as capped,
)
from tools.firmware.query_db import (
    open_ro as open_ro,
)
from tools.firmware.query_db import (
    parse_addr as parse_addr,
)
from tools.firmware.query_db import (
    short_image as short_image,
)
from tools.firmware.query_target import Program, dsp_program, select_core, select_image
from tools.sharc_dev import fw_query as sharc
from tools.sharc_dev.fw_query import Sharc as Sharc
from tools.sharc_dev.fw_query import cmd_stores as cmd_stores

DEFAULT_MAX = 60
DOSSIER_MAX = 180
CF_BASE = 0x10000000


Image = CF | Sharc


def want_sharc(token: str | None, cpu: str | None) -> bool:
    if cpu:
        return cpu == "sharc"
    if token:
        if token.startswith("dsp."):
            return True
        a = parse_addr(token)
        if a is not None:
            return a < CF_BASE
        if re.match(r"(?i)fun_[0-9a-f]{6}$", token):
            return True
    return False


class Session:
    """Opened databases for one process (batch shares it)."""

    def __init__(self) -> None:
        self.imgs: dict[tuple[str, str], Image] = {}

    def get(
        self,
        image: str | None,
        token: str | None = None,
        cpu: str | None = None,
        *,
        core: str | None = None,
    ) -> Image:
        image = image or os.environ.get("FW_IMAGE")
        if not image:
            raise FwError("select --device, pass --image, or set FW_IMAGE")
        program = (
            select_core(image, core, cpu)
            if core
            else Program(
                short_image(image), "sharc" if want_sharc(token, cpu) else "cf"
            )
        )
        if program not in self.imgs:
            self.imgs[program] = (
                Sharc(program.image) if program.cpu == "sharc" else CF(program.image)
            )
        return self.imgs[program]


def cmd_callers(img: Image, token: str) -> list[str]:
    entry = img.resolve(token)
    rows = img.callers(entry)
    out = ["%s  %d call sites" % (img.label(entry), len(rows))]
    last = None
    for fn, site, kind in rows:
        tag = img.label(fn) if fn is not None else "(no function)"
        out.append(
            "  %s %s @%0*x"
            % (kind[:4], tag if tag != last else '  "', img.address_width, site)
        )
        last = tag
    return out


def cmd_callees(img: Image, token: str) -> list[str]:
    entry = img.resolve(token)
    rows = img.callees(entry)
    out = ["%s  %d callees" % (img.label(entry), len(rows))]
    out.extend("  %s %s" % (k[:4], img.label(c)) for c, k in rows)
    return out


def cmd_path(img: Image, a: str, b: str) -> list[str]:
    ea, eb = img.resolve(a), img.resolve(b)
    g = img.graph()
    p = g.shortest(ea, eb)
    out = ["path %s -> %s (graph %s)" % (img.label(ea), img.label(eb), img.graph_state)]
    if p is None:
        out.append("  no call path (indirect calls through vtables are not edges)")
        return out
    out.append("  %d hops" % (len(p) - 1))
    out.extend(
        "  %s%s" % ("" if i == 0 else "-> ", img.label(n)) for i, n in enumerate(p)
    )
    return out


def cmd_sql(db: sqlite3.Connection, query: str) -> list[str]:
    if not re.match(r"(?is)\s*(select|with|pragma|explain)\b", query):
        raise FwError("sql is read-only: SELECT/WITH only")
    cur = db.execute(query)
    cols = [d[0] for d in cur.description or []]
    out = ["\t".join(cols)]
    for row in cur:
        out.append("\t".join(_cell(v) for v in row))
    return out


def _cell(v: Any) -> str:
    if v is None:
        return "-"
    if isinstance(v, int) and v >= 0x100000:
        return "0x%x" % v
    return clip(str(v), 120)


def cmd_catalog(
    image: str,
    term: str,
    tag: str | None,
    top: int,
    grade: bool,
    group: int | None = None,
    groups: bool = False,
    cpu: str = "cf",
) -> list[str]:
    """Rows of the firmware database's catalog, best score first."""
    short = short_image(image)
    if cpu == "sharc":
        path = os.path.join(SHARC_DIR, short + ".sqlite")
        build = "uv run python -m tools.sharc_dev.sharc_catalog %s OTHER" % short
    else:
        build = "uv run python -m tools.coldfire.atlas catalog %s" % short
        path = coldfire.atlas_path(short, "catalog")
    con = open_ro(path)
    if not _has(con, "catalog"):
        raise FwError(
            "no catalog for %s: run %s (e.g. dt2-1.17 dn2-1.12)" % (short, build)
        )
    if (groups or group is not None) and not _has(con, "catalog_groups"):
        raise FwError(
            "no catalog groups for %s: re-run %s (e.g. dt2-1.17 dn2-1.12)"
            % (short, build)
        )
    if groups:
        grows = con.execute(
            "SELECT id, size, anchored, grade, top_classes, top_strings, top_tags, best "
            "FROM catalog_groups ORDER BY size DESC, id LIMIT %d" % top
        ).fetchall()
        out = ["catalog groups %s: %d shown" % (short, len(grows))]
        for gid, size, anc, gr, tcl, tst, ttg, best in grows:
            out.append(
                "grp=%d size=%d anchored=%d grade=%d best=%08x classes=%s tags=%s%s"
                % (
                    gid,
                    size,
                    anc,
                    gr,
                    best,
                    tcl or "-",
                    ttg or "-",
                    " strings=" + repr(tst) if tst else "",
                )
            )
        return out
    where = ["1"]
    args: list[Any] = []
    if group is not None:
        where.append('"group"=?')
        args.append(group)
    if term:
        cls_col, hint_col = ("role", "hints") if cpu == "sharc" else ("class", "hint")
        where.append(
            "(name LIKE ?%d OR %s LIKE ?%d OR %s LIKE ?%d OR tags LIKE ?%d OR anchors LIKE ?%d)"
            % (
                len(args) + 1,
                cls_col,
                len(args) + 1,
                hint_col,
                len(args) + 1,
                len(args) + 1,
                len(args) + 1,
            )
        )
        args.append("%" + term + "%")
    if tag:
        where.append("(',' || tags) LIKE ?%d" % (len(args) + 1))
        args.append("%," + tag + "%")
    if grade:
        where.append("anchor_grade=1")
    sql = "FROM catalog WHERE " + " AND ".join(where)
    total = con.execute("SELECT count(*) " + sql, args).fetchone()[0]
    if cpu == "sharc":
        drows = con.execute(
            "SELECT addr, score, anchor_grade, name, role, tags, callers, hints, pair_addr, "
            'pair_method, rule_kind, rule_ordinal, anchors, "group" '
            + sql
            + " ORDER BY score DESC, addr LIMIT %d" % top,
            args,
        ).fetchall()
        out = [
            "catalog %s (sharc): %d rows match, %d shown" % (short, total, len(drows))
        ]
        for a, sc, g, name, role, tags, nc, hints, pa, pm, rk, ro, anch, grp in drows:
            rule = rk if ro is None else "%s#%d" % (rk, ro)
            out.append(
                "%06x %5.2f %s %s (%s) grp=%d  %s  callers=%d rule=%s pair=%s%s%s"
                % (
                    a,
                    sc,
                    "G" if g else "-",
                    name,
                    role,
                    grp,
                    tags,
                    nc,
                    rule,
                    "%06x/%s" % (pa, pm) if pa is not None else "-",
                    " " + repr(hints) if hints else "",
                    " anchored=" + anch if anch else "",
                )
            )
        return out
    rows = con.execute(
        "SELECT addr, score, anchor_grade, name, class, tags, ui_callers, hint, pair_addr, "
        'pair_method, rule_kind, rule_ordinal, anchors, work_name, "group", caller_classes '
        + sql
        + " ORDER BY score DESC, addr LIMIT %d" % top,
        args,
    ).fetchall()
    out = ["catalog %s: %d rows match, %d shown" % (short, total, len(rows))]
    for a, sc, g, name, cls, tags, uic, hint, pa, pm, rk, ro, anch, wn, grp, by in rows:
        rule = rk if ro is None else "%s#%d" % (rk, ro)
        out.append(
            "%08x %5.2f %s %s%s%s grp=%d  %s  ui=%d rule=%s pair=%s%s%s%s"
            % (
                a,
                sc,
                "G" if g else "-",
                name,
                " [%s]" % wn if wn else "",
                " (%s)" % cls if cls else "",
                grp,
                tags,
                uic,
                rule,
                "%08x/%s" % (pa, pm) if pa is not None else "-",
                " " + repr(hint) if hint else "",
                " anchored=" + anch if anch else "",
                " by=" + by if by and not cls else "",
            )
        )
    return out


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="fw.py", description=__doc__.splitlines()[0], add_help=True
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    def common(sp: argparse.ArgumentParser) -> argparse.ArgumentParser:
        sp.add_argument(
            "-i", "--image", help="e.g. dt2-1.17, dn2-1.12 (default $FW_IMAGE)"
        )
        sp.add_argument("--device", help="device short name or product name")
        sp.add_argument(
            "--firmware", metavar="VERSION", help="firmware version for --device"
        )
        sp.add_argument(
            "--core", choices=("main", "dsp"), help="main OS or DSP program"
        )
        sp.add_argument(
            "--cpu",
            choices=("cf", "sharc"),
            help="override the CPU selected from the address or name",
        )
        sp.add_argument(
            "--max",
            type=int,
            default=None,
            help="line cap (default %d, 0 = none)" % DEFAULT_MAX,
        )
        return sp

    for name, helptext in (
        ("fn", "function summary"),
        ("decomp", "decompiled C"),
        ("disasm", "disassembly"),
        ("callers", "call sites into a function"),
        ("callees", "functions a function calls"),
        ("xref", "everything that references an address"),
    ):
        sp = common(sub.add_parser(name, help=helptext))
        sp.add_argument(
            "target", help="address (hex), function name or declared DSP name"
        )
    sp = common(
        sub.add_parser(
            "cost", help="static SHARC instruction count with DO-loop trip counts"
        )
    )
    sp.add_argument("target", help="address (hex) or name")
    sp.add_argument(
        "--vars",
        help="values for trip symbols: NAME=V (entry function's register) or"
        " NAME@FN=V (one function), e.g. R2=32,R12@1cd286=16",
    )
    sp.add_argument(
        "--vars-all",
        help="NAME=V,...: set a register name in every function, e.g. R2=32",
    )
    sp.add_argument("--no-calls", action="store_true", help="do not add callees")
    sp = common(
        sub.add_parser(
            "stores",
            help="read-only SHARC CALL-reachable store operands as uncapped JSON",
        )
    )
    sp.add_argument(
        "roots", nargs="+", help="explicit root PCs (hex), not inferred task roots"
    )
    sp.add_argument("--expected-image-sha256")
    sp.add_argument(
        "--image-blob", help="explicit blob path override; bytes must match DB SHA-256"
    )
    sp = common(sub.add_parser("str", help="strings containing TEXT and who uses them"))
    sp.add_argument("text", nargs="+")
    sp = common(sub.add_parser("class", help="RTTI class: bases, derived, vtable"))
    sp.add_argument("name")
    sp = common(sub.add_parser("path", help="shortest call path A -> B"))
    sp.add_argument("a")
    sp.add_argument("b")
    sp = common(sub.add_parser("port", help="carry a function between versions"))
    sp.add_argument("name")
    sp.add_argument("--from", dest="src", required=True)
    sp.add_argument("--to", dest="dst", required=True)
    sp = common(sub.add_parser("sql", help="read-only SQL"))
    sp.add_argument("query")
    sp.add_argument(
        "--atlas",
        action="store_true",
        help="query the atlas tables (legacy ColdFire atlas files are supported)",
    )
    sp = common(
        sub.add_parser(
            "search", help="search symbol names, annotations, or text in firmware"
        )
    )
    sp.add_argument("term", nargs="+")
    sp.add_argument(
        "--in", dest="where", help="comma-separated sources (see query guide)"
    )
    sp = common(sub.add_parser("atlas", help="semantic lookup for ColdFire or SHARC"))
    sp.add_argument("term", nargs="+")
    sp = common(
        sub.add_parser(
            "catalog", help="candidate anchor functions (atlas.py catalog), best first"
        )
    )
    sp.add_argument("term", nargs="*", help="substring of name, class, hint or tags")
    sp.add_argument("--tag", help="tag prefix, e.g. near:cf.menu, role:model, term:fx")
    sp.add_argument("--top", type=int, default=30)
    sp.add_argument("--grade", action="store_true", help="only anchor_grade=1")
    sp.add_argument("--group", type=int, help="only rows of this group id")
    sp.add_argument(
        "--groups", action="store_true", help="list the groups (top N by size)"
    )
    sp = common(
        sub.add_parser(
            "dossier",
            help="function evidence, anchors and an optional cross-image pair",
        )
    )
    sp.add_argument("target", help="address (hex), function name or declared DSP name")
    sp.add_argument(
        "--other", help="the other image, e.g. dn2-1.12: find the pair there"
    )
    sp.add_argument("--callers", type=int, default=3, help="callers to show")
    sp.add_argument(
        "--decomp", type=int, default=12, help="ColdFire decompiled lines to show"
    )
    sp.add_argument(
        "--listing", type=int, default=8, help="SHARC listing lines per end"
    )
    sp = common(
        sub.add_parser(
            "build", help="build both images' canonical databases and catalogs"
        )
    )
    sp.add_argument("--other", help="second device image to build and pair")
    sp = common(sub.add_parser("batch", help="run many queries from FILE or - (stdin)"))
    sp.add_argument("file")
    return p


def run(
    sess: Session,
    args: argparse.Namespace,
    default_image: str | None = None,
    default_core: str | None = None,
) -> str:
    c = args.cmd
    cap = (
        (DOSSIER_MAX if c == "dossier" else DEFAULT_MAX)
        if args.max is None
        else args.max
    )
    image = select_image(args.image, args.device, args.firmware) or default_image
    core = args.core or default_core

    def program(token: str | None = None, cpu: str | None = None) -> Image:
        return sess.get(image, token, cpu or args.cpu, core=core)

    if c == "build":
        build_images(image or os.environ.get("FW_IMAGE"), args.other, core, args.cpu)
        return "Firmware databases and catalogs built."
    if c == "batch":
        raise FwError("batch cannot be nested")
    if c == "dossier":
        img = program(args.target)
        other = (
            sess.get(args.other, cpu=None if core else img.cpu, core=core)
            if args.other
            else None
        )
        if other is not None and other.cpu != img.cpu:
            raise FwError("dossier pairing requires the same instruction set")
        if isinstance(img, Sharc):
            assert other is None or isinstance(other, Sharc)
            lines = sharc.cmd_dossier(
                img, args.target, other, args.callers, args.listing
            )
        else:
            assert other is None or isinstance(other, CF)
            lines = coldfire.cmd_dossier(
                img, args.target, other, args.callers, args.decomp
            )
        return capped(lines, cap)
    if c == "stores":
        if args.cpu not in (None, "sharc"):
            raise FwError("stores supports SHARC databases only")
        img = program(cpu="sharc")
        assert isinstance(img, Sharc)
        return json.dumps(
            cmd_stores(img, args.roots, args.expected_image_sha256, args.image_blob),
            sort_keys=True,
            indent=2,
        )
    if c == "sql":
        img = program()
        if args.atlas and isinstance(img, CF):
            with closing(open_ro(coldfire.atlas_path(img.image))) as db:
                return capped(cmd_sql(db, args.query), cap)
        return capped(cmd_sql(img.db, args.query), cap)
    if c == "search":
        term = " ".join(args.term)
        img = program(term if term.startswith("dsp.") else None)
        where = args.where
        sources = where.split(",") if where else None
        search_lines: list[str] = []
        if sources is not None and "catalog" in sources:
            search_lines.extend(
                cmd_catalog(img.image, term, None, cap or -1, False, cpu=img.cpu)
            )
            sources = [source for source in sources if source != "catalog"]
            if not sources:
                return capped(search_lines, cap)
            where = ",".join(sources)
        if isinstance(img, Sharc):
            search_lines.extend(sharc.cmd_search(img, term, where, cap))
        else:
            search_lines.extend(
                coldfire.cmd_search(
                    img, term, where or "names,strings,decomp,atlas", cap
                )
            )
        return capped(search_lines, cap)
    if c == "atlas":
        img = program()
        term = " ".join(args.term)
        if isinstance(img, Sharc):
            return capped(sharc.cmd_atlas(img, term), cap)
        if img.image.endswith("-peer"):
            return capped(cmd_catalog(img.image, term, None, 15, False), cap)
        return capped(coldfire.cmd_atlas(img.image, term), cap)
    if c == "catalog":
        img = program()
        return capped(
            cmd_catalog(
                img.image,
                " ".join(args.term),
                args.tag,
                args.top,
                args.grade,
                args.group,
                args.groups,
                img.cpu,
            ),
            cap,
        )
    if c == "port":
        src = sess.get(args.src, args.name, args.cpu, core=core)
        dst = sess.get(args.dst, cpu=None if core else src.cpu, core=core)
        if isinstance(src, Sharc) and isinstance(dst, Sharc):
            return capped(sharc.cmd_port(src, dst, args.name), cap)
        if isinstance(src, CF) and isinstance(dst, CF):
            return capped(coldfire.cmd_port(src, dst, args.name), cap)
        raise FwError("port requires the same instruction set on both images")
    if c == "str":
        img = program()
        term = " ".join(args.text)
        if isinstance(img, Sharc):
            return capped(sharc.cmd_str(img, term), cap)
        return capped(coldfire.cmd_str(img, term), cap)
    if c == "class":
        img = program()
        if not isinstance(img, CF):
            raise FwError("class queries ColdFire RTTI; use atlas for SHARC roles")
        return capped(coldfire.cmd_class(img, args.name), cap)
    if c == "path":
        return capped(cmd_path(program(args.a), args.a, args.b), cap)
    if c == "cost":
        img = program(args.target)
        if not isinstance(img, Sharc):
            return "error: cost is SHARC only"
        return capped(
            sharc.cmd_cost(
                img, args.target, args.vars, not args.no_calls, args.vars_all
            ),
            cap,
        )
    img = program(args.target)
    if c == "callers":
        return capped(cmd_callers(img, args.target), cap)
    if c == "callees":
        return capped(cmd_callees(img, args.target), cap)
    if isinstance(img, Sharc):
        command = {
            "fn": sharc.cmd_fn,
            "decomp": sharc.cmd_decomp,
            "disasm": sharc.cmd_disasm,
            "xref": sharc.cmd_xref,
        }[c]
        return capped(command(img, args.target), cap)
    cf_command = {
        "fn": coldfire.cmd_fn,
        "decomp": coldfire.cmd_decomp,
        "disasm": coldfire.cmd_disasm,
        "xref": coldfire.cmd_xref,
    }[c]
    return capped(cf_command(img, args.target), cap)


def build_images(
    image: str | None, other: str | None, core: str | None, cpu: str | None
) -> None:
    if image is None:
        raise FwError("select --device and --firmware, or pass --image")
    if other and short_image(image) == short_image(other):
        raise FwError("--other must be a different image")
    if core or cpu == "sharc":
        roles = [core or "dsp"]
    elif short_image(image).endswith("-peer"):
        roles = ["dsp"]
    else:
        dsp = dsp_program(image)
        roles = ["main"]
        if dsp and (cpu is None or cpu == dsp.cpu):
            roles.append("dsp")

    plans: list[tuple[str, Program, Program | None]] = []
    for role in roles:
        selected = select_core(image, role, cpu)
        counterpart = None
        if other:
            counterpart = (
                select_core(other, role) if role == "main" else dsp_program(other)
            )
        if counterpart is not None and counterpart.cpu != selected.cpu:
            if core == "dsp":
                raise FwError("DSP catalog pairing requires matching instruction sets")
            counterpart = None
        if selected.cpu == "sharc" and counterpart is None:
            raise FwError("SHARC catalog generation needs --other with a SHARC image")
        plans.append((role, selected, counterpart))
    for role, selected, counterpart in plans:
        if selected.cpu == "sharc":
            assert counterpart is not None
            sharc.build_images(selected.image, counterpart.image)
        else:
            coldfire.build_images(
                selected.image,
                counterpart.image if counterpart else None,
                peer=role == "dsp",
            )


def run_line(
    sess: Session,
    argv: list[str],
    default_image: str | None,
    default_max: int | None,
    default_core: str | None = None,
    default_cpu: str | None = None,
) -> str:
    parser = build_parser()
    try:
        args = parser.parse_args(argv)
    except SystemExit:
        return "error: bad arguments: " + " ".join(argv)
    if args.max is None and args.cmd != "dossier":
        args.max = default_max
    args.cpu = args.cpu or default_cpu
    try:
        return run(sess, args, default_image, default_core)
    except FwError as e:
        return "error: %s" % e
    except sqlite3.Error as e:
        return "sql error: %s" % e


def cmd_batch(sess: Session, args: argparse.Namespace) -> str:
    if args.file == "-":
        text = sys.stdin.read()
    else:
        with open(args.file) as fh:
            text = fh.read()
    image = select_image(args.image, args.device, args.firmware)
    blocks = []
    cap = args.max if args.max is not None else 40
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        try:
            argv = shlex.split(line)
        except ValueError as e:
            blocks.append("## %s\nerror: %s" % (line, e))
            continue
        if argv and argv[0] in ("fw.py", "fw"):
            argv = argv[1:]
        blocks.append(
            "## %s\n%s" % (line, run_line(sess, argv, image, cap, args.core, args.cpu))
        )
    return "\n".join(blocks)


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    sess = Session()
    try:
        if args.cmd == "batch":
            print(cmd_batch(sess, args))
        else:
            print(run(sess, args))
    except FwError as e:
        print("error: %s" % e, file=sys.stderr)
        return 1
    except sqlite3.Error as e:
        print("sql error: %s" % e, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
