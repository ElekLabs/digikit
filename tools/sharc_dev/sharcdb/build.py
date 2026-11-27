"""Filling a database from a firmware image and building it when stale."""

from __future__ import annotations

import bisect
import collections
import hashlib
import json
import os
import time
from pathlib import Path

from tools.sharc_dev import sharcflow, sharcfn, sharcimm, sharcinv, sharcstrings

from .analysis import analyze_image
from .cfg import _after_sw, _build_blocks_and_succ, _overwritten_bytes, _slots
from .facts import (
    classify_literal_range,
    extract_literal,
    extract_mem_access,
    mask_relocatable,
)
from .regdefs import register_effects
from .schema import DB_VERSION, KNOWN_CODE_BLOCKS, git_commit, open_db, read_meta


def build_input_sha256():
    """Hash the analysis source and instruction tables that populate this DB."""
    root = Path(__file__).resolve().parents[1]
    files = [
        *root.glob("sharcdb/*.py"),
        *root.glob("sharcfn/*.py"),
        *root.glob("sharc_core/*.py"),
        *root.glob("sharcspec/*.json"),
        *(
            root / name
            for name in (
                "sharc_disasm.py",
                "sharc_isa.py",
                "sharc_trace.py",
                "sharc_visa_tables.py",
                "sharcflow.py",
                "sharcimm.py",
                "sharcinv.py",
                "sharcldr.py",
                "sharcstrings.py",
            )
        ),
    ]
    digest = hashlib.sha256()
    for path in sorted(files):
        digest.update(str(path.relative_to(root)).encode())
        digest.update(b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def build_matches(meta, name, blob_sha, code_blocks, min_depth, input_sha):
    return (
        meta.get("image") == name
        and meta.get("cpu") == "sharc"
        and meta.get("image_sha256") == blob_sha
        and meta.get("db_version") == str(DB_VERSION)
        and meta.get("blocks") == json.dumps(list(code_blocks))
        and meta.get("min_depth") == str(min_depth)
        and meta.get("build_input_sha256") == input_sha
    )


def _fill_database(db, name, sha, blob_path, code_blocks, min_depth, input_sha, ctx):
    mem = ctx["mem"]
    functions = ctx["functions"]
    analyzed = ctx["analyzed"]
    blocks_by_idx = ctx["blocks_by_idx"]

    meta_rows = [
        (name, "image", name),
        (name, "image_sha256", sha),
        (name, "db_version", str(DB_VERSION)),
        (name, "tool_commit", git_commit() or ""),
        (name, "build_time", time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())),
        (name, "cpu", "sharc"),
        (name, "blob_path", os.path.abspath(blob_path)),
        (name, "min_depth", str(min_depth)),
        (name, "build_input_sha256", input_sha),
        (name, "blocks", json.dumps(list(code_blocks))),
    ]
    db.executemany("INSERT INTO meta VALUES (?,?,?)", meta_rows)

    block_rows = []
    for b in ctx["blocks"]:
        idx = b["index"]
        if b["fill"]:
            kind = "fill"
        elif idx in code_blocks:
            kind = "code"
        else:
            kind = "data"
        base_sw = (
            analyzed[idx]["base_sw"]
            if idx in analyzed
            else sharcinv.sw_base_for_target(b["target_address"])
        )
        block_rows.append(
            (
                name,
                idx,
                b["target_address"],
                b["byte_count"],
                kind,
                base_sw,
                _overwritten_bytes(mem, b),
            )
        )
    db.executemany("INSERT INTO blocks VALUES (?,?,?,?,?,?,?)", block_rows)
    db.executemany(
        "INSERT OR IGNORE INTO strings VALUES (?,?,?,?,?,?)",
        (
            (
                name,
                r["loaded_byte_address"],
                r["short_word_start"],
                r["short_word_end"],
                r["block_index"],
                r["text"],
            )
            for r in sharcstrings.map_strings(ctx["data"], ctx["blocks"])
        ),
    )

    entry_to_fn = {}
    func_rows = []
    for fn in functions:
        entry_to_fn.setdefault(fn["entry"], fn)
        func_rows.append(
            (
                name,
                fn["entry"],
                fn["exit"],
                fn["n_insns"],
                fn["block"],
                "FUN_%06x" % fn["entry"],
                int(not fn["has_no_static_caller"]),
                int(fn["is_leaf"]),
                fn["label"],
                fn["entry_kind"],
            )
        )
    db.executemany("INSERT INTO functions VALUES (?,?,?,?,?,?,?,?,?,?)", func_rows)

    block_spans = collections.defaultdict(list)
    for fn in functions:
        block_spans[fn["block"]].append((fn["entry"], fn["exit"]))
    for spans in block_spans.values():
        spans.sort()

    def owner_of(block_idx, sw):
        spans = block_spans.get(block_idx)
        if not spans:
            return None
        entries = [e for e, _ in spans]
        i = bisect.bisect_right(entries, sw) - 1
        if i < 0:
            return None
        entry, exit_ = spans[i]
        return entry if entry <= sw < exit_ else None

    def resolve_target_function(to_sw, from_block):
        """The function owning to_sw: an exact entry match in any block
        (how nearly every call/jump target resolves), else the containing
        function in the SAME block as the jump (covers an intra-block back-
        edge into the middle of a function). A cross-block jump into the
        middle of another block's function is not resolved (None) -- see
        the module docstring."""
        if to_sw is None:
            return None
        fn = entry_to_fn.get(to_sw)
        if fn is not None:
            return fn["entry"]
        return owner_of(from_block, to_sw)

    code_spans_sw = [
        (
            analyzed[i]["base_sw"],
            analyzed[i]["base_sw"] + blocks_by_idx[i]["payload_len"] // 2,
        )
        for i in code_blocks
        if i in analyzed
    ]
    code_spans_byte = [
        (
            blocks_by_idx[i]["target_address"],
            blocks_by_idx[i]["target_address"] + blocks_by_idx[i]["byte_count"],
        )
        for i in code_blocks
    ]

    bblock_rows, succ_rows, leaders_by_idx = _build_blocks_and_succ(
        name, code_blocks, analyzed, functions, owner_of
    )

    insn_rows, edge_rows, literal_rows, mem_rows, hash_rows = [], [], [], [], []
    dataref_rows, regdef_rows, reguse_rows = [], [], []
    unknown_form_counts = collections.Counter()

    for idx in code_blocks:
        block = analyzed.get(idx)
        if block is None:
            continue
        b = blocks_by_idx[idx]
        base_sw = block["base_sw"]
        # DB_VERSION v13: the block's declared range through LoadedMemory's
        # final, last-write-wins bytes, not its own stream payload -- see
        # the DB_VERSION comment above ("One decode path" / loader block 1).
        payload = mem.read(b["target_address"], b["byte_count"])
        if payload is None:
            continue

        table = sharcimm.decode_all(payload)
        depth = sharcimm.depths(table, len(payload))
        sweep = sharcimm.sweep_offsets(table, len(payload))
        aligned_set = {off for off in sweep if depth.get(off, 0) >= min_depth}
        for off in sorted(table):
            insn = table[off]
            sw = base_sw + off // 2
            is_aligned = off in aligned_set
            f = sharcinv.merge_fields(insn.fields)
            mnemonic = None
            if is_aligned:
                mnemonic, _notes, _gap = sharcfn.render_instruction(
                    sw, insn, mem, set(), collections.Counter(), []
                )
            fn_sw = owner_of(idx, sw) if is_aligned else None
            insn_rows.append(
                (
                    name,
                    sw,
                    idx,
                    insn.length_bytes,
                    None if insn.raw is None else "%x" % insn.raw,
                    insn.type_name,
                    json.dumps(insn.fields, sort_keys=True),
                    f.get("cond"),
                    mnemonic,
                    int(is_aligned),
                    insn.kind,
                    fn_sw,
                )
            )

        sites = block["sites"]
        aligned_insns = block["insns"]
        return_sws = {r["sw"] for r in sites["returns"]}
        linked_indirect_sws = {c["sw"] for c in sites["indirect_calls"]}
        local_leaders = leaders_by_idx.get(idx, set())
        last_i_literal = {}

        for c in sites["calls"]:
            edge_rows.append(
                (
                    name,
                    c["sw"],
                    c["target"],
                    "call",
                    int(bool(c["delayed"])),
                    c["cond"],
                    owner_of(idx, c["sw"]),
                    resolve_target_function(c["target"], idx),
                    "linked" if c.get("linked") else None,
                )
            )
        for r in sites["returns"]:
            edge_rows.append(
                (
                    name,
                    r["sw"],
                    None,
                    "return",
                    1,
                    None,
                    owner_of(idx, r["sw"]),
                    None,
                    ("after=0x%x" % r["after"]) if r["after"] is not None else None,
                )
            )

        for n, (off, insn) in enumerate(aligned_insns):
            t = insn.type_name
            f = sharcinv.merge_fields(insn.fields)
            sw = base_sw + off // 2

            if sw in local_leaders:
                # A new basic block starts here: dataref's 'resolved_offset'
                # only fires for an I-register literal load in the SAME
                # basic block as its use (see the module's SCHEMA comment).
                last_i_literal = {}

            if t in ("8a_abs", "8a_rel", "9a_rel", "9b_rel") and f.get("b") == 0:
                # Type9a_abs/9b_abs have no 'addr' field at all (only
                # pmi/pmm): they are indirect through PM(I,M), same family
                # as 9b_abs's return jump, not a direct/pcrel target -- see
                # the indirect branch below. 9a_rel/9b_rel and 8a_rel all
                # carry a real 'reladdr' (PRM Type9); only 8a_abs carries a
                # real 'addr'.
                cond = f.get("cond")
                target = (
                    f.get("addr")
                    if t == "8a_abs"
                    else sharcflow.pcrel_target(sw, f.get("reladdr", 0))
                )
                delayed = f.get("j")
                conditional = cond is not None and cond != sharcfn.ALWAYS_TRUE_COND
                kind = "cond_jump" if conditional else "jump"
                edge_rows.append(
                    (
                        name,
                        sw,
                        target,
                        kind,
                        None if delayed is None else int(bool(delayed)),
                        cond,
                        owner_of(idx, sw),
                        resolve_target_function(target, idx),
                        None,
                    )
                )
                if conditional:
                    fallthrough = None
                    if delayed:
                        pair = _slots(aligned_insns, n)
                        if pair:
                            fallthrough = _after_sw(base_sw, pair)
                    else:
                        fallthrough = base_sw + (off + insn.length_bytes) // 2
                    if fallthrough is not None:
                        edge_rows.append(
                            (
                                name,
                                sw,
                                fallthrough,
                                "fallthrough",
                                int(bool(delayed)) if delayed is not None else None,
                                cond,
                                owner_of(idx, sw),
                                resolve_target_function(fallthrough, idx),
                                "not-taken path of the conditional jump at this sw",
                            )
                        )

            elif t in ("9a_abs", "9b_abs") and sw not in return_sws:
                # Indirect jump/call through PM(Ipmi, Mpmm) -- no address
                # field exists on either form (see the comment above); a
                # 9a_abs is otherwise identical to a return-eligible 9b_abs
                # but also carries a parallel compute field.
                note = "PM(I%s, M%s)" % (f.get("pmi"), f.get("pmm"))
                note += (
                    " -- indirect call site (push+store in delay slots)"
                    if sw in linked_indirect_sws
                    else " -- indirect jump"
                )
                edge_rows.append(
                    (
                        name,
                        sw,
                        None,
                        "indirect",
                        1,
                        f.get("cond"),
                        owner_of(idx, sw),
                        None,
                        note,
                    )
                )

            literal = extract_literal(t, f)
            if literal is not None:
                value, dest = literal
                in_code, in_data = classify_literal_range(
                    value, code_spans_sw, code_spans_byte, mem
                )
                literal_rows.append(
                    (name, sw, value, t, dest, int(in_code), int(in_data))
                )

                if t == "14a":
                    # The literal IS the memory address here; use the same
                    # d-field direction extract_mem_access does, not a
                    # separate guess, so this never disagrees with
                    # mem_access's own abs_load/abs_store rows for the
                    # same instruction. 15a used to be listed alongside 14a
                    # here, on the same wrong assumption extract_mem_access
                    # made about its "addr" field -- see that function's
                    # module note. It is register-relative, not absolute,
                    # so it falls through to the plain 'literal' role below,
                    # like any other modify delta (16a/18a/19a*).
                    _space, direction = sharcfn._space_dir(f)
                    role = "abs_store" if direction == "store" else "abs_load"
                elif (
                    t in ("17a", "17b")
                    and dest is not None
                    and dest.startswith("I")
                    and dest[1:].isdigit()
                ):
                    # A direct register load (ureg = literal); a 19a* dest is
                    # also an "I%d" string but is a modify DELTA, not the
                    # register's new value, so it stays 'literal' below.
                    role = "i_reg_base"
                    last_i_literal[int(dest[1:])] = value & 0xFFFFFFFF
                else:
                    role = "literal"
                # dataref is address-shaped values: store the unsigned 32-bit
                # pattern (0x8055c840), not extract_literal's sign-extended
                # int (literals.value is signed on purpose, for an ordinary
                # arithmetic immediate like -1; an address materialised from
                # the same bit pattern should read as the address).
                dataref_rows.append((name, value & 0xFFFFFFFF, sw, t, role))

            mem_access_rows = extract_mem_access(t, f)
            for row in mem_access_rows:
                mem_rows.append((name, sw) + row)
                abs_address = row[7]
                if abs_address is not None and t != "14a":
                    # 14a is excluded: its abs_load/abs_store dataref row
                    # already came from the extract_literal branch above
                    # (14a IS a LITERAL_FORMS entry; 14d and 15a are not --
                    # 14d has no compute-independent literal to sign-extend,
                    # and 15a's "addr" is a register-relative offset, not an
                    # address, so extract_mem_access now always returns
                    # abs_address=None for it and this branch never fires
                    # for 15a at all).
                    direction = row[1]
                    role = "abs_store" if direction == "store" else "abs_load"
                    dataref_rows.append((name, abs_address, sw, t, role))

            if t in sharcfn.IMMOFF_MEM_FORMS:
                i_reg = f.get("i", 0)
                base = last_i_literal.get(i_reg)
                if base is not None:
                    bits = sharcfn._IMMOFF_DATA_BITS.get(t, 6)
                    off_val = sharcfn.sign_extend(f.get("data", 0), bits)
                    modified = (base + off_val) & 0xFFFFFFFF
                    # PRM Type4a/4b/4d pp.13-26/13-30/13-34 ("u"): u=0
                    # pre-modifies I for the address (I keeps its old value
                    # afterwards); u=1 accesses the CURRENT (unmodified) I
                    # and only writes I+offset back afterwards
                    # (post-modify) -- so THIS access is at `base`, not
                    # `base + off_val`, when u=1. Type15b has no u bit and
                    # is always pre-modify (PRM p.15-11). This matches
                    # tools/sharc_dev/sharcdb/'s own `ptr` table (_ptr_mem_form)
                    # and tools/sharc_dev/sharc_trace.py's "4a"/"4b" handlers, which
                    # already draw this same distinction -- this
                    # `resolved_offset` role had not been updated to match.
                    post_modify = t != "15b" and bool(f.get("u"))
                    address = base if post_modify else modified
                    dataref_rows.append((name, address, sw, t, "resolved_offset"))
                    if post_modify:
                        # I is permanently updated to the modified value
                        # now, so a later same-block IMMOFF access to the
                        # same I register must resolve against the new
                        # value, not the stale literal.
                        last_i_literal[i_reg] = modified

            defs, uses, unknown = register_effects(t, f)
            for reg, kind in defs:
                regdef_rows.append((name, sw, reg, kind))
            for reg in uses:
                reguse_rows.append((name, sw, reg))
            for tag in unknown:
                unknown_form_counts[tag] += 1

        for fn in functions:
            if fn["block"] != idx:
                continue
            fn_insns = sharcinv.instructions_in(block, fn["entry"], fn["exit"])
            exact_bytes, reloc_bytes, n_hashed = bytearray(), bytearray(), 0
            for _sw, insn in fn_insns:
                if insn.raw is None or not insn.length_bytes:
                    continue
                exact_bytes += insn.raw.to_bytes(insn.length_bytes, "big")
                reloc_bytes += mask_relocatable(insn).to_bytes(insn.length_bytes, "big")
                n_hashed += 1
            hash_rows.append(
                (
                    name,
                    fn["entry"],
                    hashlib.sha256(bytes(exact_bytes)).hexdigest(),
                    hashlib.sha256(bytes(reloc_bytes)).hexdigest(),
                    n_hashed,
                    len(exact_bytes),
                )
            )

    db.executemany("INSERT INTO insn VALUES (?,?,?,?,?,?,?,?,?,?,?,?)", insn_rows)
    db.executemany("INSERT INTO edges VALUES (?,?,?,?,?,?,?,?,?)", edge_rows)
    db.executemany("INSERT INTO literals VALUES (?,?,?,?,?,?,?)", literal_rows)
    db.executemany("INSERT INTO mem_access VALUES (?,?,?,?,?,?,?,?,?,?)", mem_rows)
    db.executemany("INSERT INTO func_hash VALUES (?,?,?,?,?,?)", hash_rows)
    db.executemany("INSERT INTO bblocks VALUES (?,?,?,?,?,?)", bblock_rows)
    db.executemany("INSERT INTO succ VALUES (?,?,?,?)", succ_rows)
    db.executemany("INSERT INTO dataref VALUES (?,?,?,?,?)", dataref_rows)
    db.executemany("INSERT INTO regdef VALUES (?,?,?,?)", regdef_rows)
    db.executemany("INSERT INTO reguse VALUES (?,?,?)", reguse_rows)
    for tag, count in unknown_form_counts.items():
        meta_rows_extra = [(name, "unknown_regdef_" + tag, str(count))]
        db.executemany("INSERT INTO meta VALUES (?,?,?)", meta_rows_extra)

    return {
        "n_insn": len(insn_rows),
        "n_functions": len(func_rows),
        "n_edges": len(edge_rows),
        "n_literals": len(literal_rows),
        "n_mem_access": len(mem_rows),
        "n_func_hash": len(hash_rows),
        "n_bblocks": len(bblock_rows),
        "n_succ": len(succ_rows),
        "n_dataref": len(dataref_rows),
        "n_regdef": len(regdef_rows),
        "n_reguse": len(reguse_rows),
        "unknown_form_counts": dict(unknown_form_counts),
    }


def build_database(
    blob_path, out_path, name=None, min_depth=8, blocks=None, force=False
):
    """Build one image database, skipping when its inputs and options match."""
    t0 = time.time()
    sha = sharcfn.sha256_of(blob_path)
    if name is None:
        name = (
            os.path.basename(os.path.dirname(os.path.abspath(blob_path)))
            or os.path.splitext(os.path.basename(blob_path))[0]
        )
    code_blocks = tuple(blocks) if blocks is not None else KNOWN_CODE_BLOCKS.get(sha)
    if code_blocks is None:
        raise SystemExit(
            "sharcdb: no known code-block list for image sha256 %s (%s); pass --blocks to override"
            % (sha, blob_path)
        )
    input_sha = build_input_sha256()
    if not force and os.path.exists(out_path):
        existing = read_meta(out_path)
        if build_matches(existing, name, sha, code_blocks, min_depth, input_sha):
            return {
                "name": name,
                "path": out_path,
                "skipped": True,
                "seconds": time.time() - t0,
                "size": os.path.getsize(out_path),
            }

    ctx = sharcfn.load_context(blob_path, code_blocks, min_depth)

    # Build into a temp file in the same directory and os.replace() it into
    # place at the very end, so a concurrent reader of out_path (e.g. an
    # agent running sharcdb.sql queries against out/sharcdb/*.sqlite while
    # this build is in flight) never sees a half-written database -- either
    # the old file (still fully valid) or the new one, never a torn one.
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
            counts = _fill_database(
                db, name, sha, blob_path, code_blocks, min_depth, input_sha, ctx
            )
            counts["analyze"] = analyze_image(
                db, name, mem=ctx["mem"], blocks=ctx["blocks"]
            )
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
        **counts,
    }
