# Firmware facts for hot reload

DT2 OS 1.17 and DN2 OS 1.12. Static reading; no emulator run. Used by [the hot reload plan](../plans/digikit-hot-reload.md). Marks: **[D]** read once, **[O]** open.

## Q5: instruction cache control

- Both images write CACR once, at 0x4000055a: `move.l #0xa50ce100,d0`. Finding 01 (line 360) recorded the constant. **[D]**
- Bits of the value: SPA (bit 14) = 1; IVO (bit 20) = 0; DDPI (bit 28) = 0; IDPI (bit 12) = 0; IEC and DEC = 1 (RM CACR figure, p0167). **[D]**
- RM Table 6-10 (p0189): with SPA = 1, `cpushl bc,(ax)` searches by physical address and clears the line in both caches. So the runtime flush by address (one `cpushl` per 16-byte line, tramp.s) is correct. **[D]**
- Our code must not write CACR.
- **[O]** Does the core need a pipeline sync after the last `cpushl` before it runs new code?

## Q1: which task runs a project load

- DT2: task entry 0x400caff0, created at 0x400cc7ce, TCB 0x4039be58, priority 2, stack 0x4039beac, size 0x4000.
- DN2: task entry 0x400cd5f2, created at 0x400cec02, TCB 0x40386e48.
- This task reaches `write_live_project` 0x400df95a and the functions of the hook sites `sound_load_exit`, `fx_array.load` and `init_exit`. `fx_array.save` is reached from the UI side (FUN_400dbdca, callers through view vfuncs). **[D]**
- Reachability over-approximates, and vtable calls are not edges. Which task runs them on a project load is **[O]**.
- Probe: PC probe at 0x400df95a, `fx_array.load` 0x400daf54 and `sound_load_exit`, logging A7 and SR. A7 in 0x4039beac..+0x4000 is the project task; A7 in 0x4096775c..+0x28000 is the UI task.

## Q2: the UI-loop hook site

- DT2 site `ui_loop` 0x40033d06 is inside `main_os_task` FUN_400337ba. The task is created at 0x40033280, TCB 0x40966ee8, priority 6, stack 0x4096775c, size 0x28000. It waits on queue 0x40966f3c (FUN_40001928) and switches on the message byte. **[D]**
- DN2: site 0x4002f17e, queue 0x4059e214, TCB 0x4059e1c0, entry 0x4002ed34, created at 0x4002e902. **[D]**

## Q3: who else reads the UI queue

- FUN_400337ba has no callers; it is a task entry only. No other consumer reads queue 0x40966f3c (DN2 0x4059e214). The other nine callers of FUN_40001928 wait on their own queues in their own tasks. **[D]**
- So modal popups and browsers run inside message handlers, and the UI-loop hook runs once per message. **[D]**
- Caveats: a handler could block on another queue; a queue pointer passed in a register would not show in refscan.

## Q4: level 7 interrupts

- Of about 35 absolute ICR writes (same on both devices), two set level 7. **[D]**
  - INTC0 ICR2 (0xfc048042) at 0x40001cbe: EPF2, vector 0x108, handler FUN_40001a10. It is a 516-byte leaf with no callees, so no hook site runs there.
  - INTC1 ICR40 (0xfc04c068) at 0x400d346e: EDMA_INTR (RM Table 17-16, p0348). The handler is installed by variable into vector 0x2a0 from pointer 0x43153a2c; it is not known statically. DN2: 0x400d56c6, 0x400d5996.
- **[O]** The EDMA handler. Probe: log 0x400002a0 after 0x400d34be, and the PCs entered from that vector.
- Level 7 cannot be masked (RM).

## Q6: empty machine rows

- Stock dispatch does not map an out-of-range type to type 0. The compare is unsigned, so -1 is out of range too. **[V]**
  - DT2 FUN_400c89a4: `type <= 6 ? 0x4293b960 + 0x2c * type : 0x4293ba68`. The fallback 0x4293ba68 is the row of type 6, the last stock machine.
  - DN2 FUN_400c25f2: `type <= 4 ? 0x42433b24 + 0x2c * type : 0x42433bd4`. The fallback is the row of type 4, the last stock machine.
- A patched image: the `mf_dispatch` of the machines adapter answers types in [first, count) from the registry rows and sends everything else to the stock tail. A gap row returns the gap row, without a fault. **[D]** (patched-image code; the verifier did not check it)
- Gap row content (machines `records.rs` `remove()`): empty name pointer, empty long and short names, knob fields 0, level field 10, clone_of 0, own_dsp 0; permission and per-type byte of type 0. **[D]**
- What a gap row plays:
  - DT2: the SRC page shows eight empty boxes (finding 30, emulator). The DSP selector word is the one of clone_of 0, so the stock DSP runs the sample renderer for the track. It is not silence if the track has a sample. **[D]** The PCM of this case was not measured. **[O]**
  - DN2: the type reader gives the DSP clone type 0 (FM TONE), and the page is the stock page of type 0. A type with own_dsp = 1 goes to the DSP as its own number; no engine knows it, and TRIG plays silence without a fault (finding 33). **[D]**
  - For a selector of 6 or more the stock voice stage is skipped (`compu(R6,6)`, `JUMP IF GE 0x1c65fe`). Whether the voice block is then zero or stale is **[O]**.
- Proposed MISSING placeholder for a type whose module is absent. **[O]** (proposal)
  1. The row stays a gap row (zero knobs, level 10, clone_of 0) with an empty name pointer, so the list builder keeps it out of the selection list.
  2. The long and short names in the name table and the descriptor become "MISSING" and "MISS". Which accessor draws the SRC title and the track label is **[O]**.
  3. Silence. DN2: own_dsp = 1 with no page is silent. DT2 has no row-only way. Options: (a) a selector word of 6 or more (needs a DSP memory write; zero or stale block **[O]**; test on the sharc rig first); (b) register a one-line silent routine for the type in dsp-voice (at most 8 rows). Prefer (b) if the rig shows stale audio with (a).
  4. A kit's type byte is never changed, so a reload puts the real row back.
- See also [finding 148](148-filter-machines.md) for the filter accessor, which falls back the same way.

## Q7: readers of the machine name table

- The permission gate FUN_400da514 and the per-type accessors are reachable from both the UI task and the project task. Name accessors are called only from UI views. **[D]**
- No code caches a row pointer: refscan of the name table shows only `lea` in the accessors. The list getter result is cached once in the view at boot (finding 30). **[D]**
