---
name: firmware-query
description: Use before writing SQL or one-off scripts against the firmware databases (ColdFire cfdb, SHARC sharcdb) or when asking where a feature (machines, FX, parameters, menus, pattern and step storage, LFO, p-lock) is handled on a firmware version. Gives one compact query CLI, tools/coldfire/fw.py, and the per-firmware atlas.
---

# Firmware queries with tools/coldfire/fw.py

Run from the repo root with `-i IMAGE` (dt2-1.17 or dn2-1.12), or set
`FW_IMAGE=dt2-1.17`. Use older images as references for comparisons and porting.

In the examples, `fw.py` stands for `uv run python -m tools.coldfire.fw`.
Addressed queries select the CPU from the address or a declared `dsp.*` name.
Use `--cpu sharc` for DSP text, SQL, and atlas queries. A batch can mix CPUs.
Text output defaults to 60 lines, or 180 for `dossier`. Use `--max N` to
change the cap and `--max 0` for uncapped output. `stores` emits uncapped JSON.

Use `batch` for more than one question: one process, one `## query` block each.

    uv run python -m tools.coldfire.fw batch - -i dt2-1.17 <<'Q'
    atlas machine
    class MachineListView --max 8
    fn 0x401be1be --max 20
    str WRONG UI
    Q

## Build the databases for queries and Workbench

With sections extracted and ColdFire Ghidra dumps under `out/ghidra/`:

    uv run python -m tools.coldfire.fw build -i dt2-1.17 --other dn2-1.12

Build one program with `--core main` or `--core dsp`. Register
`out/firmware/<image>/<core>/firmware.sqlite` in Workbench
under **Settings → Firmware symbols**. Browse the files with a SQLite viewer
or query them with `fw sql`. Each database contains its functions and
cross-references, plus `catalog` and `catalog_groups` for candidate review.
See [Firmware analysis](../../../docs/FIRMWARE.md) for the input preparation
commands and the tables available for each core.

## Select a device and program

Use `--device NAME --firmware VERSION` in place of `-i IMAGE`. Short names and
product names both work. The version defaults to the device's acceptance
release or its sole registered release. `--core main` selects the main OS.
`--core dsp` selects the DSP program recorded for that release. Use `--cpu`
to select or check the instruction set. The two Syntakt programs both use
ColdFire and can have identical numeric addresses. Specify the core when
querying the DSP.

With the main and DSP Ghidra exports in place:

    uv run python -m tools.coldfire.fw build --device syntakt --firmware 1.42
    uv run python -m tools.coldfire.fw fn 0x4000290e --device syntakt --firmware 1.42 --core dsp
    uv run python -m tools.coldfire.fw review --device syntakt --firmware 1.42 --core dsp --all

The Syntakt main OS is `out/firmware/st-1.42/main/firmware.sqlite`. The DSP is
`out/firmware/st-1.42/dsp/firmware.sqlite` and its review page lands at
`out/firmware/st-1.42/dsp/review.html`. The legacy image key `-i st-1.42-peer`
also selects the DSP. A paired SHARC catalog needs `--other IMAGE`.

## SHARC queries

Run these in the same batch, with the image set on the batch:

    search q31 converter --cpu sharc
    dossier dsp.master.fn.q31_converter --other dn2-1.12
    search recips --cpu sharc --in disasm
    search q31_converter --cpu sharc --in catalog

`atlas TERM --cpu sharc` searches the recovered function inventory, port declarations,
notes and roles in sharcdb. `dossier` accepts a short-word PC
or a declared name. It shows marked anchors, callers, memory references,
hardware loops, the listing and matches on `--other`. Use `--callers N` and
`--listing N` to tune the output. `str TEXT --cpu sharc` finds candidate printable
runs from the loader, and `xref ADDR --cpu sharc` shows recorded references.

In `out/firmware/<image>/dsp/firmware.sqlite`, browse `function_index` for all recovered
functions, `function_anchors` for declared sites and provenance, and `catalog`
for anchor candidates. `sql ... --cpu sharc` queries those tables directly.
SHARC provides annotated listings and DSP-specific facts. Decompilation and C++
RTTI commands use the ColdFire data. See [Firmware analysis](../../../docs/FIRMWARE.md)
for the database build commands.

## ColdFire recipe: one primitive on both devices

1. `atlas TERM -i dt2-1.17` finds the candidate functions.
2. One `batch`, one `dossier ADDR --other dn2-1.12` line per hit. It prints name, size, hash, class and vtable slot, strings, callers with the argument setup before each call (an A0 load is marked as a possible hidden return pointer), callees, the first decompiled lines, existing anchors, the suggested rule and its pair on the other device.
3. `uv run python -m tools.firmware.catalog declare dt2-1.17 ADDR NAME --other dn2-1.12 --write` once the rule is the same on both.

Do not run `fn`, `decomp`, `disasm`, `callers` or `str` again for what dossier already printed; use `--callers N --decomp N` for more. Keep each question in one batch.

    $ fw.py dossier 0x4011139a -i dt2-1.17 --other dn2-1.12      (0.2 s; trimmed)
    dossier dt2-1.17 4011139a ConfirmWindow::ctor_dtor
    entry=4011139a size=342 end=401114f0 hash=2946e3541ac1 (1 with this hash)
    vfunc: no
    anchors: cf.window.fn.confirm_ctor [D]+0x0
    rule (catalog): cf_func both_devices=1
    on dn2-1.12 the rule resolves to: 40112d0e  (the pair)
    callers: 48 sites in 47 functions
      <- 400112a8 std::_Sp_counted_ptr_inplace<ConfirmWindow,...>::ctor_dtor @40011442 call
        40011430  lea (-0x10b2,PC),A0   ; A0: hidden return ptr?
        40011434  clr.l -(SP)
        40011436  move.l D4,-(SP)
        ...
      > 40011442  jsr ctor_dtor.l
    callees: 7
      -> 401e63ca FUN_401e63ca  {cf.string.fn.std_string.dtor}
      -> 4011b06e View::ctor_dtor
    decomp (12 of 36 lines): ...

The A0 mark is a hint: a PC-relative `lea` into A0 that is then stored is a function pointer, not a hidden
return pointer. A hidden return pointer is an A0 load right before the `jsr` that the callee writes through.

ColdFire query order

1. `atlas TERM` for "where is X handled" (terms: machine, fx, parameter, step, pattern, kit, sound, track,
   page, menu, lfo, p-lock, trig, retrig, song, chain, plus-drive, sample, filter, midi, project ...).
   Rows list the function that builds the table, UI classes, strings, descriptors, parameters and manual pages,
   each with a provenance and confidence. Build with `uv run python -m tools.coldfire.atlas build dt2-1.17`.
2. `search TERM` searches names, strings, decompiled C and atlas terms. Use
   `--in catalog` to search anchor candidates.
3. `fn`, `class`, `callers`, `callees`, `xref`, `path`, `decomp`, `disasm` on what the search found.
4. `port NAME --from dt2-1.16 --to dt2-1.17` carries a function between versions (cfmatch matches).
5. Use read-only `sql "SELECT ..."` for queries the commands above do not cover.
   ColdFire stores `functions`, `function_text`, `edges`, `datarefs`, `strings`,
   `symbols`, `typeinfos`, `vtables`, `matches` and `glossary`. SHARC stores
   `functions`, `function_index`, `function_anchors`, `edges`, `insn`,
   `strings`, `mem_access`, `literals` and `ptr`. Both have `catalog` and
   `catalog_groups` after catalog generation.
Only then write a script, and put it under tools/ if it will be reused.

An empty `callers` or `xref` is not evidence of dead code (Ghidra misses code outside functions and indirect
calls through vtables); confirm with `tools/coldfire/refscan.py`. Atlas rows marked `medium` are heuristic.

## Catalog: candidate functions for anchors

Build (both devices, one run; needs `atlas build` for both images and the cfdb of the old versions for `stable`):

    uv run python -m tools.coldfire.atlas catalog dt2-1.17 dn2-1.12

It adds the table `catalog` to `out/firmware/<image>/main/firmware.sqlite` and writes `out/firmware/<image>/main/exports/catalog.tsv` (best score first). Query:

    fw.py catalog -i dt2-1.17 --grade --top 20          # best anchor candidates
    fw.py catalog menu -i dt2-1.17 --tag near:cf.menu   # TERM matches name, class, hint or tags
    fw.py catalog -i dn2-1.12 --tag role:model
    fw.py catalog -i dt2-1.17 --groups --top 20         # groups by size
    fw.py catalog -i dt2-1.17 --group 3 --top 50        # rows of one group

Review by group first, then rows. The build also writes `out/firmware/<image>/main/exports/groups.tsv` (table `catalog_groups`) and `out/firmware/<image>/main/exports/graph.json` (nodes and call edges between catalog rows). A group is a Louvain community of the call graph between catalog rows; group -1 contains the ungrouped functions.

Full UI candidate review (after cataloguing both devices):

    uv run python -m tools.coldfire.fw review -i dt2-1.17 --core main --all
    uv run python -m tools.coldfire.fw review -i dn2-1.12 --core main --all

Open `out/firmware/<image>/main/review.html`. Paired-device links follow the release and core layout. `--all` includes every catalogue row/group, including small groups, anchored functions and ungrouped rows. Without it the visible set stays curated; omitted records remain lookup data so address/paired links can reveal them. Only the active group's filtered results render. Labels come from `out/firmware/<image>/main/labels/*.jsonl`, with a fallback to `out/atlas/labels/<image>/`. Override discovery with `--labels FILE ...`. Pass the previous review TSV with `--label-catalog FILE` when reusing labels after reclustering. The exporter matches group labels by exact member addresses.

Each run also writes `out/firmware/<image>/main/exports/review.tsv`: the complete catalogue with original evidence, flattened row/group labels and links to both pages, even when HTML is curated. Row links use `#r-0x...`. A pair outside the other candidate catalogue retains its address/match evidence without a dead link. SDK-use labels, hashes, stability, pair/rule agreement, UI-string counts and tags are visible on the page.

Without Claude's database, Keep/Later/Skip uses per-image browser storage, with JSONL export/import for backup and transfer (`<image>-decisions.jsonl`). Local-file storage varies by browser: export before closing when storage is unavailable, and before moving review files. Published Claude pages keep the existing database path; local imports are disabled there. Bitmap/Parameter candidate shortcuts search function metadata; glyph IDs, glyph previews and bitmap assets are not in this catalogue. Full workflow: `docs/DEVELOPMENT.md`, "ColdFire UI candidate review".

| column | meaning |
| --- | --- |
| tags | why it is a candidate: ui_own_vfunc, ui_vfunc, ui_helper, ui_string, near:PREFIX (next to an existing anchor), role:ROLE, term:TERM |
| ui_callers, ui_strings, hint | UI vfuncs that call it; UI strings it uses; first such string |
| h_strict, hash_count, stable | hash, functions with the same hash, same hash as the matched function in the old version (NULL: unknown) |
| pair_addr, pair_method, pair_same_hash | the matched function on the other device |
| anchors | existing anchors in the function (these score lower) |
| rule_kind, rule_ordinal, rule_both | suggested rule (cf_func, cf_vtable_slot, none) and whether it is the same on both devices |
| anchor_grade, score | 1 when a hash rule fits both devices and versions; ranking |
| caller_classes | up to 3 classes whose vfuncs call it, as `Class:count` |
| callees_known | up to 3 callees that are anchored or named, most called first |
| work_name | working name of a `FUN_` row: `Class::helper_XXXX`, `str:HINT` or `calls:NAME`; empty otherwise |
| group | call-graph group id, -1 when alone |

### DSP (SHARC) catalog

    uv run python -m tools.sharc_dev.sharc_catalog dt2-1.17 dn2-1.12 [--profile IMAGE=PATH]
    fw.py catalog -i dt2-1.17 --cpu sharc --grade --top 20

Needs `out/firmware/<image>/dsp/firmware.sqlite` (also of the old image, for `stable`). Adds `function_index`, `function_anchors`, `function_names`, `function_notes`, `catalog` and `catalog_groups` to sharcdb. TSV and graph files go in the DSP core's `exports/` folder. Addresses are short words. Columns differ from ColdFire: `reloc_hash` replaces `h_strict`; no ui or class fields. DSP-only: `role` (voice, master/fx, util, frame, io, other), `label`, `callers`, `callees`, `loops`, `do`, `recip`, `lit_data`, `abs_dm`, `hints`, and `prof_insns` (NULL without `--profile`, which also adds the tag `hot`). `fw.py catalog` shows `role` and `hints`; TERM matches name, role, hints, tags or anchors.

For browser review, run `uv run python -m tools.coldfire.fw review -i IMAGE --core dsp`
for each image in the pair. Open `out/firmware/<image>/dsp/review.html`. Paired
links follow the release and core layout. The page includes every DSP
catalog candidate and supports search by group title, name, anchor, role,
and hint. Store review JSONL in the DSP core's `labels/` folder. The legacy `out/atlas/labels/sharc/<image>/` folder is a fallback. Group
titles without a review label come from the existing DSP roles and hints.

`fw.py cost ADDR [--vars R2=32] [--vars-all R2=32]` gives a static instruction count with DO-loop trip counts (literal or symbolic) and callees. It is an upper bound over all paths with no stall model, and the unit of `dsp_cost_voice` and `patchkit budget`. Trip symbols name the register and the function that reads it, `R2@1cd286`: a callee's R2 is not the caller's. `--vars NAME=V` sets the entry function's register, `NAME@FN=V` one function's, `--vars-all NAME=V` every function's; an unknown name is an error that lists the symbols present. Jumps to another function (conditional too) count as tail calls whose targets are summed (upper bound); with `--vars` the output also gives the tighter total taking the largest target. A zero or missing DO count is `trip?@fn`; notes list what was not handled.

### From a candidate to a declaration

    uv run python -m tools.firmware.catalog emit-rule dt2-1.17 0x40113dc8 --other dn2-1.12    # rule, where it resolves, same on both?
    uv run python -m tools.firmware.catalog declare dt2-1.17 0x40113dc8 cf.text.fn.draw_fmt --other dn2-1.12 [--write]

ADDR below 0x1000000 is a SHARC pc. `emit-rule` prints the `ports/fw` rule of the function that contains ADDR (hash, `ordinal` if the hash is not unique, offset, `--len`) and checks it on the image and, with `--other`, on the other device against the catalogue `pair_addr`. `declare` builds a `mark = "O"` entry from it (shared rule, or one rule per device) and with `--write` inserts it into `ports/fw/<subsystem>.toml`. Needs the atlas catalogue for the pair.
To look up declared anchors, use `ports/bindings/reference.json` or `mise run fw:reference`.

Marks stay [D]/[O] until a second check against image bytes. "No callers" needs `tools/coldfire/refscan.py`.

## Examples (output trimmed)

Where is the machine list built?

    $ fw.py atlas machine -i dn2-1.12 --max 6
    glossary 'machine': 23 rows
      function  0x400c2c4e  builds the 43-entry page/machine descriptor table at 0x42433af8 ... (high)
      class     MachineListView  role=view vtable=0x401df910 slots=18 (high)
      class     MachineParameterPageView  role=page vtable=0x401dfaf8 slots=60 (high)

What does a function do and who uses it?

    $ fw.py fn SourcePageView::vfunc_19
    40016de2 SourcePageView::vfunc_19  size=202  end=40016eac
    vtable: SourcePageView#19
    callees: 12 ...
    decomp (12 of 27 lines): ...

A UI string and the code that shows it:

    $ fw.py str "PATTERN NAME"
    4023eb34 'PATTERN NAME'
        used by 4007f086 std::_Sp_counted_ptr_inplace<NameView,...>::ctor_dtor

An RTTI class with its vtable (methods inherited from a base are marked `<View>`):

    $ fw.py class PatternGridView --max 4
    PatternGridView  vmi_class  typeinfo=401f9008  bases=View, Observer, LedHandler, GlobalInstances
    vtable 401f9074 (offset_to_top 0)
      #0   401b1744 PatternGridView::vfunc_0

Everything that references an address:

    $ fw.py xref 0x40241826
    xref 40241826  40241826
      string 'WRONG UI CARD'
      data DATA  400cadb2 in 400cad5a FUN_400cad5a

The same function on another version:

    $ fw.py port 0x401bdee2 --from dt2-1.16 --to dt2-1.17
    401bdee2 FUN_401bdee2  (dt2-1.16) -> dt2-1.17
      401be1be FUN_401be1be  [strict]

A SHARC function (no decompiler; listing and call counts):

    $ fw.py fn 0xb80105 --max 8
    b80105 FUN_b80105 (DMA/descriptor construction)  insns=38 end=b8015b block=69 leaf return_boundary
    callers: 2 ...

Full-text search over decompiled C:

    $ fw.py search "pattern name" --in decomp
    decomp: 2 functions (best 2)
      40070c00 PatternGridView::vfunc_2 : char PatternGridView::vfunc_2(int param_1,undefined4 param_2)
