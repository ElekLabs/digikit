# Key event path, module views and the naming screen (DT2 1.17, DN2 1.12)

Marks: **[V]** verified against image bytes by a second agent on that device, **[D]** read once, **[O]** open or not
checked, **[C]** corrects an earlier claim. Method: static only, no emulator run. DT2 is OS 1.17, DN2 is OS 1.12. Earlier
notes: [finding 71](71-ui-components-1-17.md), [finding 122](122-ui-surfaces-for-mods-dt2-1-17.md). Where a first report and
the second check disagree, the second check wins; where the second check did not cover a fact, the mark is **[D]**.

## 1. Key event path

One key record travels through these functions, in one task (the UI task):

| Step | DT2 | DN2| Mark |
| --- | --- | --- | --- |
| UI task pops a type-0 record, builds a `KeyEvent`, calls the runner | caller 0x40033d94 | caller 0x4002f20c (inside the switch case `caseD_c`, 0x4002f450) | **[V]** |
| Runner `void f(Brain *, KeyEvent *)`, 86 bytes | 0x40030bd0 | 0x4002c5c8 | **[V]** (hash 182f6a3d75055f84e825188b2c4e182d on both) |
| Idle reset called by the runner | 0x40030b50 | 0x4002c548 | **[D]** (hash 67ab033a9fdd65f48f02a4abf6c67a5e) |
| Walker `bool f(ViewController *, KeyEvent *)`, 318 bytes | 0x4011c79c | 0x4011e110 | **[V]** (hash e741aa3500a20d22aa97ee402dcf9e43 on both) |
| Offer loop (view list walk) | 0x4011c41a | not recorded | **[V]** on DT2; DN2 address **[O]** |
| Key-down query, `u32 (u32 key)` | 0x4011e150 | 0x4011fac4 | **[V]** |
| Panel poll function (emits 1 on press, 0 on release) | 0x4011e318 | 0x4011fc8c | **[V]** on DT2, **[D]** on DN2 |

Runner (both devices): arguments are `Brain *` at 4(SP) and `KeyEvent *` at 8(SP), cdecl. It (1) calls the idle reset, which
clears Brain+0xe4 and sends a 2-byte message on the peripheral queue (the name "idle reset" is a guess **[O]**), (2) loops over
the vector of 16-byte `std::function` objects at Brain+0x94..+0x98 and calls invoker(+0xc) with `(function, event)` when
manager(+8) is not 0, ignoring the result, (3) tail-jumps (`jmp`) to the walker with `Brain+0x40` (the ViewController). Who adds
listeners at Brain+0x94 is **[O]**. The single call site and the runner are **[V]**.

Walker: when `event+0x10` bit 0 is clear (a release or repeat; the test function at 0x401149f8 returns `flags & 1 ^ 1`), it
looks up the key code (`event+0xc`) in the multimap at VC+0x24 (header +0x28, root +0x2c), runs that entry's vector of
`std::function`, and erases the entry **[V]**. Then it calls the offer loop. The offer loop walks the list at VC+0x14 from the
last node to the first (top view first) and calls vtable slot 2 of the view's primary vptr with `(view, event)`; the first
nonzero result stops the walk **[V]**. On a hit the walker sets the redraw byte VC+0x22; the return value (D0 byte) is the hit
flag and the caller ignores it **[D]**. Press is flags bit 0 set **[V]**. Chords have no event of their own: the decoder sets
state values 1 and 5 in `event+8` and views test `key_down()` for modifiers **[D]**.

Every view sees keys only through the walker (menus, `ConfirmWindow`, `TrackSelectionView` are entries of the same list) **[D]**.
The decoder state behind `key_down()` is independent of this path (bitmap RAM 0x44dc8644 on DT2, set and cleared by the poll
function), so a swallowed event does not change `key_down(TRK)** [V]` on DT2; the DN2 bitmap address was not dumped, the code
has the same shape **[D]**.

### Hook site

The hook site is the walker entry. Displaced bytes (8 bytes, two whole instructions, no PC-relative form, the same on both
devices **[V]**): `4fef ffa0 48d7 7cfc` = `lea (-0x60,SP),SP ; movem.l {D2-D7 A2-A6},(SP)`; SHA-256 of these 8 bytes is
3d25d06bf11f3a7bccef894d0016b52adbb4ae468a0f475dd497d7b5966e2694.

- Pass: run the displaced instructions and jump to site+8 (the tail). The stock path is unchanged.
- Consume: `moveq #1,%d0 ; rts`. The stack is untouched at the entry, so no displaced instruction has to be undone. D0 byte = 1
  is the stock "hit" value **[D]**.
- Effect of a consume: the release-callback map, the offer loop and every view are skipped. The runner part before the walker
  still ran: idle reset and the Brain+0x94 listeners see the event. No idle-counter fix is needed.
- Argument access in the hook: `event` is 8(SP), `code = *(event+0xc)`, `flags = *(event+0x10)`. Held state without own state:
  `cf.input.fn.key_down(16)` for TRK.
- No module code in the hook: the hook runs inside the walker, so the `shortcuts` service only decides (Pass or Swallow) and queues the module's `on` and `off`; its `Service::tick` (the UI pass, early phase) makes the calls in order. Nothing of a module and no view runs inside the walker. A queued call of a module that unloads is dropped.
- Release hygiene: swallow the release of a swallowed press and pass the release of a passed press. Without that, the release
  map or a view's key-up state gets a release without a press, or never gets its own. TRK always passes (never swallow the
  modifier).
- Context: the UI task, thread context. The walker holds a mutex (on VC+4) while it offers events, but the hook runs before
  the mutex is taken, so no stock lock is held **[D]**. The hook must not block and must not call back into the view system
  from inside the walker. UI-task stack headroom for a module C call is **[O]**. UI-task stack headroom for a module C call is **[O]**.
- Liveness at the tail (`tools.coldfire.liveness`, strict, DT2 1.17, DN2 1.12 and DN2 1.11, the same result): only the flags (N, Z, V, C) are dead; d0, d1, a0 and a1 are live (the stock code calls a function that may read them); D2 and A2 are saved by the displaced `movem`. The hook saves d0, d1, a0 and a1 and declares `clobbers = ["ccr"]` **[D]**.

### Why not the runner

The runner entry (DT2 0x40030bd0, DN2 0x4002c5c8, bytes `4fef fff0 48d7 0c0c`, SHA-256
9b78b6778f56449ca9429e734d2140df96316ae0e9eea0b9d6eea089f82ddbd0) sees every key once and has a void result, but a consume there
skips the idle reset and the Brain+0x94 listeners, so the hook would have to call the idle reset itself and would hide the event
from unknown listeners. The walker entry skips only what a consume should skip. The runner stays the fallback.

### The dead dispatcher

DT2 0x4011c4e8 (and 0x4011c5ae) and DN2 0x4011de5c have no code reference (refscan, 97 percent coverage, PC-relative included)
and no 32-bit data word for the address on either device **[V]**. They offer the event to the sub-object at view+0x10, not to
the primary vptr, so they are not the path stock views use. Caveat: a direct `vtable[2]` call elsewhere was not scanned
exhaustively. Sibling users of the offer loop: 0x4011c674 (`EncoderEvent`, type-1 records, runner 0x40030b64 / DN2 0x4002c55c
**[D]**) and 0x4011c71a (a MIDI function, caller 0x400318c6 **[D]**). Encoder-push keys (codes 41 to 48) are key records and use
the key path **[D]**.

### Key release

NameView and the menu views (`VerticalMenuView`) act on YES and NO on the release of the key, not on the press; the arrow keys act on the press **[V]** (second-agent check on both devices). Flag helpers, byte-identical on both devices: release = `(flags >> 4) & 1` at DT2 0x401149ac / DN2 0x40116320; press = `flags & 1` at 0x401149dc / 0x40116350; bit 1 (the FUNC-modified key) at 0x40114a18 / 0x4011638c; the key code getter at 0x40114984 / 0x401162f8 **[V]**.

- `NameView::vfunc_2` (0x40118260 / 0x40119bd4): YES (10) and NO (12) act only on a release with bit 1 clear (bit 1 set takes the FUNC branch); the arrows 0xb, 0xd, 0xe, 0xf and FUNC (0x11) act on a press **[V]**.
- `VerticalMenuView` key function is `vfunc_34` (DT2 0x4011a660 / DN2 0x4011bfd4; `vfunc_35`, 0x4011a990 on DT2, is the touch handler): YES and NO on a release, the arrows on a press **[V]**.

A view pushed from a YES press therefore sees the release of that same YES as its own key: `core/ui` pushes the naming screen after the release (a deferral), or the screen would answer its own opening press. `core/ui` and the shortcuts hook classify a release as `flags` bit 0 clear, which also covers the releases of FUNC and PAGE whose flags are 0x00 (finding 122) **[D]**.

### The view stack in the device files

The device files carry the view stack offsets (`[ui]` of devices/digitakt-ii.toml and devices/digitone-ii.toml, read by `viewStackFacts` and `readViewStack` of test-support): the Brain pointer is a global (DT2 0x44f37098, DN2 0x44648900), the view manager is at Brain + 0x40 and its list sentinel at manager + 0x14; a `std::list` node has next at +0 and prev at +4 and the View pointer at +8. The top view is the node before the sentinel, the bottom view the node after it; the first word of a View is its vtable pointer, the opaque byte is at View + 0x16 **[D]**. The DN2 offsets are the DT2 ones, because the walker and the runner hash alike on both and take the same Brain offsets **[D]**.

## 2. Key codes

Table `panel_button_names`: index = key code. DT2 index 0 at 0x4020afb8, DN2 at 0x401f3c34 **[V]**. Bit-index map for `key_down`:
DT2 0x4020b084, DN2 0x401f3d10 (-1 = no bit) **[V]** (DT2 bitmap read; DN2 same code shape).

| Code | Key | Code | Key | Code | Key |
| --- | --- | --- | --- | --- | --- |
| 1 | TRIG | 10 | YES | 19 | RECORD |
| 2 | SRC | 11 | UP | 20 | PLAY |
| 3 | FLTR | 12 | NO | 21 | STOP |
| 4 | AMP | 13 | LEFT | 22 | PAGE |
| 5 | FX | 14 | DOWN | 23 | PTN |
| 6 | MOD | 15 | RIGHT | 24 | SONG |
| 7 | PRESET | 16 | TRK | 25..40 | TRIG 1..16 |
| 8 | SETTINGS | 17 | FUNC | 41..48 | ENCODER A..H (push) |
| 9 | TEMPO | 18 | KEYBOARD | 49 | LEVEL |

Codes 1 to 49 are the same names on DT2 and DN2 **[V]** for 10 to 17 and 25 to 40 (checked by the NameView key function too: it
switches on 10, 0xb, 0xc, 0xd, 0xe, 0xf and 0x11); the other rows are **[D]**. Extra: DT2 50 SAMPLING **[V]**; DN2 50 VOICE,
51 ARP, 52 PLUS **[V]**, 53 STACK, 54 MINUS **[D]**. This closes finding 71's open question for codes 10 to 15.

KeyEvent fields: +0xc code (getter 0x40114984 on DT2), +0x10 flags, bit 0 set on a press **[V]**. Values 0x01 press, 0x03 press
with FUNC, 0x09 repeat, 0x10 release, 0x00 release of FUNC and PAGE come from finding 122 (session) **[O]**; the walker tests
only bit 0.

## 3. View

| Item | DT2 | DN2 | Mark |
| --- | --- | --- | --- |
| `View::View(this, const char *name)` | 0x4011b06e | 0x4011c9e2 | **[V]** (hash 25fcb970d10a1353214d124c7922d34c) |
| vptr literal at ctor+0x38 | 0x4021dfd8 (at 0x4011b0a6) | 0x402079a4 (at 0x4011ca1a) | **[V]** |
| vtable header (offset_to_top, typeinfo) | 0x4021dfd0 | 0x4020799c | **[D]** (vptr = header + 8) |
| slots | 17 (#0..16) | 17 | **[V]** |
| size | 0x4c | 0x4c | **[V]** (ctor clears up to +0x48; NameView fields begin at +0x4c) |
| `View::invalidate` | 0x4011b126 | 0x4011ca9a | **[V]** (34 bytes, hash 06c561b63ac3ef5d6fd66758337bb31f) |
| relayout | 0x4011bfa6 | 0x4011d91a | **[V]** (hash 01006ed9c9ff56a28ffb8ab0c036ec10) |
| render `(VC, Bitmap *)` | 0x4011bc96 | 0x4011d60a | **[V]** (hash 676367a8b2955a60ad65f160ff65ba75; caller 0x40032b3a on DT2) |
| push `(VC, shared_ptr<View> *, replace)` | 0x4011c08a | 0x4011d9fe | **[V]** |
| push of a raw pointer | 0x4011c2a6 | 0x4011dc1a | **[V]** |
| sweep of closed views | 0x4011c36c | 0x4011dce0 | **[D]** |

Slots (17, none is an LED slot; LED handling belongs to derived classes):

| Slot | DT2 | DN2 | Role | Mark |
| --- | --- | --- | --- | --- |
| 0 | 0x4011b2c4 | 0x4011cc38 | complete dtor (restores View's secondary vptrs, no free) | **[D]** |
| 1 | 0x4011b37c | 0x4011ccf0 | deleting dtor (dtor, then fw free 0x4011eb5a) | **[D]** |
| 2 | 0x4011afe0 | 0x4011c954 | key `(view, KeyEvent *)`, nonzero = consumed | **[V]** |
| 3 | 0x4011afa4 | 0x4011c918 | name: returns the string at +0x28 (not copied, keep it static) | **[D]** |
| 4 | 0x4011afae | 0x4011c922 | draw `(view, Bitmap *)`, default `rts` | **[V]** |
| 5 | 0x4011b3b2 | 0x4011cd26 | attach: +0x30 = 0, +0x2c = manager, invalidate, run attach callbacks | **[D]** |
| 6 | 0x4011afb0 | 0x4011c924 | on_push, default jumps to slot 5 | **[D]** |
| 7 | 0x4011b422 | 0x4011cd96 | detach: runs detach callbacks (+0x40), +0x2c = 0; called by the sweep | **[V]** (the call by the sweep) / **[D]** (the rest) |
| 8 | 0x4011afbc | 0x4011c930 | set byte +0x16 (opaque) | **[V]** |
| 9 | 0x4011afc8 | 0x4011c93c | get byte +0x16 | **[V]** |
| 10 | 0x4011b1ca | 0x4011cb3e | close: +0x30 = 1 and manager+0x22 = 1 (the sweep then removes the view) | **[V]** |
| 11 | 0x4011afd2 | 0x4011c946 | "tick", default `rts` | **[O]** (callers not checked; a first report said it is called per pass, not confirmed) |
| 12, 13, 14 | 0x4011afd4 / d8 / dc | not recorded | `clr.b d0 ; rts` (FUNC + REC, PLAY, STOP) | **[D]** DT2, **[O]** DN2 |
| 15 | 0x4011b550 | 0x4011cec4 | add attach callback (16-byte `std::function`) | **[D]** |
| 16 | 0x4011b55c | 0x4011ced0 | add detach callback | **[D]** |

Close detail: slot 10 only flags the view; it does nothing if the int vector at +0x1c..+0x20 holds code 7 **[D]**.

Fields: +0x14 dirty byte, +0x16 opaque byte, +0x17 blocked byte (set by relayout on views below the topmost opaque view; then
`invalidate` does nothing: `invalidate` sets +0x14 only when +0x17 is clear and then tail-jumps to the manager redraw flag
VC+0x20 through the pointer at +0x2c) **[V]** for +0x14, +0x16 and the +0x17 test, +0x2c manager **[V]**; +0x28 name, +0x30
closing byte, +0x34..+0x3c attach vector, +0x40..+0x48 detach vector, +0x1c..+0x24 int vector **[D]**. The ctor does not clear
+0x14, +0x16, +0x17 or +0x30: allocate zeroed **[D]**. Secondary vptrs at +4 (encoder), +8 (MIDI), +0xc (analog), +0x10 (note) **[D]**.

Opaque flag: byte +0x16 is opaque as a value. Relayout calls slot 9 for each view and stores the last (topmost) view that returns
nonzero at VC+0x1c **[V]**. Views below it are not drawn **[D]**. A full-screen view sets 1 (the NameView ctor does), an overlay
sets 0 (the OverlayWindow ctor tail-calls slot 8 with 0) **[D]**. Set it before push, because push runs relayout **[D]**.

Render: runs only when VC+0x20 is set. It clears the bitmap, calls slot 4 for each list node from VC+0x1c (topmost opaque view)
to the end (top), clears each view's +0x14, then clears VC+0x20 **[V]**. List order is first = bottom, last = top **[V]**. A
view must call `invalidate` whenever its picture changes **[V]**. The bitmap is 128 x 64, 1 bit per pixel **[D]**.

### Lifetime **[C]**

Push copies a `shared_ptr<View>` (object, control block) into a 0x10-byte list node (view at +8, shared_ptr at +0xc), so the list
owns a reference **[V]**. When the last reference goes, the control block's dispose slot runs. This depends on the block type:

| Block | vptr | dispose | Result | Mark |
| --- | --- | --- | --- | --- |
| `_Sp_counted_ptr<View*>` (raw pointer variant, push 0x4011c2a6 / 0x4011dc1a) | DT2 0x401f1130 | 0x4019e096 calls View vtable slot 1 (deleting dtor) | object freed by its own deleting dtor | **[V]** |
| `_Sp_counted_ptr_inplace<T>` (make_shared) | per type | DT2 0x4019c088 / DN2 0x401a043c (the same function body is shared by 241 functions) calls vptr slot 0 (complete dtor, no free) on `*(block+0xc)`; the block is freed by destroy | object is part of the block; slot 1 is not called | **[V]** |

An earlier claim, "dispose calls the deleting dtor (slot 1)", is true for the raw-pointer block only. A module view built by
`make_shared` layout (control block 0x10 bytes, object at +0x10) must keep slot 0 of its vtable valid for the in-block case;
a module view in a raw-pointer block must keep slot 1.

## 4. NameView

| Item | DT2 | DN2 | Mark |
| --- | --- | --- | --- |
| 8-argument ctor | 0x40117ac2 | 0x40119436 | **[V]** (hash c4d23607647ee2f926e547f94aa0401a) |
| 7-argument wrapper (builds the number format from "%03d:") | 0x40117c70 | 0x401195e4 | **[V]** (hash 0e537b985689c8d97a905c64f74eb485) |
| key function (slot 2) | 0x40118260 | 0x40119bd4 | **[V]** (hash 800fe244c52e3fc3570cd3e69b431aa7) |
| draw (slot 4) | 0x40116f7c | 0x401188f0 | **[D]** |
| slot 17 | 0x401178ca | 0x4011923e | **[D]** |
| encoder handler (+4 vtable slot 2; it subtracts 4 to get the View) | 0x40117934 | 0x401192a8 | **[D]** |
| vptr stored by the ctor | 0x4021d5ac | 0x40206f78 | **[D]** |
| control-block vptr (`_Sp_counted_ptr_inplace<NameView>`) | 0x401ef190 (literal in 0x4007f086 area) | 0x401e0fd8 | **[V]** (seen in the ctors 0x4007f086 / 0x400795ba) |
| object size, make_shared block | 0xa0, 0xb0 | 0xa0, 0xb0 | **[V]** |

Arguments of the 8-argument ctor (cdecl, pointers to one-word COW `std::string` objects): `(this, prompt*, initial*, number,
maxlen, show_number, number_fmt*, done*)`. The ctor copies prompt to +0x4c, sets the initial text through 0x40117550 (DT2),
number to +0x50, maxlen to +0x6c, show_number to +0x54, number_fmt to +0x58 and `done` to +0x7c **[V]**. The 7-argument form
omits `number_fmt` **[V]**. The ctor sets opaque 1 and registers a detach lambda **[D]**.

Field map (from the object start):

| Offset | Field | Mark |
| --- | --- | --- |
| +0x4c | prompt string | **[V]** |
| +0x50 | number (int, passed to `done` unchanged) | **[V]** |
| +0x54 | show_number byte | **[V]** |
| +0x58 | number_fmt string | **[V]** |
| +0x5c | formatted prefix string | **[D]** |
| +0x60 | edit buffer (fixed length maxlen, padded with spaces) | **[V]** (trimmed by YES at +0x60) |
| +0x64, +0x68 | cursor, scroll offset | **[D]** |
| +0x6c | maxlen | **[V]** |
| +0x70, +0x74, +0x78 | 3, 0x7a, 3 (box and origin) | **[D]** |
| +0x7c..+0x8b | `done`: storage word 0 (+0x7c), word 1 (+0x80), manager (+0x84), invoker (+0x88) | **[V]** |
| +0x8c, +0x90 | pointer and control block of the FUNC popup (`HighScoreView`, 0x160 bytes) | **[D]** |
| +0x94 | allow_empty byte (0 after the ctor) | **[V]** (read by YES) |
| +0x98, +0x9c | "YES:SAVE" string, 0 | **[D]** |

`done` is a `std::function<void(std::string, int)>`. The call is `invoker(&fn, &string, number)` with three stack words: the
address of the function object, the address of a one-word `std::string` (a temporary that NameView owns and frees), and `number`
raw (not +1) **[V]**. The manager protocol (op 0 type, 1 get pointer, 2 clone, 3 destroy) is the one `core/ui` already
implements; NameView clones `done` with 0x4019d802 on DT2 **[D]**. If manager is 0 the call throws `bad_function_call` **[V]**.

Key behaviour (codes 10 YES, 11 UP, 12 NO, 13 LEFT, 14 DOWN, 15 RIGHT, 0x11 FUNC **[V]**):

| Input | Behaviour | Mark |
| --- | --- | --- |
| YES press, FUNC not held | trim trailing blanks (+0x60). If the text is not empty, or byte +0x94 is not 0: call `done` (if the manager is not 0), then close (slot 10). Otherwise stay open and return 1. `done` runs before the close | **[V]** |
| YES press, FUNC held | insert a space at the cursor (0x40117598 on DT2) | **[D]** |
| NO press, FUNC not held | close (slot 10); `done` is not called | **[V]** |
| NO press, FUNC held | delete the character before the cursor (0x4011765a on DT2) | **[D]** |
| UP / DOWN | change the character under the cursor through the character set (wraps) | **[D]** |
| LEFT / RIGHT | move the cursor, clamped to 0..min(len, maxlen-1) | **[V]** for the clamp by maxlen-1, **[D]** for the rest |
| SETTINGS (8) release | random name (0x401179d8 on DT2) | **[D]** |
| FUNC (0x11) press | opens a `HighScoreView` popup; purpose **[O]** | **[D]** |
| other keys | return 1 (modal) | **[D]** |

Maximum length: `maxlen` is the buffer width and the cursor clamp; the ctor does not clamp it. Stock callers use 0x10 (song name),
0x3c and 0x80 **[D]**. An initial text longer than maxlen is truncated by the resize **[D]**.

Character set **[D]**: a global `std::string` at RAM 0x44d2e714 on DT2, filled at boot by 0x401d1d66 from the string at 0x4025dea9:
`A` to `Z`, then Latin-1 bytes 0xC5 0xC4 0xD6 0xDC 0xDF 0xC6 0xD8 0xC7 0xD1, then `0123456789~!@#$%^&()_+-= ` with the space last.
No lower case. A second set (0x4025dee6, `A-Z+-=&/#@?%$0-9~ `) is used by another editor. The DN2 address of the global is **[O]**.

Stock callers: song name (DT2 0x40084e36, DN2 0x4007f36a; string "ENTER SONG NAME", maxlen 0x10, show_number 1, format "S%02d:")
**[D]**. Stock push: `View::push(menu_view, &sp, 0)` (DT2 0x4011b22a, DN2 0x4011cb9e) or `ViewController::push(manager, &sp, 0)` **[D]**.

## 5. Fonts and text

| Font | DT2 | DN2 | Header (words) | Height | Mark |
| --- | --- | --- | --- | --- | --- |
| Stock UI font | 0x402a9be0 | 0x40294710 | 0x3, 0x00010006, 3 pointers | 6 (glyph widths 1 to 5) | **[V]** (bytes identical on both devices) |
| Large font | 0x402a764c | 0x4029217c | 0x7, 0x0002000f, 3 pointers | 15 (widths up to 7, 2-byte rows, 30 bytes per glyph) | **[V]** (bytes identical on both devices) |

The meaning of header word 0 is **[O]**. A first report called it the advance; the check shows the stock font's glyph widths reach
5 while word 0 is 3, so do not call it a width. Other fields (from the first report, **[D]**): +4 u16 y bearing, +6 u16 height,
+8 width table pointer (256 bytes, 0 = fixed advance), +0xc glyph offset table (`short[256]`), +0x10 glyph data. The large
font's header is unique in the image on both devices, so a words rule is enough; its use by the firmware is only through RAM
wrappers (no static reference found) **[O]**.

Primitives (DT2 / DN2, **[D]**): draw 0x40113d60 / 0x401156d4 and `draw_box` 0x40113fd4 / 0x40115948 take a FontData; `draw_fmt`
0x40113dc8 / 0x4011573c takes a font wrapper (RAM object, FontData pointer at +4). Width `(FontData *, int maxlen, const u32 *str)`:
0x4011467a / 0x40115fee, hash 2f0ac4fb5a0288476cf5cb7b92630562 on both; fixed font n*(advance+1)-1, else the sum of the width
table plus (n-1) **[D]** (it depends on the advance meaning, see above). Glyph columns are ORed into the bitmap (no clear) **[D]**.
The text y counts the text's bottom row from the screen bottom (NameView draws its help lines at y = 0, 6, 0x12) **[D]**. The
orientation of `fill_rect` y against text y is **[O]**.

### Addresses of control block vptrs

`sp_view_cb_vptr_lit` (`cf.view.site.sp_view_cb_vptr_lit` of devices/fw/view.toml) is the immediate of `move.l #vptr,(a0)` in the shared_ptr maker: its offset is 0x2e from the function entry (the immediate), not 0x2c (the opcode word's extension start) **[D]**.

## 6. Open items

- **[O]** Who adds listeners at Brain+0x94, and what a swallowed event hides from them (they run before the walker, so a walker
  hook does not hide the event from them).
- **[O]** The name "idle reset" for 0x40030b50 / 0x4002c548 (the effect, Brain+0xe4 = 0, is **[D]**).
- **[O]** Slot 11 (tick): default `rts`; callers not checked.
- **[O]** UI-task stack headroom for a module C call.
- **[O]** DN2 address of the offer loop, of slots 12 to 14, and of the key-down bitmap.
- **[O]** The DN2 address of the NameView character set global; which FontData each RAM font wrapper holds.
- **[O]** Meaning of FontData header word 0; whether the large font is reached only through wrappers.
- **[O]** FUNC and PAGE release flag 0x00 and the flag values 0x01, 0x03, 0x09, 0x10 (finding 122, not re-derived).
- **[O]** A direct `vtable[2]` call to the dead dispatchers was not scanned exhaustively.
- **[O]** Purpose of the FUNC popup in NameView; size of the +4 (encoder) vtable; LED path of a module view.
- **[O]** The EncoderEvent API beyond slot 2 of NameView.

Conflicts between the reports: the render caller is 0x40032b3a (second check) against 0x40032992 (first report); the
second check is used. The DN2 caller function of the runner is a switch case, not `main_os_task`. Slot 11 and the "dispose calls
slot 1" claim: see sections 3.
