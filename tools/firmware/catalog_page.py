"""catalog_page.py: build the anchor review page from a catalogue and its labels.

Reads the ColdFire review exports or the SHARC catalog tables in sharcdb,
adds the agent labels (JSONL) and writes a standalone HTML page with its TSV.
"""

from __future__ import annotations

import argparse
import csv
import glob
import html
import json
import os
import sqlite3
import sys
from collections import Counter, defaultdict
from collections.abc import Iterable, Mapping, Sequence
from contextlib import closing
from typing import Any
from urllib.parse import quote

from tools.coldfire import fw
from tools.firmware import config, device
from tools.firmware.catalog import group_edges, parse_counts

Row = dict[str, str]

HERE = os.path.dirname(os.path.abspath(__file__))
TEMPLATE = os.path.join(HERE, "catalog_page.html")
# the acceptance image of the other device, when there are exactly two devices
_ACCEPTANCE = sorted(config.DEVICE_SYX)
OTHER_IMAGE = (
    {a: b for a, b in zip(_ACCEPTANCE, reversed(_ACCEPTANCE), strict=True)}
    if len(_ACCEPTANCE) == 2
    else {}
)
DEVICE_NAMES = {d.short: d.name for d in device.load_all(config.DEVICES_DIR)}
MAX_NEIGHBOURS = 8


def read_tsv(path: str) -> list[Row]:
    csv.field_size_limit(sys.maxsize)
    with open(path, newline="") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def sqlite_rows(cursor: sqlite3.Cursor, *address_columns: str) -> list[Row]:
    columns = [desc[0] for desc in cursor.description]
    return [
        {
            key: ""
            if value is None
            else hex(value)
            if key in address_columns
            else str(value)
            for key, value in zip(columns, row, strict=True)
        }
        for row in cursor
    ]


def read_sharc_catalog(
    image: str,
) -> tuple[list[Row], list[Row], list[tuple[str, str, int]]]:
    path = os.path.join(fw.SHARC_DIR, image + ".sqlite")
    with closing(fw.open_ro(path)) as db:
        db.execute("BEGIN")
        rows = sqlite_rows(
            db.execute("SELECT * FROM catalog ORDER BY score DESC, addr"),
            "addr",
            "pair_addr",
        )
        groups = sqlite_rows(
            db.execute("SELECT * FROM catalog_groups ORDER BY size DESC, id"), "best"
        )
        calls: dict[int, Counter[int]] = defaultdict(Counter)
        for caller, callee, count in db.execute(
            "SELECT from_function, to_function, count(*) FROM edges "
            "WHERE image=? AND kind IN ('call','jump') "
            "AND from_function IN (SELECT addr FROM catalog) "
            "AND to_function IN (SELECT addr FROM catalog) AND from_function<>to_function "
            "GROUP BY from_function, to_function",
            (image,),
        ):
            calls[caller][callee] = count
    addrs = [int(row["addr"], 16) for row in rows]
    edges = [(hex(a), hex(b), n) for (a, b), n in group_edges(addrs, calls).items()]
    return rows, groups, edges


def review_filename(image: str, cpu: str) -> str:
    return image + ("-dsp" if cpu == "sharc" else "") + "-review.html"


def group_members(rows: Iterable[Row]) -> dict[int, set[int]]:
    members: dict[int, set[int]] = defaultdict(set)
    for row in rows:
        members[int(row["group"])].add(int(row["addr"], 16))
    return members


def read_labels(
    paths: Iterable[str],
    rows: Iterable[Row] = (),
    source_rows: Iterable[Row] | None = None,
) -> tuple[dict[int, list[dict[str, Any]]], dict[int, list[dict[str, Any]]]]:
    """Read labels, aligning old group IDs by exact membership when supplied."""
    current = group_members(rows)
    groups = {frozenset(members): gid for gid, members in current.items() if gid >= 0}
    previous = group_members(source_rows) if source_rows is not None else None
    shared_addresses = (
        set().union(*current.values()) & set().union(*previous.values())
        if previous is not None
        else None
    )
    group_labels: dict[int, list[dict[str, Any]]] = defaultdict(list)
    row_labels: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for path in paths:
        with open(path) as f:
            for line in f:
                if not line.strip():
                    continue
                label = json.loads(line)
                if "addr" in label:
                    addr = int(label["addr"], 16)
                    if shared_addresses is None or addr in shared_addresses:
                        row_labels[addr].append(label)
                    continue
                source_gid = int(label["group"])
                gid: int | None = source_gid
                if "members" in label:
                    gid = groups.get(
                        frozenset(int(addr, 16) for addr in label["members"])
                    )
                elif previous is not None:
                    gid = groups.get(frozenset(previous.get(source_gid, ())))
                if gid is not None:
                    group_labels[gid].append(label)
    return group_labels, row_labels


def pick_rows(
    members_by_group: Mapping[int, Sequence[Row]],
    labelled_addrs: Iterable[int],
    per_group: int,
) -> list[Row]:
    """Per group: labelled rows, then the best unanchored rows up to per_group."""
    labelled = set(labelled_addrs)
    out: list[Row] = []
    for members in members_by_group.values():
        ordered = sorted(members, key=lambda r: -float(r["score"]))
        pick = [r for r in ordered if int(r["addr"], 16) in labelled]
        taken = {int(r["addr"], 16) for r in pick}
        for r in ordered:
            if len(pick) >= per_group:
                break
            if int(r["addr"], 16) not in taken and not r["anchors"]:
                pick.append(r)
                taken.add(int(r["addr"], 16))
        out.extend(pick)
    return out


def select_groups(
    groups: Sequence[Row], labelled_groups: Iterable[int], min_group: int
) -> tuple[list[Row], set[int]]:
    """Groups of at least min_group rows, plus smaller ones with a labelled row.

    Returns the kept groups and the ids of the small ones (show labelled rows only).
    """
    labelled = set(labelled_groups)
    keep = [
        g for g in groups if int(g["size"]) >= min_group or int(g["id"]) in labelled
    ]
    small = {int(g["id"]) for g in keep if int(g["size"]) < min_group}
    return keep, small


def neighbours(edges: Iterable[Sequence[Any]]) -> dict[int, list[tuple[int, int]]]:
    """Address -> (weight, other address) for each call edge, both directions."""
    nbr: dict[int, list[tuple[int, int]]] = defaultdict(list)
    for a, b, w in edges:
        a, b = int(a, 16), int(b, 16)
        nbr[a].append((w, b))
        nbr[b].append((w, a))
    return nbr


def label_fields(labels: Sequence[Mapping[str, Any]]) -> list[dict[str, str]]:
    return [
        {
            "role": x.get("role", ""),
            "anchor": x.get("proposed_anchor", ""),
            "sig": x.get("signature_guess", ""),
            "conf": x.get("confidence", ""),
            "ev": x.get("evidence", ""),
            "use": x.get("sdk_use", ""),
        }
        for x in labels
    ]


DSP_ROLES = {
    "voice": "Voice",
    "master/fx": "Master / effects",
    "frame": "Frame",
    "io": "I/O",
    "util": "Utility",
}


def suggest_group_title(group: Row, cpu: str, rows: Sequence[Row]) -> tuple[str, str]:
    """Summarize the recorded class or role distribution and decoder hints."""
    gid = int(group["id"])
    if gid < 0:
        return "Ungrouped functions", ""
    classes = parse_counts(group["top_classes"])
    if cpu == "cf":
        if classes:
            return " · ".join(name for name, _ in classes[:2]), "Common classes"
        working_names = {r.get("work_name", "") for r in rows}
        if len(working_names) == 1:
            name = next(iter(working_names))
            if name.startswith("calls:"):
                return "Calls " + name.removeprefix("calls:"), "Common callee"
    if cpu == "sharc":
        role = ""
        if classes:
            name, count = classes[0]
            if 2 * count >= int(group["size"]):
                role = DSP_ROLES.get(name, "")
        hint = next(
            (
                text
                for text in group["top_strings"].split(" | ")
                if text not in ("", "unclassified/mixed")
            ),
            "",
        )
        title = " · ".join(part for part in (role, hint[:1].upper() + hint[1:]) if part)
        if title:
            if role and hint:
                return title, "Dominant DSP role and common decoder hint"
            return title, "Dominant DSP role" if role else "Common decoder hint"
        subsystems = {
            match.split(".")[1]
            for row in rows
            for anchor in row["anchors"].split(",")
            if (match := anchor.strip()).startswith("dsp.") and match.count(".") >= 2
        }
        if len(subsystems) == 1:
            return next(
                iter(subsystems)
            ).capitalize() + " anchor", "Declared anchor in this group"
        if any(
            tag == "helper" and count == int(group["size"])
            for tag, count in parse_counts(group["top_tags"])
        ):
            return "Shared helpers", "Call graph marks each function as a helper"
    return f"Group {gid}", ""


def build_payload(
    image: str,
    other: str,
    rows: Sequence[Row],
    groups: Sequence[Row],
    edges: Iterable[Sequence[Any]],
    group_labels: Mapping[int, Sequence[Mapping[str, Any]]],
    row_labels: Mapping[int, Sequence[Mapping[str, Any]]],
    min_group: int,
    per_group: int,
    full_inventory: bool = False,
    other_addrs: set[int] | None = None,
    cpu: str = "cf",
) -> dict[str, Any]:
    edges = list(edges)
    by_addr = {int(r["addr"], 16): r for r in rows}
    nbr = neighbours(edges)

    def disp(a: int) -> str:
        r = by_addr.get(a)
        if not r:
            return hex(a)
        return r.get("work_name") or r["name"]

    labelled_groups = {int(by_addr[a]["group"]) for a in row_labels if a in by_addr}
    keep_groups, small = select_groups(groups, labelled_groups, min_group)
    if full_inventory:
        keep_groups, small = list(groups), set()
        known = {int(g["id"]) for g in keep_groups}
        for gid in sorted({int(r["group"]) for r in rows} - known):
            members_missing = [r for r in rows if int(r["group"]) == gid]
            keep_groups.append(
                {
                    "id": str(gid),
                    "size": str(len(members_missing)),
                    "grade": str(
                        sum(r["anchor_grade"] == "1" for r in members_missing)
                    ),
                    "anchored": str(sum(bool(r["anchors"]) for r in members_missing)),
                    "top_classes": "",
                    "top_strings": "",
                    "top_tags": "",
                }
            )
    members: dict[int, list[Row]] = {int(g["id"]): [] for g in keep_groups}
    for r in rows:
        gi = int(r["group"])
        if gi in members:
            members[gi].append(r)
    picked = list(rows) if full_inventory else pick_rows(members, row_labels, per_group)
    picked = [
        r
        for r in picked
        if int(r["group"]) not in small or int(r["addr"], 16) in row_labels
    ]
    included = {int(r["addr"], 16) for r in picked}

    out_rows: list[dict[str, Any]] = []
    for r in picked:
        a = int(r["addr"], 16)
        ns = sorted(nbr.get(a, []), key=lambda t: (-t[0], t[1]))[:MAX_NEIGHBOURS]
        out_rows.append(
            {
                "a": r["addr"],
                "n": r["name"],
                "wn": r.get("work_name", ""),
                "cls": r.get("role", r.get("class", "")),
                "g": int(r["group"]),
                "sc": float(r["score"]),
                "gr": r["anchor_grade"] == "1",
                "tags": r["tags"],
                "uic": int(r.get("ui_callers") or 0),
                "hint": r.get("hints", r.get("hint", "")),
                "pair": r["pair_addr"],
                "pm": r["pair_method"],
                "pair_url": pair_link(other, r["pair_addr"], other_addrs, cpu),
                "hash": r.get("reloc_hash", r.get("h_strict", "")),
                "hash_count": r.get("hash_count", ""),
                "stable": r.get("stable", ""),
                "pair_same_hash": r.get("pair_same_hash", ""),
                "rule_both": r.get("rule_both", ""),
                "ui_strings": r.get("ui_strings", ""),
                "anchors": r["anchors"],
                "rule": r["rule_kind"]
                + ("#" + r["rule_ordinal"] if r["rule_ordinal"] else ""),
                "by": r.get("caller_classes", ""),
                "ck": r.get("callees_known", ""),
                "size": int(r["size"] or 0),
                "lab": label_fields(row_labels.get(a, [])),
                "nb": [[hex(b), disp(b), b in included, w] for w, b in ns],
            }
        )
        if cpu == "sharc":
            out_rows[-1]["dsp"] = {
                key: r.get(key, "")
                for key in ("bytes", "callers", "callees", "loops", "do", "prof_insns")
            }

    out_groups: list[dict[str, Any]] = []
    for g in keep_groups:
        gid = int(g["id"])
        title, basis = suggest_group_title(g, cpu, members[gid])
        out_groups.append(
            {
                "id": gid,
                "title": title,
                "title_basis": basis,
                "size": int(g["size"]),
                "grade": int(g["grade"]),
                "anch": int(g["anchored"]),
                "classes": g["top_classes"],
                "strings": g["top_strings"],
                "tags": g["top_tags"],
                "labels": [
                    {
                        "label": x.get("label", ""),
                        "summary": x.get("summary", ""),
                        "sub": x.get("subsystem", ""),
                        "rel": x.get("sdk_relevance", ""),
                    }
                    for x in group_labels.get(gid, [])
                ],
            }
        )

    payload = {
        "image": image,
        "cpu": cpu,
        "other": other,
        "total_rows": len(rows),
        "total_groups": len(
            {int(g["id"]) for g in groups} | {int(r["group"]) for r in rows}
        ),
        "full_inventory": full_inventory,
        "groups": out_groups,
        "rows": out_rows,
    }
    if not full_inventory:
        # Curated pages resolve links from the full TSV or the other device by
        # promoting an omitted row on demand, without rendering the inventory.
        complete = build_payload(
            image,
            other,
            rows,
            groups,
            edges,
            group_labels,
            row_labels,
            min_group,
            per_group,
            True,
            other_addrs,
            cpu,
        )
        selected = {r["a"] for r in out_rows}
        payload["linked_rows"] = [r for r in complete["rows"] if r["a"] not in selected]
        payload["linked_groups"] = complete["groups"]
    return payload


def review_link(image: str, addr: str, page_filename: str | None = None) -> str:
    page = (
        quote(page_filename, safe="")
        if page_filename is not None
        else quote(image, safe="") + "-review.html"
    )
    return page + "#r-" + quote(addr, safe="")


def pair_link(
    other: str, addr: str, other_addrs: set[int] | None, cpu: str = "cf"
) -> str:
    if (
        not other
        or not addr
        or (other_addrs is not None and int(addr, 16) not in other_addrs)
    ):
        return ""
    return review_link(other, addr, review_filename(other, cpu))


def write_review_tsv(
    path: str,
    image: str,
    other: str,
    rows: Sequence[Row],
    group_labels: Mapping[int, Sequence[Mapping[str, Any]]],
    row_labels: Mapping[int, Sequence[Mapping[str, Any]]],
    other_addrs: set[int] | None = None,
    page_filename: str | None = None,
    cpu: str = "cf",
) -> None:
    """Export the complete inventory with original evidence and flattened labels."""
    label_keys = (
        "role",
        "proposed_anchor",
        "signature_guess",
        "confidence",
        "evidence",
        "sdk_use",
    )
    group_keys = ("label", "summary", "subsystem", "sdk_relevance")
    columns = ["image", "other_image", *dict.fromkeys(k for r in rows for k in r)]
    columns += ["review_link", "pair_review_link"]
    columns += ["label_" + k for k in label_keys] + ["group_" + k for k in group_keys]
    with open(path, "w", newline="") as f:
        writer = csv.DictWriter(f, columns, delimiter="\t")
        writer.writeheader()
        for r in rows:
            labels = row_labels.get(int(r["addr"], 16), [])
            glabels = group_labels.get(int(r["group"]), [])
            writer.writerow(
                {
                    **r,
                    "image": image,
                    "other_image": other,
                    "review_link": review_link(
                        image, r["addr"], page_filename or review_filename(image, cpu)
                    ),
                    "pair_review_link": pair_link(
                        other, r["pair_addr"], other_addrs, cpu
                    ),
                    **{
                        "label_" + k: " | ".join(str(x.get(k, "")) for x in labels)
                        for k in label_keys
                    },
                    **{
                        "group_" + k: " | ".join(str(x.get(k, "")) for x in glabels)
                        for k in group_keys
                    },
                }
            )


def device_title(image: str) -> str:
    """'dt2-1.17' -> 'Digitakt II OS 1.17'."""
    prefix, _, version = image.partition("-")
    return "%s OS %s" % (DEVICE_NAMES.get(prefix, prefix), version)


def subtitle(image: str, other: str) -> str:
    if not other:
        return "%s candidates." % device_title(image)
    return "%s candidates, paired with %s." % (
        device_title(image),
        device_title(other),
    )


def render(template: str, payload: Mapping[str, Any], sub: str) -> str:
    data = json.dumps(payload, separators=(",", ":")).replace("<", "\\u003c")
    return template.replace("__SUBTITLE__", html.escape(sub)).replace("__DATA__", data)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("image")
    ap.add_argument("--cpu", choices=("cf", "sharc"), default="cf")
    ap.add_argument("--other", help="image of the paired device")
    ap.add_argument(
        "--labels",
        nargs="+",
        metavar="FILE",
        help="JSONL label files; replaces <atlas>/labels/<image>/*.jsonl",
    )
    ap.add_argument(
        "--label-catalog",
        metavar="TSV",
        help="catalog or review TSV the labels were written against, for the same image and CPU",
    )
    ap.add_argument("--min-group", type=int, default=10)
    ap.add_argument("--per-group", type=int, default=20)
    ap.add_argument(
        "--all",
        action=argparse.BooleanOptionalAction,
        help="include every catalog row and group (default for SHARC)",
    )
    ap.add_argument("--out", help="override the output HTML path under out/atlas")
    args = ap.parse_args(argv)

    image = args.image
    other = args.other or OTHER_IMAGE.get(image, "")
    base = os.path.join(fw.ATLAS_DIR, image)
    paths = [
        base + s for s in ("-catalog.tsv", "-catalog-groups.tsv", "-catalog-graph.json")
    ]
    label_dir = os.path.join(fw.ATLAS_DIR, "labels")
    if args.cpu == "sharc":
        try:
            rows, groups, edges = read_sharc_catalog(image)
        except (fw.FwError, sqlite3.Error):
            print(
                f"missing DSP catalog; run: uv run python -m tools.sharc_dev.sharc_catalog {image} {other or 'OTHER_IMAGE'}",
                file=sys.stderr,
            )
            return 2
        other_addrs = set()
        if other:
            try:
                other_rows, _, _ = read_sharc_catalog(other)
                other_addrs = {int(r["addr"], 16) for r in other_rows}
            except (fw.FwError, sqlite3.Error):
                pass
        label_dir = os.path.join(label_dir, "sharc")
    else:
        if not all(os.path.exists(p) for p in paths):
            print(
                "missing catalogue files; run: uv run python -m tools.coldfire.atlas catalog %s %s"
                % (image, other or "OTHER_IMAGE"),
                file=sys.stderr,
            )
            return 2
        rows = read_tsv(paths[0])
        groups = read_tsv(paths[1])
        other_catalog = os.path.join(fw.ATLAS_DIR, other + "-catalog.tsv")
        other_addrs = (
            {int(r["addr"], 16) for r in read_tsv(other_catalog)}
            if other and os.path.exists(other_catalog)
            else set()
        )
        with open(paths[2]) as f:
            edges = json.load(f)["edges"]
    label_files = args.labels or sorted(
        glob.glob(os.path.join(label_dir, image, "*.jsonl"))
    )
    source_rows = read_tsv(args.label_catalog) if args.label_catalog else None
    if source_rows is not None:
        hash_field = "reloc_hash" if args.cpu == "sharc" else "h_strict"
        if not source_rows or any(
            not {"addr", "group", hash_field} <= r.keys()
            or r.get("image", image) != image
            for r in source_rows
        ):
            ap.error(
                "--label-catalog must contain the old catalog for this image and CPU"
            )
    group_labels, row_labels = read_labels(label_files, rows, source_rows)
    payload = build_payload(
        image,
        other,
        rows,
        groups,
        edges,
        group_labels,
        row_labels,
        args.min_group,
        args.per_group,
        args.cpu == "sharc" if args.all is None else args.all,
        other_addrs,
        args.cpu,
    )
    with open(TEMPLATE) as f:
        template = f.read()
    sub = subtitle(image, other)
    page = render(template, payload, ("SHARC · " if args.cpu == "sharc" else "") + sub)
    out = args.out or os.path.join(fw.ATLAS_DIR, review_filename(image, args.cpu))
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(out, "w") as f:
        f.write(page)
    tsv = os.path.splitext(out)[0] + ".tsv"
    write_review_tsv(
        tsv,
        image,
        other,
        rows,
        group_labels,
        row_labels,
        other_addrs,
        page_filename=os.path.basename(out),
        cpu=args.cpu,
    )
    print(
        "groups",
        len(payload["groups"]),
        "rows",
        len(payload["rows"]),
        "labelled",
        sum(1 for r in payload["rows"] if r["lab"]),
        "bytes",
        len(page),
        "->",
        out,
        "TSV ->",
        tsv,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
