# UI components on DT2 1.17 and DN2 1.12

Marks: **[D]** read once, **[O]** open or guess, **[C]** corrects an earlier
claim. Nothing here is **[V]**. Plan:
[firmware-decls-and-sdk](../plans/firmware-decls-and-sdk.md). Earlier UI
notes: [finding 03](03-ui-and-panel.md). Existing anchors:
[dt2-1.17.toml](../../devices/ports/dt2-1.17.toml).

## 1. Scope and method

- Three static inventories of the UI framework: windows and dialogs, menus
  and lists, navigation and drawing. Tool: `tools/coldfire/fw.py`.
- DN2 pairs come from the anchor catalogue (`pair_addr`).
- "same" = strict hash pair with identical bytes. "size" = size-only match,
  not hash-checked **[O]**. "none" = no pair found.
- All addresses are ColdFire addresses (0x4xxxxxxx). Offsets were read on
  DT2 only. Whether DN2 uses the same field offsets is **[O]**. Strict ctor,
  MenuItem and paint pairs make it likely.

## 2. UI framework

ViewController is the view manager. It sits at Brain+0x40 **[D]**. `mgr_of`
(0x40030a42) returns `Brain+0x40`. View+0x2c holds the manager pointer.

| ViewController field | Meaning |
| --- | --- |
| +0x04 | lock |
| +0x14 | std::list sentinel. Node: +0 next, +4 prev, +8 View*, +0xc shared_ptr control block |
| +0x18 | last node (top of stack) |
| +0x1c | first node to draw |
| +0x20 | redraw-needed byte |
| +0x22 | close-pending byte |
| +0x23 | byte, set by relayout **[O]** |

- The list is bottom first. Dispatch starts at `sentinel+4` and walks prev,
  so the last node is the top and gets events first. Draw walks from +0x1c
  with next **[D]**.
- Stock views are live at the same time (MainScreenView, TransportView,
  page views, LED views, PatternGridView, KeyboardView). See finding 03.
- `push` at 0x4011c08a is the existing anchor `cf.menu.fn.view_add`. It is
  `bool push(mgr, shared_ptr<View>*, bool replace_same_class)`. It closes
  views of equal class id when `replace` is 1, skips if 0x4011b208 rejects,
  allocates a 0x10 node, runs relayout, and calls View slot 6 **[D]**.
- `View::push` at 0x4011b22a is a thin wrapper. It returns 0 if `this+0x2c`
  is null. It has about 230 call sites **[D]**.

### Functions

| Role | DT2 | DN2 | Same? |
| --- | --- | --- | --- |
| Brain get | 0x40198c24 | 0x4018cbbc | same |
| mgr_of | 0x40030a42 | 0x4002c47a | same |
| ViewController::push | 0x4011c08a | 0x4011d9fe | same |
| View::push | 0x4011b22a | 0x4011cb9e | same |
| Brain-level push (`cf.menu.fn.push`) | 0x4011a592 | 0x4011bf06 | same |
| invalidate | 0x4011b126 | 0x4011ca9a | same |
| relayout | 0x4011bfa6 | 0x4011d91a | same |
| remove view | 0x4011c1e8 | 0x4011db5c | size |
| pop top | 0x4011c314 | 0x4011dc88 | size |
| sweep closed views | 0x4011c36c | 0x4011dce0 | size |
| find by class id | 0x4011be46 | none | none |
| find by pointer | 0x4011bf04 | none | none |
| set close flag | 0x4011bc54 | 0x4011cac6 | size, **[O]** |
| render | 0x4011bc96 | not resolved | none |
| frame tick (labelled `main_ui_render`) | 0x40032992 | not resolved | none |
| per-view tick | 0x4011c93c | about 100 bytes **[O]** | size |
| generic dispatch walk | 0x4011c41a | 0x4011dd8e | size |
| encoder dispatch | 0x4011c674 | 0x4011dfe8 | size |
| key dispatch | 0x4011c4e8 | 0x4011de5c | size |

- invalidate: `if(!this[0x17]){this[0x14]=1; if(this[0x2c]) mgr[0x20]=1}`
  **[D]**.
- Top of stack: no function. Read `*(mgr+0x18)`, node+8 **[O]**.
- Close all or back to main: none found. Views close themselves with slot
  10. MainScreenView is never removed **[O]**.

### View vtable

Class View: typeinfo 0x4021df98, primary vtable 0x4021dfd0. Bases: KeyHandler,
EncoderHandler, MidiHandler, AnalogHandler, NoteHandler. Secondary vtables
at +4, +8, +0xc, +0x10 (0x4021e01c, 0x4021e030, 0x4021e044, 0x4021e058).

| Slot (byte offset) | Default | Meaning |
| --- | --- | --- |
| 0, 1 (+0, +4) | 0x4011b2c4 / 0x4011b37c | dtor, deleting dtor **[D]** |
| 2 (+8) | 0x4011afe0 | key event, arg KeyEvent*, returns consumed. Default routes codes 0x13/0x14/0x15 to slots 12/13/14 **[D]** |
| 3 (+0xc) | 0x4011afa4 | class id used by push **[D]**; exact return **[O]** |
| 4 (+0x10) | pure; ConfirmWindow 0x40111234 | draw(Bitmap* screen) **[D]** |
| 5 (+0x14) | 0x4011b3b2 | attach(mgr): sets +0x2c, +0x30=0, invalidates, runs attach callbacks **[D]** |
| 6 (+0x18) | 0x4011afb0 | on_push, called by push after insert **[D]**; name **[O]** |
| 7 (+0x1c) | 0x4011b422 | detach(mgr): runs detach callbacks, +0x2c=0 **[D]** |
| 8, 9 (+0x20, +0x24) | 0x4011afbc / 0x4011afc8 | set and get flag at +0x16, "opaque". Views below the topmost opaque view are not drawn **[D]**; name **[O]** |
| 10 (+0x28) | 0x4011b1ca | close: sets View+0x30=1 and mgr+0x22=1. Swept later **[D]** |
| 11 (+0x2c) | 0x4011afd2 (rts) | per-tick update **[D]** |
| 12, 13, 14 | rts | key 0x13 / 0x14 / 0x15 down (RECORD, PLAY, STOP per [finding 19](19-sequencer-and-leds.md)) **[D]** |
| 15, 16 | 0x4011b550 / 0x4011b55c | tail call to 0x401d2140, unknown **[O]** |
| 17 and up | per view | derived classes have about 45 to 80 slots **[D]** |

View fields: +0x14 dirty, +0x16 opaque, +0x17 invalidate-blocked, +0x2c
manager, +0x30 closing, +0x34 and +0x40 attach and detach callback vectors
(16-byte std::function) **[D]**. View fields of derived windows start at
+0x4c. No focus slot found; only attach, detach and on_push **[O]**.

### Redraw path

1. A view calls invalidate. This sets the dirty byte and `mgr[0x20]`.
2. Frame tick (0x40032992): counts the screensaver at Brain+0xe4..0xf0
   (TimeoutView after N*30 ticks). If `mgr[0x20]`, it makes a 0x80 x 0x40
   Bitmap and calls render **[D]**.
3. Render (0x4011bc96): if `mgr[0x20]`, clear the bitmap with 0x40113700,
   then for each view from +0x1c to the top call slot 4, then 0x4011b152
   (clears dirty). Set `mgr[0x20]=0` **[D]**.
4. The frame tick swaps the front buffer through 0x40133bea **[D]**.

### Input routing

- Source: `main_os_task` 0x400337ba. `FUN_40001928` returns a record. Type 0
  is a key (record+4 code, +8 state, +0xc timer). Type 1 is an encoder. See
  finding 58 **[D]**.
- The encoder event is posted by the constructor at 0x40030b64 (mislabelled
  `EncoderEvent::ctor`). Encoder dispatch is 0x4011c674 **[D]**.
- Key dispatch 0x4011c4e8 has no direct caller. It runs through the
  std::function list at Brain+0x94, run by 0x40030bd0 **[D]**.
- KeyEvent ctor 0x40114904. Fields: +0xc code, +0x10 flags (bit 0 valid,
  bit 1 press, getters 0x40114a28 and 0x40114a18), +0x14 **[D]**. EncoderEvent
  vtable 0x401f10bc, ctor 0x401119b8 **[D]**.
- Dispatch stops at the first view that accepts. Returning 0 from a key
  handler lets stock handling continue **[D]**.
- FUNC combinations are decided inside the views, not in a central table.
  FUNC+SRC is offered top-down until MainScreenView consumes it **[D]**.
- Key codes: code = channel*8+bit+1 for 1..48 (finding 58). 1..6 pages,
  7..9 modes, 0x10..0x12 modifiers (FUNC 0x11), 0x13 RECORD, 0x14 PLAY,
  0x15 STOP, TRIG 1..16 = 0x19..0x28. Names for 0xa..0xf are **[O]**; see
  section 7.

## 3. Drawing primitives

All take Bitmap* first. DN2 pairs are all "same".

| Role | DT2 | DN2 | Signature guess |
| --- | --- | --- | --- |
| plain text | 0x40113d60 | not given | 36-byte wrapper that calls 0x40113a84 with flags 0 **[D]** |
| printf text | 0x40113dc8 | 0x4011573c | `int (Bitmap*, Font*, x, y, flags, fmt, ...)`, returns advance |
| text box (aligned) | 0x40113fd4 | 0x40115948 | `(Bitmap*, Font*, x0, x1?, y, ..., fmt, ...)` **[O]** |
| text width | 0x4011467a | 0x40115fee | `int (Font*, int maxlen, basic_string*)` |
| rectangle outline | 0x40112f92 | not given | draws four lines through 0x40112660 **[D]** |
| fill_rect | 0x401131ae | 0x40114b22 | `(Bitmap*, x0, y0, x1, y1, color 0/-1)`; also has a mode |
| invert rect | 0x401132c0 | not given | **[D]** |
| inner fill (scroll bar) | 0x401134c0 | not given | **[D]** |
| blit | 0x40114168 | 0x40115adc | `(Bitmap* dst, Bitmap* src, x, y, bool centre)` |
| clear | 0x40113700 | 0x40115074 | `(Bitmap*, ?, mode)` |
| scroll bar | 0x401143f2 | 0x40115d66 | see below |
| toast | 0x4005b23a | 0x400562e6 | `void (UIStates*, fmt, ...)` |

- Font pointer 0x402a9be0 is used as the font argument by
  VerticalMenuView (+0xa8, +0xac). Specific font objects are not identified
  **[O]**.
- Font object: +0 fixed advance, +4 and +8 sizes, +8 a pointer to a per-char
  width table if non-null (proportional width = sum of table[c] + (n-1)).
  Glyph data from `FUN_40114670` **[D]**.
- Text flags of 0x40113dc8 (param 5) **[D]**:

| Bit | Meaning |
| --- | --- |
| 0 | alternate (proportional) path |
| 1 | centre |
| 2 | right-align |
| 3 | draw outline after (invert box) |
| 4 | clear box before (bits 3 and 4 = 0x18) |
| 5 | centre vertically |
| 6 | bottom-align |
| 7 | translate (calls 0x401360ac) |

  Text is formatted with vsnprintf into a 128-byte buffer (0x400005e0).
  Height comes from 0x4011480e. Width helpers: 0x4011482a, 0x40114874.
- Scroll bar: `scrollbar(canvas, x0, y0, x1, y1, offset, visible, count)`.
  It draws the frame with 0x401131ae, fills the inner area with 0x401134c0,
  and sets the thumb height to `track*visible/count` **[D]**. VerticalMenuView
  calls it at x = w-5..w-1 only when count > visible. It has 18 callers.
- Toast and popup **[D]**: the toast needs no view. `UIStates` is a
  singleton of 0x2f8 bytes, returned by 0x401998c0 (DN2 0x4018b2a4; created on
  first use). 0x4005b23a `(ui, fmt, ...)` and 0x4005b1e0 `(ui, const string*)`
  (DN2 0x400562e6, 0x4005628c) store up to 32 characters; a message that is
  already shown is not replaced while `UIStates+0x45` is nonzero (the byte
  suppresses toasts). PopupWindow (section 5) is a pushed view; its auto-close
  stays **[O]**.
- Toast: if `UIStates+0x45` is 0, vsnprintf into 256 bytes, copy up to 32
  chars to a string at `UIStates+0x48`, set +0x4c from 0x4005b1ba (timeout),
  call 0x400f4888 (redraw) **[D]**.
- Screen Bitmap: vtable 0x4021cff0, ctor `(this, w, h, buf, buf2)`, screen
  0x80 x 0x40. 1 bpp. +4 width, +8 height, +0xc number of 32-bit row bands
  (bands of 32 rows; fill_rect indexes `y>>5`), data pointer at +0x14
  **[D]**. **[C]** The labeller guess of +0x10 for the data pointer is wrong.
  `clear` reads `*(this+0x14) + band*4`.

## 4. Menus and lists

### Menu (base class)

Vtable 0x4021d048, typeinfo 0x4021d034. Ctor 0x401151be
`(this, visible_rows, cols, reserve, marked=-1)` (DN2 ctor 0x40116b32)
**[D]**.

| Off | Field |
| --- | --- |
| +0 | vptr |
| +4, +8, +0xc | vector of `shared_ptr<MenuItem>` (8 bytes each); count = (end-begin)>>3 |
| +0x10 | selected row index, -1 = none |
| +0x14 | scroll offset (first visible row) |
| +0x18 | cursor row in the window (selected - scroll) |
| +0x1c | visible rows |
| +0x20 | arg 3 (1; TwoColumn 8). Columns? **[O]** |
| +0x24 | "marked" index (arg 5, -1). VMV paint draws a tick box for it. Current-choice marker? **[O]** |
| +0x34 | byte flag, init 0 **[O]** |

**[C]** The catalogue labels the word at +0x10 as a "flag getter". It is
Menu+0x10, the selected row index. `FUN_40114bf6` clamps it and
`FUN_40114f44` writes it.

| Vtable offset | Function | DN2 |
| --- | --- | --- |
| 0x08 | addItem(shared_ptr<MenuItem>) 0x4011527a | 0x40116bee |
| 0x0c | count 0x40114a9c | 0x40116410 |
| 0x10 | itemAt(idx) 0x40114ebe | 0x40116832 |
| 0x14 | clear 0x4011539c | 0x40116d10 |
| 0x18 | move(delta), wraps 0x40114aac | 0x40116420 |
| 0x1c | select(idx, mode 0/1/2 = scroll policy) 0x40114f44 | 0x401168b8 |
| 0x20 | selectById(id, mode) 0x40115060 | not given |
| 0x24 | selection getter 0x40114bf6 | 0x4011656a |
| 0x30, 0x34 | marked getter, setter | not given |
| 0x38, 0x3c | scroll and cursor getters | not given |
| 0x40, 0x44 | visible rows getter, setter | not given |
| 0x48 | needsScrollbar (count > visible) 0x40114cec | not given |

All DN2 pairs above are strict.

### Lists are Menus

MachineListView, SampleListView, SoundSlotListView, ModDestListView,
VerticalMenuView and the compact and two-column variants all hold a Menu
sub-object. Items are MenuItems. There is no row callback supplier **[D]**.

| Class | DT2 vtable | Ctor | Menu(...) args |
| --- | --- | --- | --- |
| VerticalMenuView | 0x4021dcf4 | 0x40119f6c (DN2 0x4011b8e0) | Menu(5, 1, reserve, -1) |
| CompactVerticalMenuView | 0x4021cbe8 | no ctor symbol **[O]** | same Menu; own paint 0x40110970 |
| TwoColumnMenu | 0x4021dacc | 0x40119b7c(this, reserve) | Menu(16, 8, reserve); plain Menu, not a View |
| MenuWithPopoutSideBarsView | 0x4021d1a8 | 0x40115f7a | based on Compact; panels "leftPanel", "rightPanel" |
| MachineListView | 0x401f6bac | 0x4005ea6e | Menu(6, 1, 0x47, -1), EncoderFilter |
| SampleListView | 0x401ee8c0 | 0x4001e4ae | Menu(arg8, 1, 0x47) |
| SoundSlotListView | 0x40207690 | 0x400b938c(this, a, selectId) | Menu(7, 1, 0x80) |
| ModDestListView | 0x4021a138 | 0x40104b18 (11 args) | Menu(height/7, 1, 0x47); 7 px rows; Grouped subclass 0x40105044 |

PatternGridView is not a Menu (step grid).

### MenuItem

Size 0x54, vtable 0x4021d18c, ctor 0x40115a44 (DN2 0x401173b8)
`(this, label_fn, action_fn, draw_fn, adjust_fn, id=-1, arg7=8)` **[D]**.
Builder 0x40115b90 (DN2 0x40117504) is `cf.menu.fn.item_builder`. It takes
`(item, cstr label, &action, &icon-or-draw, &adjust)`.

| Off | Field |
| --- | --- |
| +0 | vptr |
| +4 | id (-1); used by selectById |
| +8 | init -1 **[O]** |
| +0xc | std::function label `string f(MenuItem&)` (mgr +0x14, inv +0x18) |
| +0x1c | std::function action `void f(MenuItem&)` (mgr +0x24, inv +0x28) |
| +0x2c | std::function draw `void f(MenuItem&, Canvas&, x, y)` (mgr +0x34, inv +0x38). Empty = skip. Stock items put a bitmap blit here |
| +0x3c | std::function adjust `void f(MenuItem&, int delta)` (mgr +0x44, inv +0x48) |
| +0x4c | int = arg7*2 = 16; encoder sensitivity. VMV passes it to 0x40111aaa before adjust |
| +0x50 | byte flag, getter 0x4011588c, setter 0x40115880. "editing" or "locked"? **[O]** |

Item kinds:

- Action item: label + action, rest empty **[D]**.
- Value item: label text, a draw fn that prints the value, and an adjust fn
  that changes it **[D]**. Example LedSettingsMenuView ctor 0x400970a8:
  adjust 0x40095f80 does `v=clamp(v+delta,0,255)` in global 0x402a1ac8 and
  notifies. Draw 0x40095bec blits, then prints at x+0x4e, y+2.
- Toggle: same as a value item with ON/OFF text **[O]**, not traced. The recipe
  that is read: allocate 0x54 bytes, build four `std::function`s (label, action,
  draw, adjust; each a heap-stored 4-byte capture plus a manager and an invoker
  per lambda), call the ctor 0x40115a44 with id -1 and arg7 8, call
  `Menu::addItem` (0x4011527a) with the raw `MenuItem *` (it makes the
  `shared_ptr`), then destroy the four temporaries with 0x40197ce0
  (LedSettingsMenuView 0x400970a8 does this for every row) **[D]**. A toggle
  is an adjust function that flips on any delta and a draw function that prints
  ON or OFF.
- Submenu: an action that pushes a view (digikit uses 0x4011a592) **[D]**.
- Separator, disabled or grey: no field found **[O]**.

### VerticalMenuView

Bases Menu, View, LedHandler. Size 0x1ac. View sub-object at +0x38 **[D]**.
Ctor `(this, name cstr, reserve, const string* title, icon ptr)`.

- Fields: +0x90 sub-view shared_ptr (+0x98), +0xa0 title std::string, +0xa4
  icon, +0xa8 and +0xac font ptr 0x402a9be0, +0xb0 EncoderFilter.
- Layout: row pitch 13 px, rows from the top, row text x = 0x36, icon or
  draw x = 0x2a, selection bar x 0x27..w-8 inverted, title at bottom-left.

| Slot | Role | DT2 | DN2 |
| --- | --- | --- | --- |
| 31 | paint | 0x4011a234 | 0x4011bba8 |
| 32 | deactivate | 0x4011ac42 | 0x4011c5b6 |
| 33 | activate | 0x4011ab9e | not given |
| 34 | key event (id 0xd = press, calls action; up and down) | 0x4011a660 | 0x4011bfd4 |
| 35 | encoder (scales delta by item+0x4c, calls adjust 0x4011584a, else moves) | 0x4011a990 | 0x4011c304 |
| 36 | LED setup | 0x4011a4d6 | not given |

Subclasses: Pattern*MenuView, ProjectMenuView, LedSettingsMenuView,
MidiMenuView, SynthSettingsMenuView, TotalRecallMenuView, SongMenuView,
VerticalSettingsMenuView, DesignerVerticalMenuView (own paint).

### NameView (text entry)

Typeinfo 0x4021d570, vtable 0x4021d5a4, ctor 0x40117ac2 (DN2 0x40119436)
**[D]**:

`(this, string prompt, string initial, int number, int maxlen, char show_number, string number_fmt, std::function<void(std::string)> done)`

- Example caller 0x40084e36 (song name): prompt "ENTER SONG NAME", maxlen
  0x10, show_number 1, format "S%02d ".
- Fields: +0x4c prompt, +0x50 number, +0x6c holds arg 5 (0x10), +0x7c done
  copy, +0x98 "YES SAVE" string. Which field is which is **[O]**.
- Made with make_shared (size 0xb0 with control block). Pushed with
  0x4011b22a(viewmgr+0x38, &sp, 0). Clipboard cleared on close (0x40117d18).
- The `(std::string,int)` lambda in RTTI belongs to PatternSaveMenuView, not
  NameView.

### Not UI values

- KeyboardView (vtable 0x401f68d8, ctor 0x4005c9e2) and KeyboardHandler
  (vtable 0x401f2944, ctor 0x40038e76) are the note keyboard (piano keys).
  They are not text entry **[D]**.
- AbstractValue and Value<T> are persisted project, kit and pattern data
  (for example `Value<Digisharc::kit_t>`). Menu items do not bind to them.
  Each item has its own lambdas **[D]**.
- RTTI lambdas exist only for ProjectMenuView and PatternSave, Clear and
  Reload MenuView. Other menus use plain functions with the same four
  functors **[D]**.

## 5. Windows and dialogs

### Common mechanics

- Base chain: View -> OverlayWindow (vtable 0x4021d7bc, ctor 0x40118c2e
  `(this, w, h)`, DN2 0x4011a5a2) -> Message, Progress, MultiChoice.
  ConfirmWindow, LargeConfirmWindow, PopupWindow and TextWindow derive View
  directly. BlockerWindow derives TextWindow. ConfirmWindowBase has no
  vtable **[D]**.
- Objects use `std::make_shared`: `operator new(size)` = 0x10 control block
  + object. Object size = new size - 0x10. The control-block vptr is
  0x401ed3b0 for ConfirmWindow in 1.17 **[D]**. **[C]** It is not one vptr for all
  windows: each `make_shared` type has its own `_Sp_counted_ptr_inplace`
  vtable (MultiChoiceWindow 0x40204fb4). The vptr is the immediate of a
  `move.l #vptr,(a2)` in the call site. It is declared as the data anchor
  `cf.window.site.confirm_cb_vptr_lit` / `...multichoice_cb_vptr_lit`
  (DN2: 0x401dd7e4 / 0x401edba4). The `_Sp_counted_ptr_inplace<...>::ctor_dtor`
  labels are really call-site functions that hold new, ctor and push.
- std::function is 16 bytes: storage[8], manager at +8, invoker at +0xc
  (libstdc++ style). Invoke helper 0x4019c89a(fn, arg) throws
  bad_function_call if the manager is 0, else tail-calls the invoker **[D]**.
- Strings are libstdc++ COW `std::string`, passed by pointer. Builder
  0x401e7e7a(char*). Ctors copy with 0x401e7570 **[D]**.

- The view stack on 1.17 (DT2) **[D]**: the Brain pointer is the global 0x44f37098, the one that the Brain getter 0x40198c24 tests with `tst.l`
  (anchor `cf.view.value.brain`). The view manager is Brain + 0x40; its views are a `std::list` with the sentinel node at manager + 0x14 (+0 next,
  +4 prev). A node holds the `View *` at +8 and the shared_ptr control block at +0xc. The key dispatcher starts at the last node, the top view. A
  View stores the vtable symbol + 8 at +0: ConfirmWindow 0x4021cd50 (result at +0x4c: 0 unanswered, 1 YES, 2 NO; title data pointer at +0x50, text
  at +0x54), ProgressWindow 0x4021d980 (ctor 0x401196c4, a `make_shared` object of 0x88 bytes; base OverlayWindow). `MainScreenView::vfunc_5`
  (0x40064644) calls FUN_400110de, which pushes a ProgressWindow titled "VERIFYING FILE SYSTEM..." and queues the filesystem check job. The job's
  completion callback 0x40011670 is the only code that builds the ConfirmWindow titled "FILE SYSTEM OK"; it then calls 0x400105aa, which sets the state word
  (RAM 0x46f4dcb4) to 4 and the checked word (0x46f4dce4) to 1. FUN_400110de runs only when 0x4001058c is true: the filesystem is mounted (flag 0x44f2bd68
  nonzero) and the state word is 3 or the checked word is 0 (0x4015a834 and 0x4015a8e6 read them under the lock 0x46f4deb0; a call with the argument 0, as
  vfunc_5 makes it). The words are a copy of the superblock at flash 0x5d8000 (magic 0x656b4653, finding 14), so a boot whose copy has state 4 and checked 1 runs no check and
  shows no window. The device file `devices/digitakt-ii.toml`
  holds these values in `[ui]` and `[ui.boot_window]`; the Digitone II has no such window (finding 03 "Modal windows take every key").

### Class table

| Class | Size | Vtable | Ctor DT2 / DN2 | Ctor args | Push |
| --- | --- | --- | --- | --- | --- |
| ConfirmWindow | 0x7c | 0x4021cd48 | 0x4011139a / 0x40112d0e (same) | `(this, string* title, string* text2, std::function<void(bool)>* cb, char flag)` | 0x4011c08a(brain+0x40, sp, 0) |
| LargeConfirmWindow | 0x88 | 0x4021cad8 | A 0x4010fdcc (no pair); B 0x4010ff9c / 0x40111910 (same) | A `(this, vector<12-byte items>*, func* cb)`; B `(this, string* text, func* cb)` splits text at 10 | 0x4011c08a |
| MultiChoiceWindow | 0xd8 | 0x4021d420 | 0x40116b1e / 0x40118492 (same) | `(this, vector<string>* items, string title, func<void(int)>* cb, char horizontal)` | 0x4011b22a |
| MessageWindow | 0x70 | 0x4021d304 | 0x401163b6 / 0x40117d2a (same) | `(this, string line1, string line2)` | via 0x4008ee66, 0x401bd180 **[O]** |
| PopupWindow (toast) | 0xa0 | 0x4021d8c0 | 0x40119054 / 0x4011a9c8 (same) | `(this, int, string text, char, int, func a, func b)`; helpers 0x401191c8(this, text), 0x40119234(this, text, flag, n) | 0x4011b22a or 0x4011c08a |
| ProgressWindow | 0x88 | 0x4021d978 | 0x401196c4 / 0x4011b038 (same) | `(this, const char* title)` | 0x4011c08a |
| TextWindow | n/a | 0x4021da24 | 0x40119a2c / 0x4011b3a0 (same) | `(this, string* lines, Font*, int, int)` | n/a |
| BlockerWindow | 0x78 | 0x4021ca10 | 0x4010fb6a / no pair (DN2 has the class) | `(this, string text)` calls TextWindow(text, "", 6, 2) | 0x4011c08a **[O]** |
| OverlayWindow | base | 0x4021d7bc | 0x40118c2e / 0x4011a5a2 (same) | `(this, w, h)` | n/a |
| CalibrationCountdownWindow | n/a | 0x4021c3e0 | 0x4010d446 / 0x4010edba (no recorded pair) | `(this, std::function* cb)` | no callers found **[O]** |
| HelpBubbleView | 0xa8 | 0x40216f0c | 0x400f6e8e / 0x400f8922 (same) | `(this)`; bases include Timer | created in KitBrowser ctor 0x40066584 |
| SongModePopup | 0x160 | 0x401fc344 | 0x400856d0 / 0x4007fc04 (same) | `(this, owner)` | MainScreenView::vfunc_2 |
| SampleBrowserPopupMenuView | 0x140 (SampleManager member) | 0x401ef1c4 | 0x40023370 (mislabelled Font::ctor_dtor) / none | embedded panel "popupPanel" | n/a |
| AssignPrompt | 0x60 | 0x401edeac | 0x40018eea / none | `(this, string)` | 0x400218dc |

ConfirmWindow::vfunc_0 pairs to 0x401b9dd4. SongModePopup, AssignPrompt,
SampleBrowserPopupMenuView and CalibrationCountdownWindow are feature
specific, not SDK material **[O]**.

### ConfirmWindow

- Callback type is `lambda(bool)`. The lambda is stateless (1 byte). Invoker
  thunks such as 0x401b4dda belong to each call site **[D]**. Result field
  +0x4c: 1 = yes, 2 = no.
- Key handler vfunc_2 0x401114f0 **[D]**: key id 10 on press = YES: close,
  callback(true). Key id 12 = NO: close, callback(false). Keys 0x19..0x28
  are swallowed unless the flag at +0x6c is set. All other keys return
  handled (the window is modal). Physical names of 10 and 12 are **[O]**.
- Layout and callback **[D]** (step 3b re-read of ctor 0x4011139a and handler
  0x401114f0): result +0x4c, title string pointer +0x50, text pointer +0x54,
  line count +0x58 (1, or 2 when the text is not empty), callback
  `std::function<void(bool)>` +0x5c (the ctor clones it with manager op 2), flag
  +0x6c, window size 0x7c, `make_shared` block 0x8c. Title and text are cut to
  0x50 characters. The handler reads the key id with 0x40114984 and tests press
  with 0x401149ac. The callback runs through the invoke helper 0x4019c89a
  (`fn, bool`) after the window closed itself (View slot 10).
- **[C]** Key ids: 10 = YES, 12 = NO. They are the panel keys YES and NO
  (`devices/digitakt-ii-leds.toml`: 10 YES, 11 UP, 12 NO, 13 LEFT, 14 DOWN,
  15 RIGHT). Both act on press only.
- Draw 0x40111234: box centred at x = 0x40, text via 0x40113dc8 at +0x50
  (title) and +0x54 (second line), one or two lines **[D]**.
- About 48 call sites. Example strings:

| Call site | Title / text |
| --- | --- |
| 0x401b660c | "CLEAR SEQUENCE" / "CONFIRM Y/N" |
| 0x40084d02 | "CLEAR SONG?" |
| 0x40019d06 | "CURRENT SLICE POINTS WILL BE REPLACED, PROCEED? Y/N" |
| 0x400215f4 | "FILE ALREADY EXISTS" / "OVERWRITE? Y/N" |
| 0x400a3f7c | "ANY CHANGES WILL BE LOST BEFORE LOADING" |

### MultiChoiceWindow

Key handler 0x40116e44 **[D]**, re-read in step 3b. The constructor argument
`vertical` (last argument, stored at +0xac) decides the layout: 0 puts the
items side by side (width = sum of item widths), nonzero stacks them (width =
widest item). **[C]** The earlier text said ids 0xb/0xd with a "layout flag";
the ids are: vertical, 11 (UP) = previous and 14 (DOWN) = next; side by side,
13 (LEFT) = previous and 15 (RIGHT) = next. The selection is clamped, not
wrapped, and starts at 0 (+0x9c). Id 10 (YES) or 0x31 selects: invoke the
callback with the index through 0x40198b5c (`fn, int`), then close. Id 12 (NO)
cancels: close without callback. Id 0x11 goes to the View default. Fields:
item vector +0x74..+0x7c (4-byte COW strings), title lines vector +0x80,
callback `std::function<void(int)>` +0x8c, selection +0x9c, flag +0xac; size
0xd8, `make_shared` block 0xe8. The window starts as 0x40 x 0x20 and grows to
the content. The stock call site 0x400a9f54 builds the callback as a lambda
with 4 bytes of captured state on the heap (storage word 0 points to it). Callback
`std::function<void(int)>` at +0x8c. Call site 0x400a9f54: title "SELECT
INTERFACE", item "USB MIDI" and others, pushed with 0x4011b22a.

## 6. Proposed SDK mapping (not built) **[O]**

| SDK call | Firmware component | Anchors needed |
| --- | --- | --- |
| `dk_ui_view_new(ops, user, opaque)` | trampoline View class | View vtable 0x4021dfd0 and secondary vtables, View ctor (not read), operator new 0x4011e954, control-block vptr |
| `dk_ui_push` | View::push / ViewController::push | 0x4011b22a, 0x4011c08a, Brain get, mgr_of |
| `dk_ui_back`, `dk_ui_close` | View slot 10; top = `*(mgr+0x18)` | mgr_of; mgr layout |
| `dk_ui_invalidate` | View::invalidate | 0x4011b126 |
| `dk_ui_top` | read `*(mgr+0x18)`, node+8 | mgr_of; list layout |
| `dk_ui_text`, `text_fmt` | printf text, plain text | 0x40113dc8, 0x40113d60, font ptr |
| `dk_ui_text_width` | text width | 0x4011467a |
| `dk_ui_rect`, `dk_ui_fill` | rectangle outline, fill_rect | 0x40112f92, 0x401131ae |
| `dk_ui_blit` | blit | 0x40114168 |
| `dk_ui_scrollbar` | scroll bar | 0x401143f2 |
| `dk_ui_toast` | toast | 0x4005b23a |
| `dk_ui_menu`, `_clear`, `_select`, `_open` | VerticalMenuView, Menu vslots 0x14, 0x1c | view_ctor 0x40119f6c, new, str_ctor 0x401e7e7a, str_dtor 0x401e63ca, make_sp 0x4011a0ba, push 0x4011a592 (all existing in `dt2-1.17.toml`) |
| `dk_ui_menu_action`, `_submenu` | MenuItem via item_builder | 0x40115b90, MenuItem layout, std::function ABI, own manager and invoker |
| `dk_ui_menu_value`, `_toggle` | fill MenuItem +0x2c draw and +0x3c adjust after item_builder | text 0x40113d60, blit 0x40114168, invalidate; font ptr; sensitivity +0x4c. No new menu anchor |
| `dk_ui_menu_text` | item with no action | digikit text_page copies VMV View vtable slot 4 |
| `dk_ui_text_input` | NameView | ctor 0x40117ac2, make_shared wrapper, push 0x4011b22a, std::function<void(std::string)> with COW string ABI |
| `dk_ui_confirm` | ConfirmWindow | ctor 0x4011139a, push 0x4011c08a, Brain accessor, new, str builder 0x401e7e7a, hand-built std::function(bool) |
| `dk_ui_choice` | MultiChoiceWindow (cancel gives no callback) | ctor 0x40116b1e, push 0x4011b22a, std::function(int) |
| `dk_ui_message` | MessageWindow | ctor 0x401163b6 |
| `dk_ui_popup` | PopupWindow helper 0x401191c8 | ctor 0x40119054; auto-close **[O]** |
| `dk_ui_progress_open/set/close` | ProgressWindow | ctor 0x401196c4; update API not read |

Main risk: the adapter needs a trampoline View class. It is a copy of the
stock View vtables with module callbacks in slots 0, 1, 2, 4, 5, 7, 10, 11.
The manager also holds a `shared_ptr<View>`, so the adapter must build a
valid control block (use count +4, weak count +8, dispose and destroy in
vtable slots 2 and 3), for example following the inplace pattern of
TimeoutView (vtable 0x401f1248). The object dtor must still run. Calling a
View base ctor is safer than hand-initialising the fields. Object size and
ctor are not read **[O]**.

## 7. Conflicts and open questions

- **Key names 10 to 15.** **[C]** Resolved in step 3b. The key table in
  `devices/digitakt-ii-leds.toml` names 10 YES, 11 UP, 12 NO, 13 LEFT, 14 DOWN,
  15 RIGHT. ConfirmWindow (10, 12) and the vertical MultiChoiceWindow (11, 14)
  use them this way. [Finding 50](50-dt2-src-second-page.md) says UP and DOWN
  are 0xb/0xc: that is wrong for DOWN (14); 12 is NO. The conclusion of finding
  50 does not change: UP and DOWN reach the same page handler.
- Page switching: which variable holds the current page, and how the old
  page is hidden, was not traced. MainScreenView::vfunc_2 (0x400649c4) calls
  push at about 10 sites.
- The View encoder slot was not traced.
- DN2 pairs for render and frame tick are not resolved. Size-only DN2 pairs
  need a hash check.
- How stock menus grey out items and draw toggles. Read a MidiMenuView
  ON/OFF item. Meaning of MenuItem +0x8, +0x50 and Menu +0x20, +0x34.
- Argument meaning of text, text box and blit before a value draw is
  exposed. NameView field map, length limit, cancel behaviour.
- ProgressWindow update API and PopupWindow auto-close are not read.
- Labeller mislabels seen: 0x40032992 `main_ui_render` (frame tick),
  0x40030b64 `EncoderEvent::ctor` (posts the event), 0x40023370
  `Font::ctor_dtor` (SampleBrowserPopupMenuView ctor), `Sp_counted_ptr_inplace
  ::ctor_dtor` (call-site functions), the Bitmap data pointer at +0x10, and
  the "flag getter" word at +0x10 (see sections 3 and 4).
- Two labellers agree on the function role far more often than on the anchor
  name: 21 of 60 have the same subsystem and 12 of 60 the same name. Anchor
  names need a fixed vocabulary; see step 3 of
  [firmware-decls-and-sdk](../plans/firmware-decls-and-sdk.md).

## 8. List views with a side panel

PRESET MANAGER is `SoundManager` over `MenuWithPopoutSideBarsView` (KitManager and ProjectManager are the same
base). Addresses are DT2 1.17; the DN2 1.12 pairs are in `devices/fw/menu.toml` and `view.toml`. All were found by
the hash rules and resolve on both devices.

- **Classes [D].** `CompactVerticalMenuView` (ctor 0x40110624, mislabelled `Font::ctor_dtor`) is a Menu at +0 and a
  View at +0x38, 0x1c0 bytes, vtable 0x4021cbe8 with 39 slots (slot 0 at 0x4021cbf0, the address the ctor stores).
  `MenuWithPopoutSideBarsView` (ctor 0x40115f7a, vtable 0x4021d1a8, 43 slots) holds three of them: the main list at
  +0, `leftPanel` at +0x1c0 (x 0 to 0x2b), `rightPanel` at +0x380 (x 0x43 to 0x7f), then the active-part pointer at
  +0x540 and the bytes has_left +0x544, has_right +0x545. The main list takes x 0 or 2 to 0x7f or 0x7d. The base
  size is 0x548: the first member of SoundManager, KitManager and ProjectManager is at +0x548.
- **Vtable slots [D].** Menu slots 0 to 30; 31 paint, 32 key, 33 encoder, 34 to 38 hooks of the compact view. The
  popout class overrides 31, 32, 33, 38 and adds 39 main-list paint hook, 40 rebuild, 41 populate the right panel,
  42 left-panel hook. Slots 40 and 41 are empty stubs (0x401d181a, 0x401d181c) in the base; a subclass copies the
  table and overrides 39, 41 and 42.
- **Focus [D].** `popout_set_active(this, part)` (0x40115f30) clears the focus byte (+0x1ac) of the three lists, sets
  it on `part` and invalidates the view. The key handler (slot 32) stores +0x540 itself. LEFT (13, press bit 0): from
  the main list, call slot 42 and activate the left panel (only with has_left); from the right panel, go back to the
  main list. RIGHT (15) is the mirror: with exactly one checked row, it calls Menu slot 7 (select) on the first
  checked entry, then slot 41, then activates the right panel (only with has_right). NO (12, bit 4): from a panel,
  activate the main list; from the main list, close the view (View slot 10). The item actions in the subclasses
  (0x4008cae8, 0x4008dada, 0x4009005a and others) return the focus themselves: they call `popout_set_active(view,
  view)`. Nothing else moves the focus back after an action.
- **Check marks [D].** A Menu has a `vector<int>` of checked row indexes at +0x28, +0x2c, +0x30 and a multi-select
  byte at +0x34 (it was open in section 4; this is it). Slots: 5 clear items (also zeroes selected, scroll, cursor),
  20 clear checks, 21 check (push_back, no duplicate test), 22 uncheck, 24 checked count, 25 is checked, 26 toggle,
  27 multi getter, 28 enable multi, 29 disable multi, 30 select all. The popout ctor enables multi. The compact paint
  (0x40110970) draws the one-character string at 0x40229d55 with `cf.text.fn.draw` (maxw 1) 5 pixels left of the row
  text, for each checked row, when multi is on. The string holds the single byte 0x1b; what that glyph looks like in
  the UI font is **[O]**.
- **Keys [D].** The key event has the code at +0xc and flags at +0x10. The compact handler (0x40110ec6) moves on 11
  (UP) and 14 (DOWN) and calls the item adjust function on 13 and 15, all on flag bit 0; flag bit 1 with the byte
  +0x1ad set moves a page. Key 10 (YES) and key 0x31 act on flag bit 4: they run the action of the selected row
  (item +0x1c function). Key 12 acts on bit 4 too. The names of bits 0, 1 and 4 are **[O]**. **[C]** Section 3 calls
  bit 1 "press".
- **Title and rows [D].** `set_title(view, string*)` (0x4011083c) keeps the title at +0x1bc, cut to 0x78 pixels
  ("..", then the tail). The byte at +0x1b8 is the title-bar flag: the ctor stores its `sub_rows` argument there
  after subtracting it from the rows, and `set_title_bar(view, on)` (0x401108f0) takes or gives back one row when
  the flag changes. `compact_set_rect` (0x40110798) sets the rectangle and rows = height / pitch; it ignores the
  title row, so call it before `set_title_bar`. **[C]** The first research note said the ctor leaves +0x1b8 unset
  and that `make_view_sp` wraps a raw pointer; the first is wrong, and 0x401b9170 is a converting copy from a
  `shared_ptr<Derived>` (View at +0x38) to `shared_ptr<View>`.
- **Recipe [D]** (read in 0x40091cae, the PRESET MANAGER opener): make a shared SoundManager (the stock make_shared
  block, as for ConfirmWindow), `set_title(obj, "PRESET MANAGER")`, `set_title_bar(obj, 1)`, convert with
  0x401b9170 into a `shared_ptr<View>`, `view_push(parent_view, &sp, 0)` (0x4011b22a), release both pointers, then
  invalidate the parent. Items are added with `Menu::addItem` and `MenuItem` as in section 4; the check functions
  above are called on the Menu part.

Open **[O]**: the make_shared control block of a module-built popout view (each class has its own vtable literal, as
for ConfirmWindow); the exact content of the stock slot 39 and 42 hooks; the meaning of focus byte +0x1ac beyond the
selection bar; the glyph 0x1b.

## 9. core/ui on the device (first run)

Written while building `modules/core/adapters/ui` and checked in the emulator on DT2 1.17 and DN2 1.12 with `test/ui-demo`
(then an image module, driven by a Python tool that has since been removed). Marks: **[D]** seen in the emulator or read once; none is **[V]**.

- **View vtable, not the Menu vtable [C].** The key and paint of a Menu view are reached from the View manager through the View
  sub-object's vtable (at +0x38). Its entries are thunks (`moveq #-0x38,d0; add.l d0,4(sp); bra <function>`) that branch to the
  function directly. Overriding slot 32 or 31 of the primary (Menu) vtable changes nothing the manager calls. core/ui copies the
  17-slot View vtable (plus the two words before it) and replaces slot 2 (key, a list) and slot 4 (paint). **[D]**
- **Stock RIGHT needs one checked row [D].** The popout key handler (0x401160bc) opens the right panel on key 15 only with exactly one
  checked entry; check marks are state of the list, so core/ui handles RIGHT itself: clear the right panel's items
  (`clear_items`), call the descriptor's `panel`, then `popout_set_active(view, right panel)`. `popout_set_active` does store +0x540
  (the note on `cf.menu.fn.popout_set_active` says it does not; 0x40115f56 stores it). **[C]**
- **A panel row returns the focus itself [D].** The row's action calls `popout_set_active(view, view)` first, then the module's
  callback, as the stock PRESET MANAGER rows do.
- **Empty list [D].** A `MenuWithPopoutSideBarsView` with no items faults (access error) on the first frame after the push: the
  stock paint, and the LED or key code after it, read the item at the selection. core/ui keeps one hidden item in an empty list,
  paints the title and the empty text itself and swallows the keys except NO. A check of the selection at -1 did not help.
- **Views built by us [D].** `cf.string.fn.shared_ptr.make` (`_Sp_counted_ptr<View*>`) takes any `View*`, here the View sub-object
  of the popout view (+0x38), and registers a detach callback (View slot 16) that clears the `shared_ptr` at `parent+0x90`. A
  zeroed heap block of 0xa0 bytes as the parent makes that clear a no-op. The module keeps the returned `shared_ptr`, so the
  firmware never frees the view; the view stack pushes a copy with `view_controller_push(brain view manager, &sp, 0)`.
- **Dialogs [D].** The make_shared block is `{control-block vptr, 1, 1, block+0x10}` then the window at +0x10; the vptrs are the
  `cf.window.site.confirm_cb_vptr_lit` and `...multichoice_cb_vptr_lit` anchors (data anchors that the generated C header does not
  declare: core/ui declares them with `__asm__`). The std::function callbacks take `(any_data*, int)`, the argument by value
  (a `bool` arrives as a long). A View's detach callbacks (slot 16 adds one) run when the manager removes the view; core/ui uses one
  to notice a choice cancelled with NO (no callback exists for it). After the push the stack holds the only count.
- **Item draw and adjust [D].** The draw function of a MenuItem is called as `(any_data*, MenuItem*, canvas, x, y)` with `x`, `y`
  by value; the stock value rows print the value right-aligned with `draw_box(canvas, font, x + 0x4e, y + 2, 2, 0, 0, "%03d", v)`
  and core/ui does the same. The font of a VerticalMenuView is the pointer at +0xa8; of a compact view the font object is embedded at
  +0x94. The adjust function is `(any_data*, MenuItem*, delta)`.
- **Toast [D].** A toast is drawn by the main screen: with a menu open it is not visible. It shows on the main screen after the menus
  closed.
- **Compact paint [D].** The title is drawn with `draw_fmt(canvas, font, 0x40, 0x41 - [view+0x84], 2, title)`; `[view+0x84]` is 7.
- **MultiChoice paint reads `selected` only for the highlight [D].** The paint (DT2 0x4011693c, DN2 0x401182b0) compares each item index with `[+0x9c]` to draw the highlight box; item positions come from the item and title vectors, `vertical` (+0xac) and the layout fields (+0xa0, +0xb0, +0xb4), so a start index stored before the first paint (`dk_ui_choice_at`) moves no scroll offset.
