# UI and panel

Panel chords, the UI event queue, MACHINE SEL, timer-rate behaviour after boot, and where the GUI and a headless replay disagree.

## Driving panel chords: modifiers must latch **[V]**

A chord is not a press followed by another press. `emu/panelin.py`'s `held`
argument is a mask of other buttons *in the same channel*, and the modifier
keys are not in the same channel as the page buttons, so `held` cannot express
a chord at all:

```
code = channel*8 + bit + 1
FUNC = 17 -> channel 2, bit 0      SRC  =  2 -> channel 0, bit 1
YES  = 10 -> channel 1, bit 1      NO   = 12 -> channel 1, bit 3
UP   = 11 -> channel 1, bit 2      DOWN = 14 -> channel 1, bit 5
```

The wire carries each channel's whole 8-button state as one byte, so a
cross-channel chord is expressed by asserting one channel and *leaving it
asserted* while another changes — never by a press/release pair:

```
buttons(m, profile, 2, 0x01)   ; FUNC down, and leave it
buttons(m, profile, 0, 0x02)   ; SRC down, FUNC still held
buttons(m, profile, 0, 0x00)   ; SRC up
buttons(m, profile, 2, 0x00)   ; FUNC up, last
```

The firmware's own records confirm the difference: a plain tap gives flag
`0x01` on press and `0x10` on release, while the same button inside a latched
chord gives `0x03` and `0x12`, and the modifier's own release reads `0x00`.
Press/release pairs produce two isolated taps that no chord handler will ever
see. **[V]**

The descriptor's two name pointers are **`std::string`, not `char*`** — the
pre-C++11 libstdc++ copy-on-write representation, with a 12-byte header
immediately *before* the character data: **[V]**

```
data-0xc  length
data-0x8  capacity
data-0x4  refcount
data+0    chars, NUL-terminated
```

Read back live, the header is exactly that — `SAMPLE` at `0x44f25c7c` has
length 6, capacity 6, refcount 0; `STRETCH` at `0x44f25cfc` has 7, 7, 0;
`MANUAL SLICE` at `0x44f25ddc` has 12, 12, 0. Three of `FUN_400caf48`'s six
callers copy the whole 44-byte descriptor by value, calling a constructor and
destructor per name field — which is why they are non-trivial members rather
than pointers. The copy is `FUN_401d3aba`: **[V]**

```
401d3aba  move.l (A1),D0             ; the stored data pointer
          tst.l  -4(D0)              ; refcount
          bmi    deep_clone          ; refcount < 0 -> _M_is_leaked(), clone
          cmp.l  #DAT_44f1e088,...   ; the empty-string singleton, by address
          beq    skip                ; never refcount the singleton
          addq.l #1,-4(D0)           ; otherwise share: refcount++
```

So a new descriptor **cannot** point bare at a rodata string: the bytes before
it are not a valid header, and `*(int*)(ptr-4)` would be whatever happens to
sit there — either corrupting neighbouring data with a refcount increment, or
taking the release path against a bogus header.

It does not need a runtime-constructed string either. Laying the full rep out
statically in the cave as `[u32 length][u32 capacity][i32 -1]["NAME\0"]` and
pointing the field at the chars makes `refcount < 0` true, so every copy
deep-clones into the heap, the static bytes are never mutated, and destructors
only ever run against the clones. This is the same mechanism libstdc++ uses
for a leaked rep. Not yet tried. **[O]**

`FUN_4005e022` separately gates auto-scrolling the list to the active row on
`param_1[0x73] + 1 < 8`; that is cosmetic — an unpatched eighth row would fail
to auto-scroll rather than crash. **[O]**

The list vectors are rebuilt every time `MachineSelectionView` is constructed,
not once at static init: hooking `FUN_400607b2`, `FUN_40051fbc` and
`FUN_4005224c` on a run resumed from `snapshots/boot400M.snap` shows all three
firing ~60M instructions in. So a patch to the rodata tables or the descriptor
array can be tested by poking guest memory after a resume — no cold boot
needed. `FUN_4005e022` does *not* fire on an idle post-intro run, so the rows
are built but never drawn without navigation. **[V]**

Still open: what the literal IDs (`0xca`-`0xfe`) mean, and how `machine_type`
reaches the six callers. **[O]**

## The eighth row on screen, and a replay that disagrees with the GUI **[V][O]**

**Rung 1 is observed.** In `emu/gui.py` with all five parts (`--patch-machine`,
no `--weakptr`), MACHINE SEL stays open and scrolls to an eighth row drawn as
`PLACEHOLDER` — the display-name table's `Placeholder`, upper-cased at draw
time. It sits below MIDI with a dotted separator between them, so patch 3 does
put type 7 in a different group from MIDI; whether that is group `1` is not
visible from one screen. Seen by a person, 2026-09-14, at 248.6M
instructions. **[V]**

**A scripted replay does not reproduce it.** `tools/guirun.py --input`
(FUNC latched, then SRC through the GUI's own inbox and dwell pacing) opens
MACHINE SEL and then, 2-4M instructions later and with SRC still held, drops
back to the SRC page with a `ONE: ---` header (the track's machine and sample)
for about five seconds. The outcome is the same in every variation tried: **[V]**

| variation | list drawn | list gone |
|---|---|---|
| unpatched, `--weakptr`, SRC held 170-196M | 178M | 180M |
| all five, `--weakptr`, SRC held 170-196M | 176M | 178M |
| unpatched, no `--weakptr` | not caught at 2M spacing | `ONE: ---` by 180M |
| all five, no `--weakptr` | 176M | 178M |
| all five, SRC pressed late (250M) | 258M | 262M |
| all five, second SRC press (230M) | 240M | 242M |

No exception is thrown in any of them. So neither the patch, `--weakptr`, nor
press timing explains the difference, and an earlier version of this section
that called the auto-close "what a person sees in the GUI" was wrong. **[C]**

What the GUI session delivered that the replay does not is open. To settle
it, `emu/gui.py` now prints every panel feed it delivers as
`[gui] input --feed <instrs>:<hex>`, and `tools/guirun.py --feed` replays
those bytes raw at the same chunk boundary. A recorded session that keeps the
list open, replayed headlessly, either reproduces (and can then be bisected
event by event) or exposes a difference between `guirun.py` and the GUI. **[O]**

**The replay is faithful; the two GUI sessions are not the same run.** A GUI
session whose MACHINE SEL flashed was recorded (`[gui] input --feed`: FUNC
latch plus SRC tap four times, at 202M, 262M, 410M and 493M, plus one bare
SRC tap and one FX tap) and replayed with `tools/guirun.py --feed`. Every
feed landed on its recorded chunk, the replay's DTIM3/mainloop pairs match the
GUI's status lines at every 20M from 80M to 280M (`89/88`, `211/201`, ...
`1296/1272`), and the list flashes on screen at the same point, drawn at 210M
and gone by 212M. So `guirun.py` reproduces the GUI, and an idle boot is
deterministic between them. **[V]**

The earlier session in which the list stayed open had already diverged by
80M, before any recorded input: mainloop `98` against DTIM3 `87`, then `225`
against `212` at 100M, while the flashing session and every headless run have
mainloop *behind* DTIM3. It was run before feeds were printed, so what it
received is unknown; since an idle boot is deterministic, input during boot is
the likely difference. So whether the list stays open depends on state
established early, not on when FUNC+SRC is pressed. **[V][O]**

## MACHINE SEL closes itself: the UI queue falls behind **[V][O][C]**

Found with `tools/guirun.py --trace-ui` (`emu/uitrace.py`), which prints UI
queue sends and pops with their wait, each view a key event is offered to,
and view activate/close. Runs start from `snapshots/boot400M.snap` with all
five machine patches; instruction counts start at 0 there.

- The UI main loop (`mainloop`, `0x40033492`) pops the UI queue at
  `0x4094ef3c`. The queue holds item pointers, not copies: `+0x04` count,
  `+0x10` mask, `+0x14` storage, `+0x18` write index, `+0x1c` read index.
  Records are 16 bytes: byte `+0` type, long `+4` code, long `+8` flags,
  long `+0xc` timestamp. Type 5 is the DTIM3 tick (fixed item
  `0x4022aea6`); type 0 is a key event. **[V]**
- Headless, the queue never drains after boot. Depth is 1 at 80M, 10 at
  100M, 20 at 140M, 30 at 160M and 45 at 200M. The loop pops 114-121 items
  per 20M while DTIM3 posts 120-126. The wait in the queue grows from 0.4M
  instructions at 80M to about 5M at 170M and 7.4M at 200M. This is the
  `mainloop` < `dtim3` drift in the progress lines. **[V]**
- Key event flags: `0x01` press, `0x02` chord (a modifier is held), `0x08`
  auto-repeat, `0x10` release. Seen: `0x03` and `0x12` for SRC in a FUNC
  chord, `0x0b` for its repeats, `0x09` for FUNC repeats. Edges are queued
  from return address `0x40110d7c`, repeats from `0x401108f8`. The first
  repeat comes 24 counts of the counter at `0x47dc5a6c` (advanced by
  `FUN_40110820`) after the press, about 1.9M instructions, then every 8
  counts. **[V]**
- MachineSelectionView's key handler `0x40060c2c` calls View::close
  (`0x4010daa2`, returning to `0x40060c72`) for any event with code 1-6,
  whether press, repeat or release. NO (12) closes on release; YES (10)
  commits. **[V]**
- Path of a key event: case 0 of the main loop calls `0x401072bc` at
  `0x40033518` (A2 = item). Later in the same loop pass, `FUN_4010ecf2`
  offers the event to each view in turn (`jsr (a0)` at `0x4010ed64`; D3 =
  view, D0 after the call = consumed). For FUNC+SRC, 8 views pass and
  MainScreenView consumes it; MachineSelectionView is activated
  (`FUN_4010dc8a`) inside that offer, about 83k instructions after the
  pop. **[V]**
- Chord at 170M (`--panel-dwell 3`, SRC released at once): the SRC press
  was sent at 170.47M and popped at 175.78M (waited 5.31M). The SRC
  release was sent at 172.34M, before MachineSelectionView was activated
  at 175.79M. The release was popped at 177.64M, offered to
  MachineSelectionView, and closed it. With SRC held instead, the first
  auto-repeat (sent 172.53M) closes it. **[V]**
- Chord at 84-92M, backlog 2-4 items (`--panel-dwell 3` tap, `--panel-dwell
  3` with SRC held to 104M, and default `--panel-dwell 16`): the SRC press
  waited 0.30-0.49M, MachineSelectionView was activated, and no SRC repeat
  or release was queued after that, even with SRC held or released later.
  The list stayed open to the end of the run (130M) in all three. So the
  firmware stops generating that key's repeats and release once the view
  is active. Only events generated before activation reach the view, and
  they exist only because the backlog delays activation. **[V]**
- The GUI session that kept the list open had `mainloop` 12 ahead of
  `dtim3` with no drift, so its queue was probably empty when the chord
  came. That session was not traced. **[O]**
- Why the main loop cannot keep up: each popped item costs about 171k
  instructions on average, against a DTIM3 period of about 156k. Not yet
  known whether this is real UI work made too expensive by the 4.68M
  instructions-per-second timer rate (`docs/archive/HANDOVER.md`, lines 31-58,
  puts the real rate about 50 times higher), or an emulator artifact such
  as a busy wait or a slow peripheral model. **[O]**
- `--ips` above 4.68M, applied from `boot400M.snap`, stalls boot: still on
  the splash screen at 200M at 4x and 10x. Not a quick test. **[O]**
- `0x4005e022` is the MachineListView constructor: it calls
  `FUN_4010d946(this, "MachineListView")`. It still gets no hits in these
  runs; why is open. **[C]**
- The display-name accessor `FUN_400dcc50` is called from `0x4005faf2` in
  `FUN_4005fab8`, the per-row text callback stored in each MenuItem, not
  directly from `FUN_4005da40`. **[C]**
- View class names can be read from any view object through the Itanium
  RTTI layout: name = `cstr(*(*(vptr - 4) + 4))`, e.g. vtable
  `0x401e36f8` → typeinfo `0x401e36b4` → `"20MachineSelectionView"`.
  **[V]**

## A1 FUNC+SRC A/B is repeatable across state and profile lanes **[V]**

The experiment descriptor `experiments/a1-func-src.json` (removed; last present at tag `unicorn-reference-final`'s era, see git history) was run from `snapshots/postintro.snap` against
1.15C (`62d588456e47194bd56dfee9568fb9dd4521c4ff1e8b5427eb461355532e8c6c`)
as `out/experiments/a1-func-src/a1-real-002/`.  It delivered the raw FUNC/SRC
sequence `2201`, `2002`, `2000`, `2200`, requested an observation while the
chord was held at 14.4M, and ended at 40M.  Two fresh repeats of each
baseline/manipulated case passed in both the non-perturbing state lane and the
separately perturbing profile lane.  Every child returned zero, touched zero
fault pages, saved at the same actual boundaries (14,664,427 and 40,388,243),
and repeated its case's snapshot, panel, UiTrace and block-profile bytes.

- The four manipulated feeds landed repeatably at 624,018, 8,736,192,
  20,592,235 and 28,704,264 instructions.  UiTrace then shows a queue-send of
  `SRC(2) 0x03`, activation of `MachineSelectionView`, no subsequent SRC
  release/repeat or MachineSelectionView close, and a later queue-send of
  `FUNC(17) 0x00`.  This is the low-backlog behaviour described above: the
  raw `2000` proves SRC-up delivery, while the firmware suppresses its queued
  `0x12` release after the view activates.
- At observation, the state panels are repeatable 1,024-byte buffers and 772
  bytes differ across cases.  The baseline is the normal track page; the
  manipulated frame is the open `MACHINE SEL > TRACK 1` list.  The latest
  untorn-frame latches were 14,554,643 (baseline) and 9,048,263
  (manipulated), both before the 14,664,427 observation save.
- The configured on-chip SRAM range `0x80000000..0x80010000` differs by zero
  bytes at both observation and final endpoint.  A provenance-bound sweep of
  every mapped snapshot page instead finds 4,487 changed bytes on 13 of 134
  pages at observation and 4,413 bytes on 13 pages at the endpoint; see
  `mapped-page-diff.json` in the run directory.  These broader differences
  are state-lane evidence, but are not yet producer ownership.
- The perturbing profile lane has 16 baseline versus 57 manipulated UiTrace
  events.  Its address-sorted basic-block-entry profiles contain 11,749 versus
  12,351 `(address, hit-count)` tuples, with 1,416 baseline-only and 2,018
  manipulated-only tuples.  This is scoped dynamic call/view and block-entry
  evidence, not instruction coverage or a complete call graph.

The earlier `a1-real-001` artifacts are diagnostic only: their state/profile
bytes were already repeatable, but the report rejected every panel because it
compared an absolute restored timer clock with run-relative save counts.
`guirun.py` now records the live hook-time clock relative to the resumed run's
timer origin; `a1-real-002` is the accepted run.  The report and the raw
snapshot, panel and profile bytes were checked independently before this was
marked verified.

## Raising the timer rate after boot drains the UI queue **[V][O][C][D]**

`tools/guirun.py --ips-at WHEN:N` changes the timers' instructions per
emulated second at instruction count WHEN. Raising the rate from the
snapshot stretches boot (4x and 10x were still on the splash screen at
200M), so these runs raise it at 80M. `--trace-tasks` (`emu/taskprof.py`)
charges instructions to RTOS tasks at each context switch.

- `--ips-at 80M:4680000` gives progress lines identical to a run without
  the option. **[V]**
- Late chord (FUNC at 164M, SRC tap at 170M) with the rate raised at 80M
  to 1.5x, 2x, 4x or 10x: the UI queue depth stays at 0-3 up to 200M, the
  SRC press waits 0.07M-1.7M instructions instead of 5.3M, and
  MachineSelectionView is activated and still open at 200M. At 1x it
  closes at 177.6M. **[V]**
- At 2x the backlog comes back later: depth 9 at 200M, 29 at 300M and 60
  at 420M, and a chord at 380M closes the list at 400.8M. At 4x the depth
  stays at 0-1 up to 420M and the list stays open. **[V]**
- At 1x after boot the UI task (TCB `0x4094eee8`) gets 99.2% of the CPU.
  The tasks with entries `0x400f1fce` (priority 3), `0x400f1eb6`
  (priority 2) and `0x4012606a` (priority 6) get almost none, and `jobs`
  stays at 1. With the rate raised, `0x400f1fce` and then `0x400f1eb6`
  take 26-71% of the CPU between 100M and 160M, `jobs` goes to 2, and then
  `0x4012606a` takes 50-78%. **[V]**
- At 2x, 4x and 10x the screen shows `+DRIVE INITIALIZING` from about
  160M, covering the list (at 4x it is still there at 419M). At 1.5x the
  list is visible from 176M to 199M. So at 1x these runs never reach
  +Drive initialization: the UI task leaves no CPU for the job tasks.
  **[V]**
- The GUI session that kept the list open reached `jobs 2` at about 260M,
  and its screen changed to 409 lit pixels at about 470M. That may have
  been this splash. **[O]**
  > **Corrected 2026-09-28 [C][V].** `jobs` is not a queue depth. `emu/gui.py`
  > counts entries to the BgWorker loop prologue (`profile.job_pump`, on 1.16
  > `FUN_400f0958`), which runs once per worker thread, so `jobs 2` means two
  > worker threads started.
- With the rate raised at 80M to 1.5x, 4x or 10x and no key input,
  `+DRIVE INITIALIZING` is still on screen at 1000M: 131, 49 and 20
  emulated seconds after the change. It first appears at 250M (1.5x) or
  by 150M (4x and 10x). There is no exception and no fault. Between
  frames only the spinner changes. **[V]**
- The splash task (entry `0x4012606a`) waits until the count at
  `0x44e2d5cc` is nonzero, then loops with no exit. Each pass draws a
  progress step, `(0x44e2d5d0 * 0x27) / 0x44e2d5cc`, and turns a spinner
  (`FUN_40126332`). Ghidra shows one direct writer for each of the two
  counts, both before the loop. **[V]**
- The counts are also written through the helper `0x40125f6a` (reached
  through thunks `0x4003230c` and `0x40032320` from `FUN_40032c5c`, a
  mount routine): 2121 calls between 80M and 250M at 4x. So the bar does
  advance once the task gets CPU; "one direct writer" was true but
  incomplete. **[C]**
- While the splash is up, its task takes 49-51% of the CPU at 1.5x,
  72-74% at 4x and 88-90% at 10x. The job tasks (entries `0x400f1eb6`
  and `0x400f1fce`) use CPU only in the 20M window after they start,
  then almost none. At 1.5x the UI task gets half the CPU and the UI
  queue grows again (109 at 300M, 916 at 1000M); at 4x and 10x it stays
  at 0-1. **[V]**
- Why +Drive initialization stalled: with `unblock=True`, `build()`
  force-satisfies every semaphore pend that is not on its exclusion
  lists. The progress-screen task pends on semaphore `0x44e2d148`
  before its loop (at `0x401260c0`, covered by `display_wait`) and once
  per frame inside it (at `0x40126132`, not covered). The display
  module's PIT3 ISR (`0x40125f3c`; PIT3 set to PCSR `0x0936`, PMR
  `0x4323`, about 7.5 Hz) posts that semaphore. At 4x the per-frame pend
  ran 3879 times between 80M and 250M against 93 PIT3 posts, so the
  prio-6 task drew about 40 frames per real one and starved the prio-2
  job worker (entry `0x400f1eb6`), which was ready: parked right after a
  mutex unlock's `trap #0`, resume PC `0x4000178c`. **[V]**
- Fix: `display_sem` (read from `pea.l` at `0x40125f4e`, symbol
  `display_frame_post`) is always in the never-fake set. With it and the
  rate raised to 4x at 80M, the progress-screen task uses about 2% of
  the CPU, the job worker about 69%, the splash is gone by 200M, the job
  worker rests in the job pump from about 370M, an idle task takes the
  spare CPU, and the UI queue stays at depth 0-2 up to 600M. At 1.5x the
  splash is gone by 150M and the job worker is still working at 600M
  with the UI queue at 0. At 1x nothing changes: the job worker never
  starts. **[V]**
- Pends still force-satisfied in a 600M run at 4x, by call site:
  `0x400d4038` (intro, 175, all before about 120M), `0x40120cac` (in the
  CMD25 write routine, 4), `0x40120a92` (in the CMD18 read routine, 2),
  `0x40126046` (1). The UI task hits none of them, so `unblock` does not
  explain the UI backlog at 1x. **[V]**
- At 4x the firmware issues 941 CMD18 multi-block reads (`0x401208fe`)
  and 3 CMD25 multi-block writes (`0x40120ae4`) by about 129M. The
  eSDHC model completes each command at once and reads have no backing
  data (see emu/esdhc.py). **[V]**
- Whether the +Drive being empty matters later (projects, samples).
  **[O]**
- The MCF5441x reference manual gives "Up to 385 Dhrystone 2.1 MIPS @ 250
  MHz" (`docs/refs/rm.txt`, line 2475). The firmware's bus clock is 132
  MHz (see the section on the intro running at 15.00 fps), and the manual
  fixes the bus clock at half the core clock, so the core runs at 264
  MHz, above the manual's 250 MHz maximum. **[D]**
- Why the core clock is above the rated maximum. **[O]**
- The firmware has no CPU-speed calibration or counted delay loop that
  was found: its timed waits use DTIM1 or the PITs. No MCF5441x BogoMIPS
  boot log was found online. The real instruction rate is estimated at
  200-264M instructions per second, from docs/archive/HANDOVER.md lines 31-58 and
  separately from the Dhrystone figure; `INSTR_PER_SEC` is 4.68M. **[D]**

## The rate rises automatically after the intro; where wall time goes **[V][O]**

`tools/guirun.py` and `emu/gui.py` raise the timer rate to 18.72M (4x
`INSTR_PER_SEC`) at the first chunk boundary after the intro hands over.
`--post-intro-ips N` sets the rate and 0 turns it off; an explicit `--ips`
or `--ips-at` disables it. Progress lines and the GUI label show wall-clock
instructions per second and the percentage of real time.

- `--post-intro-ips 0` matches a default run from before the change, and
  `--ips-at 80M:18720000` matches the earlier `--ips-at` run, in every
  emulated field of the progress and end lines to 200M. **[V]**
- Default, headless from `snapshots/boot400M.snap` with the five parts:
  the rate changes at 52999788, UI queue depth stays 0-1 (max 2) to 500M,
  +Drive init finishes at about 360-380M, no exception. **[V]**
- After init, 76-77% of each 20M window is the init task at the parking
  loop `0x400cf3e0`; the UI task takes about 23%. **[V]**
- Wall-clock rate in that run on an Apple M3 Max: about 10M instructions
  per second while the job worker runs (about 53% of real time at 4x), and
  4.0-4.1M per second once idle (21-22%). `idle-spins` rises from 0 to
  about 3.9M per 20M window at the same point. **[V]**
- Whether the idle-spin hook is what halves the wall-clock rate. Needs an
  A/B with that hook disabled before building idle skipping. **[O]**
- An earlier baseline on the same machine ran at load average 18-25, so
  its absolute numbers are not comparable: 0-80M at about 2.5M per second
  at every rate, 80-200M at 8.4M per second at 1x and 6.0M at 4x.
  `--trace-tasks` and `--trace-ui` changed the rate by about 1%. **[V]**
- The GUI label was not checked (no Tk in the agent sandbox). **[O]**

## A panel-path track trigger joins machine invalidation to the refresh queue **[V]**

The missing operation in the successful behavioral join after machine
selection was a track-0 trigger, not a synthetic SSI receive record. In an
exact 1.16 run with
`--no-unblock` and no `--weakptr`, the accepted panel-wire replay changed
track 0's machine type from 0 to 2 and invalidated its refresh cache. A
subsequent host-replayed `TRIG 1` wire event (`2301`, release `2300`) produced
this chain through the firmware's normal panel-input path: **[V]**

```
panel machine commit 0x40036798
  -> setter 0x40051712 (source +0xa2: 0 -> 2)
  -> notification / cache invalidation
  -> panel TRIG 1
  -> FUN_40139878 record construction
  -> FUN_4013a78a queue append
  -> guest vector 191 at 0x4002dd0c
  -> FUN_4002d438(0x426532ec, 0)
  -> track-0 row 0x80003cd0 byte 0 = 2
  -> TX frame 0x80005348 + 0x94 = 0x0002
```

The appended 0x6c-byte child had observed values type 0, state 1, track 0,
word `+0x14 = 2`, and flags `0x00010781`. Its append hook reported return
address `0x4012009a`: `FUN_4011fe12` calls `FUN_40139878` at `0x40120094`, and
on the observed path `FUN_40139878` restores its frame before tail-jumping to
`FUN_4013a78a` at `0x40139aa0`, preserving the outer return address. Raw 1.16
bytes were checked at `0x40120094`, `0x40139aa0`, `0x4013a78a`, `0x4002dd0c`,
and `0x4002d438` against MAIN OS SHA-256
`57bb4dfa8df07d846adc72fdb4fb0d3cd3c5680c524bf498338460207e008e7d`.
**[V]**

The controls separate the causal pieces: **[V]**

- `TRIG 1` without a machine change appended and consumed the same track-0
  record, but the cached source already matched; there was no refresh, row
  write, or TX type change.
- Machine change followed by `PLAY` scheduled records only for the current
  pattern's tracks 3, 12, 13 and 15. Track 0 stayed invalidated and neither its
  row nor TX type changed.
- Machine change followed by `TRIG 1` made one refresh call, copied the row,
  and changed the repeatedly emitted TX word from `0x0000` to `0x0002`.

The reproducible artifact is
`out/experiments/a2-queue-trigger/report.json`; hashes are in
`out/experiments/a2-queue-trigger/checksums.txt`. The decisive run ended at
96,145,920 instructions with 81 vector-191 entries, two queue appends, one
refresh, zero fault pages, row prefix `02000200`, and TX word `0002`.

This is still a **calibration**, not final hardware qualification: its parent
snapshot acquired normal vector-170 handover from host-poked markers and the
SSI0 request rate is the exploratory 1,000 Hz value. Thus it proves the
ColdFire behavioral join under exact execution, while natural marker
production and firmware-backed cadence remain open. **[O]**

## Modal windows take every key (1.16) **[D][O]**

2026-09-28, from `snapshots/dt2-1.16-drive3/loaded.snap` (cold boot with a
+Drive image). A panel TRIG 1 reached the key dispatcher but never the note
path, because two modal windows sat on top of the view stack.

- `FUN_4011c13a(0x44f5de80, ...)` is the 1.16 key dispatcher. It walks the
  view list at `+0x14` from the top and gives the key to the first view whose
  predicate accepts it. In `loaded.snap`, top first: `ConfirmWindow` (vtable
  `0x4021c898`), HelpBubbleView, KeyboardView, PatternGridView, TempoLedView,
  SamplerLedView, SourcePageView, TransportView, MainScreenView.
  `running.snap` has the same stack without the ConfirmWindow.
- The ConfirmWindow shows "FILE SYSTEM OK" / "PRESS Y/N TO CLOSE" (strings at
  `+0x50`/`+0x54`). Closing it shows a second one, "MMC NOT IN SLC MODE",
  which `FUN_400337ba` selects when `FUN_4012da80()` (reads `0x4fe69198`,
  EXT_CSD SLC state) is not 1 **[V for the condition]**. `emu/esdhc.py`
  reports SLC off on purpose (finding 14, "The capacity bound fixed"), so the
  second window is a side effect of the emulator's eMMC model. Why "FILE
  SYSTEM OK" appears is not traced **[O]**.
- With the windows up, the UART8 receive callback `0x4011e368` gets the
  bytes and `FUN_4011c13a` runs for press and release, but
  `KeyboardView::vfunc_2` (`0x4005dc58`), `FUN_4011fe12` and the record
  builder `FUN_40139878` are never reached. On `running.snap` a TRIG goes
  `KeyboardView::vfunc_2` -> `FUN_4005d26a` -> `FUN_4005c5e8` ->
  `FUN_4011fe12` -> `FUN_40139878` -> `FUN_4013a78a`.
- Two NO presses close both windows, and TRIG 1 then reaches the frame
  (finding 04, "1.16 frame fields"). `tools/sharc_capture_run.py --press
  NO@1000000 --press NO@2000000` does this with real panel input (button
  names from `panelin.control_names`).


## Encoder rotation in the native boot harness (2026-10-07) **[D]**

Panel encoder rotation reaches the 1.16 firmware through the UART8 ring (header
`0x30|(enc-1)`, then a signed byte), as in `emu/panelin.py`. While the boot modal "FILE
SYSTEM OK / PRESS Y/N TO CLOSE" is up, rotation does nothing; after NO, rotation changes the
OLED on the FLTR, AMP, FX and MOD pages. The display lags a page change: the first turn
afterwards can show 0 changed pixels until the page animation ends. Dead-looking encoders
in the desktop app were therefore the modal, or (before the DSPI1 fix in finding 14) the
SRC-page hang. `packages/emulator/boot/examples/turn_probe.rs` boots, taps buttons, turns encoders and
reports OLED pixel changes (`CARD=img`; `b:CODE` tap, `d:ENC:DET:COUNT:GAP` drag,
`c:HOLD:TAP` chord, `w:M` wait). The web faceplate knob drag was checked in Chromium only;
WebKit was not tested **[O]**.

## DT2 1.16 FX pages and encoder map, by read-back (2026-10-11) **[D]**

Found with `native` scenario runs on the one-sample card (`coupled.py scenario --device dt2 --dt2-card`, 8 probe
scenarios, then one scenario per encoder), reading the live Sound array of track 1 (`*(0x8000470c)+0x14`) and the
Fx array (`*0x80004708`) after 10 events of +-4 detents (finding 02 for the arrays). Button and encoder codes are the
DN2's (`tools/emulator/scenario_dsl.py`: FX 5, FLTR 3, MOD 6, FUNC 17, NO 12, TRIG 1 = 25), and so are the pages: FUNC + FX
(a latched chord) opens Delay, a second chord Reverb, a third Chorus, a fourth Delay again; FUNC + MOD opens the
Master compressor page. Encoder rotation is ignored until NO has closed the boot modal (the `dt2-sample-trig`
prelude).

| Page | Encoders (1..8, then LEVEL = 9) |
|---|---|
| FX (button 5) | 1 Bit Reduction, 2 Overdrive, 3 Sample-Rate Redux, 4 SRR Routing (0/1), 5 Delay Send, 6 Reverb Send, 7 Chorus Send, 8 OVR Routing (0/1) |
| Delay | 1 Time, 2 Pingpong (0/1), 3 Stereo Width, 4 Feedback Gain, 5 Feedback HPF, 6 Feedback LPF, 8 Reverb Send, LEVEL Mix Volume |
| Reverb | 1 Pre-delay, 2 Decay Time, 3 FB Shelving Freq, 4 FB Shelving Gain, 5 Input HPF, 6 Input LPF, LEVEL Mix Volume |
| Chorus | 2 Speed, 3 High-pass, 7 Delay Send, 8 Reverb Send, LEVEL Mix Volume (1 Depth and 4 Width start at their maximum, so a +4 turn changes nothing) |
| Master (FUNC + MOD) | 1 Threshold, 2 Attack, 3 Release, 4 Makeup Gain, 5 Ratio, 6 Sidechain Src, 7 Sidechain Filter, 8 Dry/Wet Mix |
| Filter (button 3) | 5 Frequency (127 at start), 2 Env. Decay, 4 Env. Release, 8 Env. Depth |

**The UI value is not a smooth function of the event count.** The encoder accelerates by the time between events, and
the value after N events of +4 detents (1M instructions apart) is irregular and the same for every parameter of a
layout: in the DT2 FX-page layout of `tools/emulator/gen_dt2fx.py`, N = 24, 30, 52 give UI 22, 54, 127, while N = 48 gives 127
and N = 50 gives 113 (`gen_dt2fx.py OUT --ladder` measures it). The DN2 counts (22, 26, 30, 40, 44 events for UI 28 to
127) do not carry over: 44 events reach UI 90 on DT2. A scenario is therefore calibrated by read-back in its own
event layout, and the filter-closed layout (sweep 60M instructions later) needs N = 46 for 127.

## Panel input: the "encoder accumulator" counts turns since a push, it is not an undrained delta (Digitone II 1.11) **[D]**

Relevant to issue #6 (encoder deltas arrive but the value never moves). The
per-encoder counter at `0x445a0dc4` climbs with every detent and never clears
under emulation, which read like a missing drain. Reading its only clearing
code says otherwise.

- The panel MCU's UART stream is dispatched on the wire tag at `0x4011fd5c`:
  tag 3 accumulates encoder detents, tag 7 is the console block, tag 2 calls
  `0x4011f9ac`. **[D]**
- `0x4011f9ac(obj, byte)` is the **button-byte handler for one channel**. For
  each changed bit it looks the control code up in
  `0x401f39dc[channel*32 + bit]`. Only codes **41..48 -- ENCODER A..H pressed
  as buttons** -- reach the clear at `0x4011fbb0` (counter and pending bit).
  On release (`0x4011fc08`) the push is timestamped if the counter is <= 9. **[D]**
- On the instrument, turning an encoder **while it is pushed** changes the value
  in a coarser, rounder step (for example whole units instead of 0.01; the step
  is set per control). The owner reports this from the hardware. **[V on
  hardware]** So the counter plausibly tracks turns made during or since a push,
  for that coarse mode, and a push released with few turns may count as a click.
  Which code applies the coarse step has **not** been read. **[O]**
- The counter clears only on a push of its own encoder. A test that sends only
  turns, or button frames for other controls, will never clear it, and that is
  correct firmware behaviour. **[D]** Five +1 deltas give 5 with the
  pending bit set; a no-edge frame, a press and a release of channel 0 bit 5
  (not an encoder push) each leave it at 5; five more give 10.
- The encoder's own event still reaches the main task's loop with the right
  code and delta (hooking `queue_send`). So #6's stuck value lies downstream of
  that loop, in how the page applies a delta, not in panel input. **[O]**

Measured with Digitone II OS 1.11, `boot400M.snap`, `--weakptr --slc`.

## Where the stuck value was: an encoder filter swallows the first counts after idle (Digitone II 1.11) **[D]**

Follows the section above, which left #6's stuck value "downstream of that loop, in how the
page applies a delta". It is a filter between the two, `0x4011336e` (72 direct callers, among
them the arpeggiator menu's MODE and RANGE, `0x40018ed2`..), and it is not stuck: it returns
0 for the first counts after the knob has been idle, then lets one unit through.

- **Per encoder it keeps a counter that starts at 48.** A periodic tick (`0x40113314`..
  `0x40113320`) adds 1 up to 48. A count that arrives while the counter is above 24 takes
  `3 * |delta|` off it (not below 0) and returns 0 (`0x40113426`..`0x401134ac`). At 24 or
  below the delta is accumulated and scaled, and the filter returns the result; one unit is
  256 (8.8 fixed point, `lsll #8` at `0x401134a8`) for a plain value, a whole number for a
  menu value. Each unit emitted puts 48 back (`0x4011349e`). **[D]**
- **Measured**, Digitone II 1.11 booted from reset, TRIG page, knob A, `+1` counts 20 M
  instructions apart: the counter read 48, then 47, 45, 44, 42, 41, 39, 38, 36, 35, 33, 32,
  30, 29, 27 after each count, and every call returned 0. At the 28th count the call
  returned 256 and the page changed (303 pixels); after 30 counts the counter read 65. 200
  counts in packets of 10 gave seven units. The tick adds about 1 per 13 M instructions here
  (3 off per count, 1.5 net per 20 M). **[D]**
- **Menus take the same filter and a whole unit.** The ARPEGGIATOR menu's MODE (knob A) moved
  one step per 28 counts and `setMode` (`0x4004bea4`) saw 0, 1, 2 for 56 counts; its RANGE
  (knob C) needed 56 counts for one step (`setRng` `0x4004c0da` saw 0 then 1). **[D]**
- **So a test that turns a knob a few counts and sees nothing is inside the lead-in, not
  looking at a dead path.** The seqplay tool of #67 to #70 (not merged) used 28 counts a unit,
  packets of 10 with 15 M between them. Pressing the knob (codes 41..48) is not needed, as on the unit.
- **Not known:** the filter's role on hardware. The unit's encoders are smooth, with no
  detents (owner), so a "count" is a position step and 28 of them for one unit may be the real
  mapping, with the lead-in a jitter rejection. The tick rate there is not measured. #6's own
  case (45 counts, 0.00 after) was not re-run; this explains it only if its page uses this
  filter. **[O]**

### The panel's button names, read from the image **[D]**

`panel_button_names` (resolved by `emu.symbols`) for 1.11, code to name: 1 TRIG, 2 SRC,
3 FLTR, 4 AMP, 5 FX, 6 MOD, 7 PRESET, 8 SETTINGS, 9 TEMPO, 10 YES, 11 UP, 12 NO, 13 LEFT,
14 DOWN, 15 RIGHT, 16 TRK, 17 FUNC, 18 KEYBOARD, 19 RECORD, 20 PLAY, 21 STOP, 22 PAGE,
23 PTN, 24 SONG, 25..40 TRIG 1..16, 41..48 ENCODER A..H (pushed), 49 LEVEL, 50 VOICE,
51 ARP, 52 PLUS, 53 STACK, 54 MINUS. The device profile maps 52..54 to channel 6 bits 5..7
(not `code * ...`, which holds for 1..48 only). PLUS and MINUS transpose a held trig; they
do not set a note. **[D]**

## The boot intro on DT2 1.17: scene lists, draw contract, and which way up the framebuffer is (2026-10-18) **[D][C]**

Read from the 1.17 MAIN OS bytes and measured in bounded emulator boots (`panel_drive`, captures of the OLED block
sender); the module is `modules/ui/boot-splash`, its tests `tests/patchkit/test_boot_splash.py`.

- `main()` calls `intro_start` (0x400d1448): an intro task (entry 0x400d1a12, prio 7) and PIT3 at 30 Hz. The task
  picks one of five scene lists by `rand()`; the lists are one 80-byte block at 0x402a1b64, five `{draw_fn, nframes},
  {0, 0}` pairs (frame counts 175, 180, 180, 180, 170). step() (0x400d18f0) builds a 128x64 Bitmap over the draw
  buffer, calls `draw_fn(frame, nframes, &bitmap)` (0x400d1992, cdecl: bmp at 12(sp); the stock draw functions keep
  d2/d3 and clobber d0/d1/a0/a1), then sends a full flush (0x40133b6c) on a scene's first frame and a diff flush
  (0x40133bea) otherwise. Both send `(col, page, 8 bytes)` records through 0x40133b1c. The task posts the done
  semaphore 0x43149550 when the list ends; only the app task waits for it (0x400337dc), before the loading screen
  and project load. **[D]**
- Bitmap (ctor 0x401124b6): +4 width, +8 height, +0xc words per column `(h + 31) >> 5`, +0x10 storage; setPixel
  (0x401124fc) sets bit `0x80000000 >> (r & 31)` of word `x * stride + (r >> 5)`. **[D]**
- **Bitmap row 0 is the bottom of the glass [C].** The stock full-screen intro image (the Bitmap the scene 0x400d11f0
  blits, storage in the MAIN image) reads upright ("MORE IS MORE") only when Bitmap row r is drawn at screen row
  63 - r, and the UI framebuffer reads upright under the emulator's panel model (frame byte `(7 - y/8) + 8x`, bit
  `y % 8`, which is the framebuffer's memory as is) for the same reason. This corrects the research note ("MSB = top
  row, y = 0 at the top"). The emulator's intro-frame tracker (`decode_intro_bitmap`, runtime.rs) maps Bitmap row 0
  to the top, so the intro frames it publishes are upside down relative to its own panel frames **[O: not fixed]**.
- Timing in the emulator runtime (`panel_drive`): stock (it picks the 175-frame list) intro_wait returns at 823.0 M
  instructions, main UI at 831 M; with all lists at 45 frames (ui/boot-splash) at 250.9 M and 259 M, and the frame
  after boot is identical to stock. main()'s own init does not hold the app task back once the intro is short. **[D]**
- ui/boot-splash uses this: data writes to the five lists (draw function = its `splash_draw`, 45 frames), no code
  hooks. **[D]**
