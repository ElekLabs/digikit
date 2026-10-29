# UI surfaces for mods on DT2 1.17 and DN2 1.12

Marks: **[D]** read or measured once, **[O]** open, **[V hw]** seen on a unit by its owner. Nothing here is **[V]**: a
second agent has not checked it against the image bytes. What a mod that adds a mode of its own (knobs, keys, LEDs,
screens, a PERSONALIZE row, per-frame values) needed beyond [finding 71](71-ui-components-1-17.md), measured with the
scripted runner (`packages/emulator/coupled/examples/zero_patched.rs`) on patched builds.

## 1. Calling convention **[D]**

The firmware returns pointers in d0. GCC's m68k ABI reads a pointer result from a0, so a C prototype that returns a
pointer reads a stale a0. Calling the `UIStates` getter that way returned the view controller, and the UI froze.
Declare such routines as returning a 32-bit integer.

## 2. Input **[D]**

| Input | What the key and encoder handlers get |
| --- | --- |
| Encoders A-H | encoder ids 1-8 |
| LEVEL/DATA | encoder id 9; pushed, key code 49 |
| Pushes A-H | key codes 41-48 |

A key event's flags (event + 16): bit 0 is "down". A press is `0x01` (`0x03` with FUNC held), a repeat `0x09`
(`0x0b`), a release has bit 0 clear: `0x10` for most keys, but `0x00` for FUNC and PAGE. After FUNC + NO (the pattern
reload) FUNC's release is not passed on at all. The key-down query ([finding 58](58-panel-input-path.md)) knows.

While FUNC is held the firmware puts `QuickMuteMenuView` on top of the view stack (vtable DT2 `0x401f9208`, DN2
`0x401e1eb4`), and while TRK is held `TrackSelectionView` (DT2 `0x401f9ec8`, DN2 `0x401e2b68`). On the main screen
and the parameter pages the top view is `HelpBubbleView` (DT2 `0x40216f14`, DN2 `0x402008e8`). A menu, dialog or
popup pushes another view.

### The encoder filter **[D]**

Every stock knob turns through one filter: after the knob rests it swallows the first counts, then moves more per
count the faster it turns. [Finding 03](03-ui-and-panel.md) has it on DN2 1.11 at `0x4011336e`.

| | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| `filter(block, knob, count, cfg, pushed)` | `0x40111cda` | `0x4011364e` |
| `list_filter(block, event, cfg)`: count x8 when pushed, then `filter` | `0x40111e42` | `0x401137b6` |
| The block's constructor (252 bytes; starts the timer that refills it) | `0x40111b68` | `0x401134dc` |
| cfg of the 0-127 parameters (LEV, VOL, sends): raw units, 256 a step, count x2 pushed | `0x44d2e654` | `0x44508e40` |
| cfg of the modulation depths: 1/256 steps, count x12 pushed | `0x44d2e670` | `0x44508e5c` |
| cfg of MODULATION SETUP's destination knobs: steps | `0x44d2e600` | `0x44508dec` |

An encoder event has the knob at + 12, the count at + 16 and a "pushed" byte at + 20. A parameter's cfg is the word
at + 16 of its run-time descriptor (`0x4293c040 + 0x44 id` on DT2, `0x400c892e(id)`). Measured: a mod's knob turned
through `filter` with the parameters' cfg moved by exactly the steps of the stock LEV knob (DT2) and Delay Send (DN2)
over the same 38 turns.

## 3. Per-frame parameter values **[D]**

The frame interrupt (vector 191) builds a fresh block of per-track values every frame. The glide writes it from the
mirror, then the stock modulators add to it.

| | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| Glide `(mirror)` -> block, every frame | `0x400d9406` (mirror `0x80003340`, block `0x80005b50`) | `0x400db28e` (mirror `0x80003af0`, block `0x800068e4`) |
| The per-track modulators `(block, ...)`, called with a 6-byte `jsr` | `0x400d9508`, at `0x4002e90c` | `0x400db390`, at `0x40027196` |
| Track t, value index j during the frame | `0x80005b72 + 142 t + 2 j` | `0x80006906 + 202 t + 2 j` |

A hook at the call site adds to the block before the stock velocity, second source and LFOs: on top, as a modulator,
without touching the kit or the mirror. Measured: the frame values moved by the expected offsets, and the kit and
mirror did not.

## 4. Key LEDs **[D]**

The LED frame (DT2 `0x4003117c`, DN2 `0x4002c854`) runs when the view controller's byte + `0x21` is set. It clears
every LED with `jsr (pc)` to DT2 `0x4003098e` / DN2 `0x4002c3c6`, at DT2 `0x400311a2` / DN2 `0x4002c87a`; those are
6 bytes with the `move.l d2,-(sp)` after the call. Then the views set theirs, and the finish turns off what nobody
set. The setter `(mgr, logical, layer, value)` (DT2 `0x400308e0`, DN2 `0x4002c318`) refuses an LED whose layer is
already set, so the first setter wins: a hook after the clear sets LEDs over the views'. Layer 2 lights with the
value, layer 1 turns off.

The value's high nibble is a palette index **[V hw]** (a Digitakt II, trig key n lit with `(n - 1) << 4 | 1`):

| `0x01` | `0x11` | `0x21` | `0x31` | `0x41` | `0x51` | `0x61` | `0x71` | `0x81` |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| off | red | green | deep blue | yellow | light blue | purple | white | orange |

This corrects the bits-per-colour reading in [finding 19](19-sequencer-and-leds.md) for these values **[C]**.

## 5. Text, fonts, header **[D]**

- Text: `draw_fmt(bitmap, font, x, y, flags, fmt, ...)`, DT2 `0x40113dc8`, DN2 `0x4011573c`. `y` is the text's
  bottom row counted from the screen's bottom; flags bit 1 centres on x.
- Fonts: main DT2 `0x44d2e70c`, DN2 `0x44508ef8`; the narrow one (3-pixel letters) `0x18` below on both, DT2
  `0x44d2e6f4` and DN2 `0x44508ee0`. The stock lists and page boxes use the narrow one.
- The header's message (finding 71's toast): `UIStates` getter DT2 `0x401998c0` (`*0x44f370c0`), DN2 `0x4018b2a4`
  (`*0x446488c8`); the message string at + 72.

## 6. PERSONALIZE value rows **[D]**

The PERSONALIZE builder (DT2 `0x4009daec`) makes LED INTENSITY as a MenuItem with label, action, draw and adjust:
- Its draw invoker (DT2 `0x4009c722`, DN2 `0x400966f6`) gets `(functor, item, bitmap, x, y)`. It draws the value
  string from a table with `0x40113fd4` (DN2 `0x40115948`) as
  `(bitmap, font 0x402a9be0 (DN2 0x40294710), x + 19, y + 2, 2, 0, 0, text)`: right-aligned.
- Its adjust (DT2 `0x4009e2ae`, DN2 `0x400987ae`) gets `(functor, item, delta)`, clamps the value and redraws the
  menu with `View::invalidate(menu + 0x38)` (DT2 `0x4011b126`, DN2 `0x4011ca9a`).

Measured: a row of the same shape draws as LED INTENSITY does. LEFT and RIGHT call adjust and YES calls the action;
the LEVEL encoder does not reach the rows in this menu.

## 7. The naming screen **[D]**

A MIDI machine opens `NameView` to name a parameter (FUNC + hold a knob) with its second constructor (DT2
`0x40117c70`, DN2 `0x401195e4`): `(view, &title, &name, 0, len, 0, &done)`. `done` is a
`std::function<void(std::string)>`; its invoker gets the string as a pointer to the character pointer. The view is
made as `std::make_shared` makes it: operator new DT2 `0x4011ebd0` / DN2 `0x40120544`, 0xb0 bytes, the control
block's vtable DT2 `0x401ef190` / DN2 `0x401e0fd8`, then the view. It is pushed with
`0x4011c08a(Brain + 0x40, &shared_ptr, 0)` (DN2 `0x4011d9fe`). A std::string is one pointer to its characters: built
with DT2 `0x401e7e7a` / DN2 `0x401ceab8`, freed with `0x401e63ca` / `0x401cd008`. Measured: a mod's own title and
name round-trip.

## Open **[O]**

- How the stock MODULATION SETUP marks the selected column.
- The DN2's MODULATION SETUP was not dumped.
