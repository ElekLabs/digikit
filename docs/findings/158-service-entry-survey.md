# Service entry survey: which plug-in point sites and tables can move from the image to card services

Static survey, DT2 OS 1.17 and DN2 OS 1.12. Nothing was run (no emulator, no Ghidra). Three agents read the firmware databases, the image-module manifests and the service manifests; the per-site tables follow below. Marks: **[D]** read once, **[O]** open. Nothing here is **[V]**: a second agent has not checked the claims against image bytes.

Question: a generic resident dispatcher would name no service, and services would register handlers for named sites. The site-specific part is the entry from stock code into the dispatcher. Options, best first: (1) redirect an indirect call the firmware already makes, with a 32-bit data write at run time; (2) a code patch written at load by the runtime's installer; (3) an entry flashed in the image.

## Answer

- **No existing firmware dispatcher can be reused (option 1: 0 sites).** Every hooked site on both CPUs is a direct call or jump immediate or a mid-function address. The computed jumps on the SHARC are compiler switch tables (read-only boot-stream data, entries are case labels in the same function, guarded against new types). The LFO function table (DT2 `0x402227d4`, DN2 `0x4020b7f8`) is in writable DDR but covers only waves 0..5; new waves need the compare sites. [D]
- **ColdFire: almost everything can be installed at load (option 2).** The whole main OS runs from DDR (ACR0 `0x40000000..0x47ffffff`, copyback, writable), so code, tables and vectors are writable at run time. The installer runs at the main-OS task entry (`cf.rtos.hook.main_os_task`), before the UI exists and before vector 191 has run. [D]
  - Already option 2: 31 service hook entries (fx 9, machines 13, project 5, shortcuts 1, lfo 3) and 14 slot fills. [D]
  - Flashed today, movable to option 2: the 12 lfo stubs (the port hooks and exits exist; the lfo service already imports the exit names), machines (5 hook ops, the DN2 `frametype` stub, about 30 bound and pointer writes), dynreg (3 hook ops), params (2 hook ops, 4 bulk writes). They need a load-time buffer in the arena for the copied tables (names, permit, per-type, list, params). [D]
  - Must stay flashed: the runtime's own `main_os_task` hook, and the DN2 sample-pool `boot_wire_size` (boot thread, before the card) and `reply_copy` (interrupt, tied to the boot wire change). [D]
- **Firmware tables: content can be written at load; one table needs a decision.** [D]
  - The machine list and rank are first read in the MainScreenView constructor, after the task entry, so a service loaded at the entry needs no rebuild; `FUN_40060f50` rebuilds the list if needed (finding 30).
  - Machine descriptor and dispatch rows, DN2 page tables and fxpages: A (data write at load). Name and permit tables: 3 and 1 readers to patch. DT2 per-type byte: 12 sites.
  - The parameter table (DT2 275 rows, DN2 321) is read through absolute addresses: 56 references and 54 `cmpi` bounds, 110 sites per device. Growing it is either 110 load-time patches or one flashed reservation (about 15 KB).
- **SHARC: no load-time path exists today.** [D]
  - Option 2 would cover 30 sites (28 dsp-voice hooks, both fx hooks), but only with a DSP-side installer, which does not exist and is unproven (48-bit instructions, instruction cache, finding 128).
  - Option 3 only: DN2 `cache_enable` (boot), `boot_wire_size` (boot literal), and the dsp-link `block_tail` (the transport cannot install itself). These are platform, not service, sites.
  - dsp-link can write only its windows (slot block and area, voice rows, effect rows, stats). Stock DSP tables are outside every window; a "poke" window in the DSP receiver would be needed. DSP row capacity (3 effect rows, 8 voice rows) is flashed.

## Consequence for the design

- On the ColdFire a service can own all its entries and tables at load, so `points/{lfo,machines,dynreg,params,fxpages}` can move into their services, apart from one generic parameter-table reservation if the 110 patches are not wanted.
- On the SHARC the 30 voice and effect sites stay flashed until a DSP-side installer is proven. Kept flashed, they should be generic numbered slots (call through slot N or run stock) generated from port data, owned by the platform and filled by name, so no service code is in the image.
- The flashed remainder is platform: the installer, the platform hooks, dsp-link, the DN2 sample boot path, and the SHARC slot stubs.

## Open

- Interrupt level of vector 191: if it is 7, masking to IPL 7 does not keep it out during an lfo install [O].
- Whether `permit` (DT2 `0x400da514`) is read before the task entry [O].
- Whether the dynreg `view.build` op is stale (docs/design/digikit-runtime.md line 173 says removed; the manifest still has it) [O].
- Stores to the SHARC switch tables through computed addresses; the DN2 block-handler and RPC tables; register liveness at the DN2 case labels; SDRAM cache behaviour on hardware [O].
- The UI-hook context labels come from direct call edges; virtual calls are not covered [O].


## ColdFire entry survey (static; DT2 1.17, DN2 1.12). Marks: [D] read once, [O] open.

Method: image-modules/points/*/module.toml, services/*/dks/module.toml, ports/fw/*.toml (hook docs), ports/bindings/reference.json
(addresses), `fw.py xref/callers/dossier`, call-graph reach (direct edges only, from out/cfdb; virtual calls are not edges, so
"reached from" is a lower bound; a throwaway reachability script, not kept).

### Facts that decide the question

1. The whole MAIN OS image runs from DDR (ACR0 0x40000000..0x47ffffff, finding 01/07; install.rs `write_code` + `flush_site`). Every
   code immediate, literal pool word, vector, table and function-pointer table of the image is writable at run time. "Flashed" only means
   "patched into the .syx before boot". [D]
2. The runtime installer (runtime/core/src/rs/src/loader/install.rs) runs at main-OS task entry (cf.rtos.site.main_os_task, DT2 0x400337ba,
   DN2 0x4002ed34), one IRQ mask per provider, writes `jmp.l entry` over the site, flushes lines, fills slot words. Nothing of the UI exists
   yet there, and DSP frames do not flow yet (docs/design/digikit-runtime.md l.145), so vector 191 has not run. [D]
3. Vector 191 slot (DT2 0x400002fc) is written at boot by FUN_4002d4f2 (boot init, before task entry) and sits in RAM: it can be redirected, but
   that moves the whole handler, not a site. [D]
4. Tick/LFO function table (DT2 0x402227d4, DN2 0x4020b7f8) is in DDR, read only by the tick (`lea` 0x40139818, DN2 0x40137cda) and the second
   reader (0x401394e6, DN2 0x401378ce). Writing entries 0..5 (option 1) can replace stock shapes but cannot add waves: the clamp to 0..6
   feeds three 7-entry state tables (finding 149), wave 6 is branched to RAND before the table, and slot 6 of the table is the first long of
   the slew table. New waves need the six compare sites. Redirecting the table pointer (an absolute `lea` operand) does not help. [D]

### Per-site table

Legend: ctx = execution context. Option 1 = data write of an existing indirect call, 2 = installer code patch at load, 3 = in image.

### core/lfo (image-modules/points/lfo): 12 stub ops (6 sites x 2 devices); today all flashed stubs + 12 pointer slots

| site | DT2 addr | DN2 addr | stock code | ctx | entry today | recommended |
| --- | --- | --- | --- | --- | --- | --- |
| tick_call | 0x40139804 | 0x40137cc6 | `moveq 6;cmp D3;beq` before `jsr (A1)` of wave table, in FUN_4013961e / FUN_40137a06 | interrupt, vector_191_handler only caller [D] (callers 4013961e: 1 site, 4002e91c) | image stub + slot | 2 |
| tick_slew | 0x4013986c | 0x40137d1e | compare before RAND slew code | same | stub | 2 |
| tick_start | 0x4013977e | 0x40137c40 | compare for RAND start phase | same | stub | 2 |
| read_call | 0x401394dc | 0x401378c4 | same compare in second reader FUN_401392d4 / FUN_401376bc | task: caller FUN_4012c590 / FUN_4012ac88 has 0 direct callers, pops a queue (queue_pop), address stored by FUN_4012d3b8 / FUN_4012ba7a = a task entry [D] | stub | 2 |
| read_slew | 0x4013954a | 0x40137932 | | task | stub | 2 |
| read_start | 0x4013944e | 0x40137836 | | task | stub | 2 |

Why 2 works: the port file already defines the six hooks with `exits` and liveness (`cf.lfo.hook.tick_call` ...); the lfo dks already imports
`hook.cf.lfo.hook.*.exit` names and holds the routines. A service `[[hook]]` over the 6-byte compare puts the routine in place with no image
stub, no slot, and zero cost for waves 0..5 when no service is loaded (stub costs 2 jmp + 6 insns per LFO, 48 LFOs x 1500 Hz). Interrupt
site: only vector_191_handler reaches the tick (one caller, [D]); the installer holds IPL 7 (irq_mask) so the handler cannot run mid-write,
and the handler has not started at task entry anyway. Remaining [O]: the vector 191 interrupt level is below 7 (a level-7 source would be
non-maskable); the task's second reader can be preempted only by interrupts, which are masked during the write. Port hooks must be
allowed to have the exits for both devices (already resolved, ports/fw/lfo.toml).

### core/rows (image-modules/points/machines)

| site | DT2 / DN2 addr | stock fn | ctx | entry today | recommended |
| --- | --- | --- | --- | --- | --- |
| hook.dispatch (op hook, `mf_dispatch`) | 0x400c89a4 / 0x400c25f2 | machine descriptor lookup | task/UI: callers SourcePageView::vfunc_47, kit and page code (7 sites DT2) [D]; never reached from the vector 191 root in the graph [D] | image | 2 (answer added types from the service rows) |
| hook.list.entry (`mf_list_entry`) | 0x40052a08 / 0x4004d8b6 | machine list builder | task, from MachineListView ctor and MachineSelectionView (reached from main_os in graph) [D]; runs after task entry | image | 2 |
| hook.list_rank_insert | 0x400522be (DT2 only) | sort comparator std::map insert | task, same chain | image | 2 |
| write list.alloc/operands (4 writes) | 0x40052a2c,0x40052a4c / 0x4004d8e0,0x4004d900 | list builder immediates | same | image | 2 (plus buffer allocated at load) |
| writes name.long/short/hint (6), permit (2), sound_load_type_bound (1), group (2, DT2), pertype.0..5 (12 writes, DT2) | see ports/bindings/reference.json cf.machine.site.* | bounds `moveq #last` and table-pointer literals | accessors are task code; `permit` 0x400da514 is also reachable from vector 191 and from the boot root by direct edges [O] | image | 2: bound + table pointer written under one mask; at task entry no reader is live. Tables (mf_names, mf_permit, mf_pertype, mf_list_buf) must come from a runtime-allocated area instead of cf.expansion |
| stub frametype (DN2, slot `frametype`) | 0x4002757c | frame builder reads track machine type into TX frame word, inside vector_191_handler | interrupt (only) [D] | image stub + slot | 2 (hook `cf.machine.hook.frametype` already has regs and exit in port; service already resolves `...frametype.exit`) |

The machines service already takes 13 more machine/page hooks at load (DT2: clone.5, param_code.args, srcpage + source_draw/code/key/turn;
DN2: group, type_page.count/desc, param_code.map, type_reader.load, owner_type_load + the four page hooks). Those are option 2 and
confirmed: each is `[[hook]]` without `handler`, installed by install.rs. [D]

### core/rows (image-modules/points/dynreg)

| site | addr DT2 / DN2 | what | ctx | entry today | recommended |
| --- | --- | --- | --- | --- | --- |
| list.copy_target (DT2 retarget of `jsr copy`) / copy_call (DN2, 6 bytes `jsr (A3);move.l`) | 0x40052b46 / 0x4004d994 | tail of the list getter (inside list builder FUN_40052a08 / FUN_4004d8b6) | task (MachineListView ctor) | image | 2 |
| view.build hook (`dynreg_view_hook`) | 0x40060f50 / 0x4005b2c0 | entry of machine selection view item builder | task | image | 2, or delete: digikit-runtime.md l.173 says the view hook and rebuild are removed with the archive commit, but module.toml l.118-124 still has the op and the build contains `dynreg_view_hook` [O: stale doc or stale op] |
| 8 machine rows, 16 param rows, 8 page rows (data) | data only | reserved rows | | image | stays as data (the registry area), not an entry |

### core/rows (runtime/platform/rows)

| site | addr DT2 / DN2 | what | ctx | entry today | recommended |
| --- | --- | --- | --- | --- | --- |
| hook.value_class (offset 4, before `cmpi.l #count`) | 0x400c8932 / 0x400c2580 | value-class accessor | task, 15 callers (ParameterSet vfuncs etc., 0x4003a768 ...) [D]; no direct edge from vector 191 root [D] | image | 2 |
| writes cf.param.sites.descriptor_table.ref.* (absolute pointers into the 60-byte-row table) and .bound.* (`cmpi.l #N`) | first 0x4003a7ea / 0x400174e0; bound 0x4003a7d8 / 0x400174d6 (ParameterSet::vfunc_22 etc.) | repoint to the copied table (pf_table) and grow bounds | task (ParameterSet vfuncs) | image | 2 (bulk writes; the table copy needs the runtime-allocated area; the copy is made from live RAM at load, which equals the image bytes since nothing writes the table) |

### core/fxpages (image-modules/points/fxpages): no image ops (data only: 24+2 param rows, `fxpages_info`). Nothing to move.
services/fx/dks: 9 `[[hook]]`s (page_desc 0x400c898a/0x400c25d8, tab_label_test, filter_graph_test, fx_view_list.load, fx_key_list.load,
fx_array.load/save/init_exit, sound_load_exit): all option 2 today, all UI/storage task code (reach: main_os or the console task; FxPageView::vfunc_4)
[D]. Two DSP fills (`fx_entry`, `fx_send`) are DSP-side.

### core/sample-pool (CF parts)

| site | DN2 addr | what | ctx | entry today | recommended |
| --- | --- | --- | --- | --- | --- |
| hook.boot_wire_size (retarget the wire length getter call) | 0x400cf08c (in FUN_400cedfc) | boot-only: wire size 0xabc to 0xacc | boot thread / link init, before task entry [D] (reached from the idle-site root) | image | 3: runs before the card is readable; paired with the DSP wire-size write (`dsp.link.site.boot_wire_size`, DSP code) |
| hook.reply_copy (retarget the copy that stores the reply) | 0x400cf956 (in FUN_400cf922; stock fn 0x40134770) | reads the 16-byte ack extension after the copy | interrupt: FUN_400cf922 reached from vector 191 root [D] | image | 3 with boot_wire_size (extension exists only if the wire is 0xacc from boot; mixing is unsafe). Could be 2 only if wire size were not changed at boot [O] |
DT2: no CF hook (borrows stock pool).

### Other service [[hook]]s (confirm option 2)
- project: slot_set, save_exit, clear_one_exit, clear_many_exit, paste_exit (5, both devices; addrs in reference.json cf.project.site.*): task code; option 2 [D].
- shortcuts: key_event (cf.input.site.key_event_walk DT2 0x4011c79c, DN2 0x4011e110; reached from main_os): option 2 [D].
- lfo UI hooks wave_name (DT2 0x400e1b68 area; DN2 two formatters 0x400e3610, 0x4000766c), wave_name3, graph_vec: UI task; option 2 [D].
- samples: no CF hook; two slot fills into the sample-pool adapter (pool_done/pool_lost), written by the installer: option 1 style (slot word).

### Counts (CF sites, per device-op as listed)
- Option 2 hooks today in services: fx 9, machines 13 (DT2 7, DN2 10 incl. shared), project 5, shortcuts 1, lfo 3 = 31 hook entries; plus 14 slot fills (lfo 12, samples 2).
- Flashed today and movable to option 2: lfo 12 stubs, machines 5 hook ops + 1 stub + about 30 write ops, dynreg 3 hook ops, params 2 hook ops + 4 bulk write ops.
- Must stay option 3: sample-pool boot_wire_size and reply_copy (DN2), the main_os_task boot hook itself (the installer's entry), and the image-side tables/areas (expansion rows).
- Option 1 (existing indirect call already used): none of the site hooks qualifies. Candidates only for data: vector 191 slot, tick function table entries 0..5, slot words.

### Open
- [O] vector 191 interrupt level vs IPL 7 (confirm in INTC setup, FUN_4002d4f2 region).
- [O] boot-root reach of `permit` (DT2 0x400da514) by direct edges: confirm whether a pre-task-entry reader uses the permit table (then bound and table pointer would be stock at that time and new later: acceptable, but check).
- [O] dynreg view hook stale (see above).
- [O] DN2 second-reader task (FUN_4012ac88) entry registration and DN2 vector 191 slot writer, not read.
- [O] Virtual calls are not in the direct-edge graph: contexts for UI hooks above are from callers and names, not a full context proof.
- [O] Machines tables and param table copy need a load-time allocator (cf.expansion today).

## Table survey: what can a card-loaded service own (DT2 1.17, DN2 1.12), static only

Marks: [D] read once here, [O] open, [V] taken from an existing verified finding (not re-checked here).
Commands: `fw.py xref/callers/decomp/path` on both images; anchors from `ports/bindings/reference.json`;
module.toml of image-modules/points/* and runtime/platform/dyndsp; findings 02, 30, 33, 61, 132, 135.

### Global facts that decide most rows

- G1 [D/V] All ColdFire code and const tables (0x401xxxxx/0x402xxxxx) run from DDR. ACR0 maps 0x40000000-0x47ffffff copyback, NOT write-protected
  (finding 01 l.361); no MMU write-protect found. So "flash-mapped" tables are plain writable RAM at run time. Writes to code/immediates
  need the runtime's cpushl flush (finding 135 Q5, tramp.s). The only reason tables cannot grow is that neighbours are packed: no spare room.
- G2 [D] Main-OS task entry: DT2 FUN_400337ba (main_os_task), DN2 0x4002ed34. In that function (decomp line order): TimerManager, Brain, MmcFs ctors
  first (line ~45), MainScreenView ctor at line 222, THEN the message-queue wait (line 227, FUN_40001928). `path 0x400337ba -> 0x40060f50`:
  MainScreenView -> KitBrowser -> MachineSelectionView -> FUN_40060f50 (5 hops). So every view (incl. the machine list view) is built after the entry,
  in the same task, before the first UI message. A service that fills tables at the entry (before the MainScreenView ctor) is seen by the stock
  builders with no rebuild. Hooks of 135/132 confirm entry installs are safe. [O] exact hook point of the service load relative to line 222.
- G3 [D/V] Before the entry only the boot project decode loop and static initialisers read these tables (finding 132: param_code runs
  pre-entry, 8 iterations per group, never saw a digikit type). The DN2 page/machine descriptor builder FUN_400c2c4e has 0 callers = static
  initialiser (init array) at boot; DT2 value-class builder FUN_401be1be likewise (0 callers). Those build bss objects from STOCK rows only.
- G4 DSP side: dyndsp/dynfx rows are SHARC memory the runtime writes one word per transfer through dsp-link windows; the rows are already
  runtime data; only capacity (number of free rows, a DSP-image constant and a bound the dispatch code reads) is flashed.

### Table by table

### 1. core/rows (image-modules/points/machines, 36 [[op]], 6 [[section]])

| Table | Stock addr, size, memory | First read / frequency | Reach | Verdict |
| --- | --- | --- | --- | --- |
| Machine descriptor rows (DT2 0x4293b960, 7 x 0x2c; DN2 0x42433b24, 5 x 0x2c; fallback row = last stock) | bss, built by static init (DN2: FUN_400c2c4e, 0 callers) | every param/SRC page draw (DT2 dispatch FUN_400c89a4: 7 call sites in 6 fns, UI + load); many times | already one function `dispatch`, hooked (`cf.machine.hook.dispatch`): types >= stock go to registry rows | A once the adapter reads the registry base from a pointer word (today absolute symbol `machines.rows`, bound `moveq #last` patched at flash). Confidence medium-high |
| List order table `mf_list_stock` (DT2 0x401f5458; DN2 0x401de168) | const in DDR; DT2 2 refs (FUN_40052a08, FUN_40052c98), DN2 2 refs (FUN_4004d8b6, FUN_4004dae2) | first read inside MachineSelectionView ctor (FUN_400610e8 -> FUN_40052c98 local static init with guard; getter FUN_40052a08 from FUN_40060f50, MachineListView ctor, FUN_40052b5e). After G2 entry. List is built once and cached in the view (`view+0x184`) | 2 abs refs + alloc/operand sites (`list.alloc`, `list.operands`) + copy hook | A if service loads before the MainScreenView ctor (rebuild not needed). If later: rebuild by re-calling FUN_40060f50 (proven in emu, finding 30, via `dynreg_view_hook`). Rank std::map (DT2 `list_rank_insert` site 0x400522be) is built at the first getter call, so also fresh if early. High for the early case |
| Name table `mf_names` (DT2 0x4020ef78, DN2 0x401f7c04) | const DDR; 2 xrefs in 1 fn (long: 0x400da6ac / 0x400dc496) + short/hint sites 0x400da6d2, 0x400da6f8 | UI draw only (finding 135 Q7: no row pointer cached) | 3 accessor sites, each bounded by `moveq #last,D1` | B-small (3 accessors, 6 patch words) or A with pointer indirection in the adapter's accessor code |
| Permission mask `mf_permit` (DT2 0x4020f0ce, DN2 0x401f7d42) | const DDR; 2 xrefs in FUN_400da514 / FUN_400dc2fe | permission gate, reachable from UI and project task | 1 fn, bound `moveq #last,D2` | B-small (1 fn) / A with indirection |
| Per-type byte `mf_pertype` (DT2 only, 0x401eda30) | const DDR; 12 xrefs in 6 fns (SourcePageView vfuncs 40016ccc, 40016d60, 40017728, 40017ed0, 400181fe x2...) | SRC page draw/key (UI pass) | 6 bounds + 6 refs (`cf.machine.site.pertype.0..5`) | B (12 sites). Alternatively keep stock copy sized for N rows: C-ish only for capacity |
| DSP selector word table `mf_selector` (DT2 only, DSP 0x2567c0 / 0x282567c0, DSP L2) | SHARC data, one ref (`dsp.machine.value.selector.table_ref`) | per block per voice on the DSP | DSP image; the table is in the DSP image loaded at boot | Row write through dsp-link window: A for rows that exist (reserved types already have type-0 selector); capacity C (DSP image). [O] whether the table has slack and where the bound sits |
| DN2 type reader copy `mf_reader_real` + 9 call sites (`type_reader.call.0..8`, load 0x4004b816, tail_jump) | code | per track play/draw | 11 sites | C (hooks into stock code; unchanged by rows), already generic |
| DN2 page tables (`0x42433ad4` rows of 16 B x 5 types, `0x42c65d18` 5 x 40 map) | bss + heap vectors, static init | UI page draw | page_count (0x400c2636) and page_desc (0x400c2652) hooked; 2 xrefs each | A: service answers pages from own rows (hooks `type_page.count/desc` already generic) |

Machines overall: the adapter is already mostly "hook + registry". Everything written as data at flash (clone rows, copied tables with one row appended)
could be pointer-indirected in the flashed adapter code once; afterwards the service allocates rows in arena memory and fills a pointer+count
(A). Cost today: the copies of names/permit/pertype with appended rows exist only to keep the stock readers bounded and to give each added type a row.
Confidence 0.7 (not checked: every reader of the type byte beyond those anchors).

### 2. core/rows (6 [[op]], 2 [[section]] pf_table)

- Stock: DT2 descriptor table 0x4020f59c, 275 rows x 60 B (0x113 count from `cmpi.l #0x113` at 0x400c8932, accessor FUN_400c892e); DN2 0x401f83a4, 321 rows. Const data in DDR, no slack (next table at 0x40211a68 is another object).
- Readers: 56 absolute references + 54 clamp bounds `cmpi.l #N` per device (anchors `cf.param.sites.descriptor_table.ref` / `.bound`; xref shows ~70 hits at
  ParameterSet::vfunc_8/22/26/27, Sound/Fx/Midi/TrigParameterSet, ParameterPageView, FUN_40048664, FUN_4004ff12 ...). Called per knob turn, per page draw,
  per p-lock / param set; project load.
- Derived state: value-class objects (275 objects of 0x44 bytes, static init FUN_401be1be pre-entry, accessor FUN_400c8952 T2 hook `pf_t2` already maps new codes to a template's class); defaults refresh and per-owner lookup maps built from stock rows only and end on
  `bne/beq` with the stock count: added rows do not enter them (module.toml comment). So a service fill after entry has no derived state to rebuild
  except what the adapter already handles.
- Verdict: capacity growth = (B) 110 load-time patches per device (56 words + 54 immediates) with flush, or (C) keep the flashed copy but make it large
  (max=64 rows today -> e.g. 256 rows = 15 KB of cf.expansion). Row content is already (A): the runtime writes reserved rows (finding 30, verified in emu).
  Pointer indirection is not available: all readers use absolute addresses. Recommendation: reserve generously at flash (cheap) rather than patch 110 sites at load. Confidence 0.8.
  [O] whether the 54 bounds are all of the form `cmpi.l #N,Dn` writable by one rule at load (they are today by one `write` op each, so yes, mechanically).

### 3. core/rows (machine rows 16..23, param rows 16, DN2 page rows 8; dynreg_info)

- Rows are content of tables 1 and 2 above; `dynreg_copy` (DT2 0x40052b46 jsr; hook target) / `dynreg_list_hook` (DN2 0x4004d994) filter empty-name machines at the end of the list getter; `dynreg_view_hook` at FUN_40060f50 entry remembers the view so the runtime can rebuild the list (call FUN_40060f50 again; emulator-proven, finding 30).
- Timing: list built once per boot (G2); rebuild path exists and ran without fault in emu; heap/cursor over many rebuilds not examined [O].
- Verdict: reserved row count (A for content, C for count unless tables 1-2 move to pointer form). The rebuild question is answered: yes, a service can fill rows
  and call the stock rebuild; if it loads before the MainScreenView ctor, no rebuild is needed and `dynreg_view_hook` can go. Confidence 0.8.

### 4. core/fxpages (0 [[op]], 0 [[section]])

- Already service-owned: the nine firmware sites are taken by fx.dks at load (module.toml l.22-25). The firmware page descriptor table (33 rows x 0x2c, RAM built at start, no free row, bound 33 in the accessor) is bypassed: page numbers >= `[fx] page_first` answer from the service's own table.
- What stays flashed: 24 reserved parameter rows `params.fp0..` + 2 send-page rows (rows of table 2) and `fxpages_info`; kit effect values in spare halfwords of the kit storage record (no table).
- Verdict: A already for pages; param rows follow table 2 (flash capacity). Confidence 0.8 (not re-read the hook code).

### 5. core/dynfx (SHARC effect chain rows, `dynfx_info`)

- 3 free rows (pc 0xffffffff, skipped by the adapter) in the fx chain `fx.rows`; claimed/freed by the runtime one word per transfer through dsp-link window 2. `cap_fx_slots` = `dynfx_info + 20`.
- Per audio block: the chain scan reads the rows. Row content: A (already runtime-written). Capacity: C (DSP image row count and the adapter's bound), unless the chain table has slack after it [O: look at `fx.rows` neighbours with tools/sharc_dev/sharc.py].

### 6. runtime/platform/dyndsp (voice rows, DSP address table)

- 8 free voice rows (type 0xffffffff, pc 0) in `voices.rows`; `dyndsp_info` (CF data) holds area base and exported DSP symbols. Render scan per audio block skips free rows.
- Same verdict as 5: content A, capacity C (8). The address table is CF data in the image (A in principle: a pointer word; it holds only addresses the image already defines, so it cannot move to a service that does not know them: it is the port data, keep flashed (C), it is small).

### Decision summary

| Table | Verdict | Conf |
| --- | --- | --- |
| machines dispatch/descriptor rows | A with pointer indirection added once in flashed adapter code | 0.7 |
| machine list + rank (early load) | A (service loads before MainScreenView ctor); rebuild via FUN_40060f50 proven otherwise | 0.8 |
| names / permit | B-small (3 + 1 functions) or A with indirection | 0.7 |
| pertype (DT2) | B (12 sites) | 0.7 |
| DSP selector (DT2) | A for existing rows, capacity C | 0.5 |
| DN2 page tables | A (hooks generic) | 0.7 |
| params table | capacity C (reserve big) or B 110 sites; rows A | 0.8 |
| dynreg | content A, count follows machines/params | 0.8 |
| fxpages | A (done); param rows follow params | 0.8 |
| dynfx, dyndsp | content A, capacity C (DSP image) | 0.6 |

### Open

- [O] Exact service-load point vs MainScreenView ctor (decomp line 222) on DT2 and DN2; does the card become readable before it? (MmcFs ctor is at the top, line ~47.)
- [O] DSP table slack and the bound location for selector/voices/fx chain.
- [O] Whether any pre-entry code (boot project decode) clamps against the param table with a larger-than-stock count if a saved project holds digikit params (finding 132 saw none).
- [O] The three DN2 anchors beyond the ones listed (FUN_4004dae2 second list reader, not read here).

## SHARC entry survey (static), DT2 1.17 and DN2 1.12

Marks: [D] read once here, [O] open. Tools: `fw.py callers/disasm/sql -i IMAGE --cpu sharc`, a throwaway reader (LoadedMemory read of the boot stream, alias 0x28000000 + DM word address; SDRAM tables by 0x80xxxxxx byte address), findings 06, 10, 49, 96, 148, dn2-stereo.md 1.5, 128.
Addresses are SHARC short-word pcs. Computed jumps found with: `sql ... from edges e join insn i ... where kind='indirect'`, real ones are Type 9b with pmm=5 (M13), j=0 ("JUMP (M13,I12)"); M14 + delayed ones are returns (finding 06 "12 indirect calls are returns").

### 1. Result in one paragraph

No hooked site is reached through a pointer or table that carries the call we hook. All 28 dsp-voice hooks, both fx hooks, the block tail and the sample-pool DSP sites are direct CALL/JUMP immediates or mid-function addresses [D]. The computed jumps that exist are compiler switch tables in the boot stream: read-only initialised data, one reader literal each, no stores found, entries are labels inside the same function [D]. They select a case body, not a callee, and they are guarded so that new machine types never reach them. So option 1 gives nothing for the existing sites. The one generic entry worth noting is the RPC command table (section 4).

### 2. Per-site table

Legend for the last column: O1 = redirect indirect pointer, O2 = code patch at load (CF installs; SHARC needs the unproven DSP installer), O3 = flashed in image.

### core/dsp-voice (28 hooks: 3 + 25)

| site (hook name) | dev | stock code at the site | stock path to it | option |
| --- | --- | --- | --- | --- |
| dsp.voice.hook.render_entry (fn render_entry, first insn `I7=MODIFY`, 48 bit) | dt2 0x1c4ecf, dn2 0xb8251b | per-voice render/copy function (84 / 55 insns, leaf, no callees); DT2 called once per voice buffer from chain driver FUN_1c642a at 0x1c6b00, plus tail JUMP from 0x1c4f81; DN2 called from engine FUN_1c8ef1 at 0x1c9459 (after FM synthesis), plus JUMP from 0xb8258f [D] | direct `CALL imm`. The type test is NOT a table read: DT2 per-track selector (word track_base+0x4c, from selector table 0x2567c0 = 0 1 2 3 4 0 5, loader data, one reader 0x1c2731) feeds three switch tables in FUN_1c642a (below) but the render call at 0x1c6b00 is outside them [D, finding 06/49]. DN2: same, type word DM(I0-12) selects tables at 0x8052db90.. but the call 0x1c9459 is direct [D] | O2 (hook today). O1 not available: no pointer exists. O3 fallback |
| dsp.frame.hook.frame_rx.unpack_call_a / _b | dt2 0x1c2b24+0x176 (byte alias 0x28385934), +0x185 (0x28385952) | the two direct CALLs of per-track unpack FUN_1c24e9 per track in the frame consumer's 16-track loop; unpack turns frame words into voice params (knob 7 -> level) [D] | direct `CALL 0x1c24e9` x2 | O2 / O3 |
| 25 DN2 call-site hooks (first_order_process, first_order_reset, uy_process early/late, uy_reset, gain_stage, case5, held_sample early/late/reset, shaper early/late, filter_case1/2/3 + reset, ring_stage case4/case6 + reset case4/case6, case5 reset) | dn2, inside FUN_1c8ef1 (0x1c8ef1..) and continuation fragment 0x1c9b73; offsets in dn2-stereo.md 1.5 | each is one linked, delayed CALL of a stage function (b80c39, b80b93, b81368, b8135a, b8265b, 1cdb56, b82342, b81a16, b80f2e, b806f5, b81526, b80345 ...); hook = retarget of the call | direct `CALL imm` with both delay slots [D]. Cases 1..6 (filter_case1/2/3, ring 4/6, case5, and their calls at 0x1c98af, 0x1c9bb6, 0x1c9bd1, 0x1c9be9, 0x1c9c08, 0x1c9c4f) are entered through the 7-entry switch table 0x8052dbc0 = {0x1c98b9, 0x1c98ab, 0x1c9ba9, 0x1c9bc4, 0x1c9bdf, 0x1c9bf7, 0x1c9c45} [D] | O2 (each retarget). The six case calls could in principle be reached by redirecting a 0x8052dbc0 entry to a stub that jumps back to the label (O1-like), but the stub runs before the stage and not instead of the call, so twin semantics (R twin after left stage, blend) are lost; [O] not examined |
| dsp.voice.hook.pan_slot, pan_pre | dn2 engine 0xc05 / 0xbbe (pcs 0x1c9af6 / 0x1c9aaf) | mid-function replace of the pan-loop body and the instruction before the fade-flag clear | mid-function, no entry | O2 / O3 (replace at 6 / 4 bytes) |

Switch tables (all read-only loader data in SDRAM, boot-stream block 101 DT2 / block 93 DN2; no store literal found; one `JUMP(M13,I12)` reader each) [D]:

| table | dev | address | width | entries | reader | guard |
| --- | --- | --- | --- | --- | --- | --- |
| A | dt2 | 0x8055c840 | 32-bit pc, 15 entries overlapped by B, C | 0x1c65bd 0x1c6715 0x1c6782 0x1c686e 0x1c692f 0x1c6992 ... (labels in FUN_1c642a) | FUN_1c642a 0x1c6579, selector < 6 (compu at 0x1c6553/0x1c6561) | selector >= 6 skips |
| B | dt2 | 0x8055c858 | | 7 entries | 0x1c66ec | selector < 7 |
| C | dt2 | 0x8055c874 | | 7 entries | 0x1c6c25 | selector < 7 |
| D | dn2 | 0x8052db90 | | 0x1c90b2 0x1c91dc 0x1c920b 0x1c922f 0x1c90d4 (5; FM TONE, WAVETONE, FM DRUM, SWARMER, MIDI) | FUN_1c8ef1 0x1c90b0 | `compu(type,5)` skips >=5 (0x1c909f) |
| E | dn2 | 0x8052dba4 | | 7 | 0x1c9195.. | bound 7 at 0x1c9197 |
| F | dn2 | 0x8052dbc0 | | 7 (case 0..6) | 0x1c9595 | R1=7 compare 0x1c9581 |

Tables cannot grow in place (next table starts behind the last entry). Nobody writes them at run time as far as sharcdb literals show (only reader literals; [O] a store through a computed address is not excluded, finding 06 says whole-image scan found no table of synthesis targets and no writer).

### core/fx (2 hooks)

| site | dev | stock code | stock path | option |
| --- | --- | --- | --- | --- |
| dsp.fx.hook.entry (fn entry, first insn `I7=MODIFY`) | dt2 0x1c80f2, dn2 0x1c244d | post-master function: runs once per block after master mix and before USB pack; DT2 block copy of 25 words' range, DN2 includes USB pack | DT2: `CALL` from orchestrator FUN_1c2b24 at 0x1c311f (+ tail JUMP from glue 0x1c815d); DN2: one caller, FUN_1c2712 at 0x1c307b [D]. No pointer list of master stages: orchestrator calls its stages as immediates | O2 / O3 |
| dsp.fx.hook.send (fn send) | dt2 0x1c207b, dn2 0x1c1f1e | master function (reads track/slot buses, FX return bus, mixes) | DT2 called at 0x1c3099 plus 11 tail-JUMP callers from 0x1c238a (the function is also a shared entry); DN2 called at 0x1c305d plus 11 jumps from 0x1c22de [D]. Direct | O2 / O3. Note 11 jump callers share the same entry address, so a patch at the entry also runs on those paths (the present hook does) |

### dsp.master.hook.q31_converter.call (not in the asked list, same family)

dt2 pc 0x1c7734 (byte alias 0x2838ee68), dn2 0x28393fa4: the block handler's one CALL of the Q31 converter, direct; O2.

### core/sample-pool DSP sites (DN2 only)

| site | stock code | path | option |
| --- | --- | --- | --- |
| dsp.cache.hook.cache_enable (byte alias 0x20017358, boot region) | boot cache configuration after RR3 cleanup, before final CFG enable/invalidate; displaced `R2=DM(CFG)` | boot code, runs once before the RTOS and long before the ColdFire can talk to the DSP | O3 only (runs before any card service exists); [D] |
| dsp.link.site.boot_wire_size (op write, 4 bytes `840fcc0a`) | immediate R4=0xabc before SPI2 descriptor build | boot-only literal | O3 only, a data patch (32-bit-sized, atomic) [D] |
| the block_tail call `_sp_tail` | see block_tail | | follows block_tail |

### runtime/platform/dsp-link: dsp.link.hook.block_tail

| item | value |
| --- | --- |
| site | dt2 pc 0x1c75c1 (alias 0x2838eb82), dn2 pc 0x1c9e5f (alias 0x28393cbe) (second last insn of the block handler's common tail, `DM(ring toggle)=R1`) |
| stock code | block handler FUN_1c74cd (DT2, called from Audio Task loop 0x1c7749 after notify-take 0xb86b1e) / FUN_1c9d6b (DN2, handler entry 0x1c9e76 per link README). Reads command from `0x264220 + (DM(0x261ca4)<<12)`, dispatches, all paths rejoin at 0x1c758b and end in the tail, then bit 0 of flag (DM 0x25f780 DT2 / 0x268a38 DN2) toggles [D, finding 06 lines 3305-3318] |
| table | command table DT2 0x25f7b0 = {0x1c7524, 0x1c75d8, 0x1c763c, 0x1c7671} (4 x 32-bit, loader block 31, labels in the same function: cmd 0/>3 clear, 1 clear, 2 loopback, 3 render). Read by 0x1c7521 `JUMP(M13,I12)`; only reader literal 0x1c7501. DN2: the matching handler table is at I4 set earlier (not a literal at the jump 0x1c9dbf) [O]; a different DN2 table 0x25d968 = {0x1c29ac, 0x1c3219, 0x1c3381, 0x1c34f0} belongs to FUN_1c2712 (0x1c299c..0x1c29aa), not shown to be the block handler [O] |
| option 1? | no. The table entries are command-path labels that run BEFORE the tail; the link needs to run after the reply record is built. The tail is a common join, so it needs a patch. Also the transport cannot install itself (finding 128). O3 (it is the bootstrap hook) |

### 3. Counts

dsp-voice 28 hooks, fx 2, sample-pool DSP 2 (boot hook, boot data write) + block_tail 1 (+ q31 converter call 1 not asked):
- Option 1: 0 certain. 0 hooks have a pointer path.
- Option 1 partial / unexamined: 6 DN2 dispatch-case call sites via table F (twin semantics lost, [O]).
- Option 2 (patch at load, direct CALL/mid-function): 2 fx + 28 voice - 0 = 30; of these the render_entry and fx hooks are single-instruction 48-bit patches.
- Option 3 only: cache_enable, boot_wire_size, block_tail (3).
- Unknown: DN2 handler table (I4 not literal), RTOS task entry pointers [O].

### 4. Generic entries that DO exist as run-time-readable tables

1. DT2 RPC command table 0x2577c4: 11 x 32-bit {0x1c3c3e, 0x1c3c4f, 0x1c3c72, 0x1c3c95, 0x1c3d0a, 0x1c3d54, 0x1c3d75, 0x1c3db0, ...} (loader block 23). Reader FUN_1c3bef 0x1c3c3c: `I12=DM(I1,M4)` then `JUMP(M13,I12)` after `compu(R2,R15)` bound check at 0x1c3c2b. The ColdFire reaches it with the stock RPC. Documented in finding 06 line 1000. This is a ready command dispatcher on the DSP, but growing it needs a write (it is flash/boot-stream data: entry swap is an option-3 data patch, 32-bit, atomic) [D for table/readers, O for the swap]. DN2 equivalent not located [O].
2. DT2 0x256c98 (4 entries 0x1c351c 0x1c352f 0x1c353e 0x1c354d, FUN_1c3504 jump 0x1c351a; index R4) [D finding 10], wrappers that call 0x1c349b, 0x1c347e, 0x1c3474, 0x1c3464. Not on a hot path; [O] what R4 is.
3. DT2 per-track selector table 0x2567c0 (read at 0x1c2731) and 0x2567a0: the only data tables keyed by machine type; already relocated by the machines framework (finding 02).

### 5. Can the ColdFire write DSP data memory at the addresses of such tables?

No, not with dsp-link as built [D, cf/dsp-link.c lines 147-190, dsp/dsp-link.inc lines 24-40, 184-223]:
- window 0: slot block (DL_SLOT_WORDS words just below `_dl_area`) and the reserved area `_dl_area` (512 KiB DT2, 320 KiB DN2) in dsp.l2. A write may not leave either; the DSP refuses a chunk that crosses word 16.
- window 1: voice rows `_dv_table` (counted `_dl_vcount`), window 2: fx rows (`_dl_fcount`), window 3: 3 stats control words. The DSP checks offset against the window size (status 3 = out of range, transfer stopped).
- all copies are made by DSP code in the block tail with DM stores into those windows. Stock tables (0x25f7b0, 0x2577c4 in internal L2; 0x8055c840, 0x8052db90.. in external SDRAM, which only the DSP addresses) are outside every window. To write one a new DSP-side window or "poke" command is needed in the receiver (our code, in the tail), plus for SDRAM no cache issue is modelled in the emulator [O]; for code addresses the 48-bit non-atomic write and I-cache invalidate remain unproven (finding 128). For 32-bit data entries (jump tables, RPC table) the write is a single 32-bit store: atomic against a table read [D reasoning], but whether the read hits a stale D-cache line for SDRAM tables is [O].

### 6. Open questions

1. Does anything store to table A..F or 0x25f7b0 / 0x2577c4 at run time (computed-address stores)? A `mem_access` scan with base-register tracking is not done [O].
2. DN2 block handler dispatch table (I4 set before 0x1c9db2) and DN2 RPC table location [O].
3. RTOS task entry pointer of "Audio Task" (0x1c7749): a task-creation pointer (written at boot, RAM) is a candidate option-1 entry for the whole block handler; not looked at [O].
4. Whether jump-table redirects (case tables D/E/F) could host machine/stage dispatch with a stub that sees the same registers as the stock label: needs register-liveness at 0x1c98ab..0x1c9c45 [O].
5. Cache behaviour for SDRAM tables on hardware [O].
