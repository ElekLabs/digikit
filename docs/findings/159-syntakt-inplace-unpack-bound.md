# Syntakt flash image: the in-place unpack bound, not packing waste

Static read of the Syntakt 1.42 bootstrap and patchkit's packer (nothing run but `patchkit size`). Marks: **[D]** read once, **[O]** open. It corrects the reading of finding 147 that the stored literal tail of MAIN is a packing loss: it is the cheapest way to meet a hard bound, so the Syntakt fit depends one-to-one on the unpacked size of MAIN. The plan handles the Syntakt fit in item RC2s ([runtime contract plan](../plans/runtime-contract-and-lifecycle.md)); lever A below is a fallback.


## 1. Cause [D]
- Bootstrap (finding 152): stages packed MAIN at 0x40200000 (devices/syntakt.toml main_stage), unpacks in place to 0x40000400 (main_dest), byte-at-a-time, no bounds check.
  Room before the output reaches the stream start: 0x200000-0x400 = 2,096,128 (DT2/DN2 stage 0x40400000: 4 MB, so no issue there).
- patchkit: aplib.rs:477 inplace_margin = min over tokens of (stream bytes read) - (output written past `room`). apply.rs:2445 calls
  pack_section_in_place (aplib.rs:374) with floor main_margin_min = 8192 (devdata.rs:446). If plain stream is below the floor it binary-searches the
  shortest literal tail (emit(), aplib.rs:296) that restores the floor.
- Slack falls whenever the stream expands (ratio<1) and only literals keep it constant. So the binding point is the END of the stream:
  stream_len >= out_len - 2,096,128 + floor (+8 header). Check with finding 152 numbers: out 3,683,064, plain 1,544,457, margin -42,486 -> -42,471 by formula.
- Consequence [D, derived]: the stored MAIN is bounded below by out_len - 2,096,128 + floor, whatever the packer does and wherever the literals sit.
  Literalizing an output byte costs (1-c) extra and buys (1-c) slack (c = local stored bytes per output byte): cost == gain exactly. That is why
  "tail 111,517 costs 69,762": the plain stream is 69,762 short of the bound; the tail is the cheapest way to pad, not a waste. Reordering, splitting, packer
  settings, choice of which bytes go last cannot beat the bound (they only choose where the padding sits). Section layout does not matter either.
- Small margin on Syntakt = small room (stage only 2 MB above dest); MAIN is 3.44 MB stock + appended runtime chain (non-bss 274,498 now; was 215,583).
  After MAIN's end: the output end lands about floor bytes inside the stream end; the OS-upgrade stage (0x403d10d0) is above and unrelated at boot.

## 2. Options (all keep the bootstrap/unpacker stock)
A. Lower main_margin_min (devices/syntakt.toml [patch], device data, no code). Stored MAIN shrinks 1 byte per byte of floor. The depacker reads on demand, so
   the true need is slack >= 0 after each token (writes of a token happen before the next read); 8192 is a safety default, not measured. Floor 64: -8,128 bytes.
   (Section 4 of 152: depack_check runs the real bootstrap depacker on the native core and verifies length+SHA: that proves the floor statically.)
B. Reduce out_len (unpacked MAIN). While the bound binds each raw byte removed saves one STORED byte (not 0.54): runtime feature cut for dsp=none
   (about 33 KB raw [O], finding 147 sec 4), drop dead appended bytes/alignment/padding in the segment chain, smaller digikit-menu/ui. Best value per byte.
C. Put the runtime object outside MAIN's in-place stream: own container section, packed (0.54): 274,498 raw -> ~148 KB stored, bound drops 274 KB:
   net about -126 KB. Needs the installer (core/expansion, runs after bootstrap) to read it from NOR and depack (stock depacker address or own); NOR is likely
   not memory-mapped, section ids unknown to bootstrap, OS-upgrade receiver stores whatever L covers [O]. Largest, riskiest, days.
D. Section 8 (103 KB stored, use unknown) or section 4 repack: read-only for patchkit, unknown consumers [O]. Not recommended.
E. Not helpful: reordering cf.expansion, splitting the stream, packer depth/nice, order of bytes (bound is length-only); caves (2 KB).

## 3. Best near-term: A + B
- Over now (caller): 5,888. A alone (floor 8192 -> 64): -8,128 => about 2,240 under container_max (2,096,640). Thin but fits.
- A + 33 KB feature cut (B): about 35 KB under.
- Verify statically: scratch copy of devices/syntakt.toml (and the port copy it reads) with main_margin_min = 64, `patchkit apply` the device set to a scratch file,
  check size (patchkit size prints the margin line; verify.rs:115 guard uses the same floor), then run packages/emulator/boot/examples/depack_check.rs
  with --stage 0x40200000 --base 0x800003ec on the resulting MAIN stream: length and SHA-256 must equal the patched MAIN. For floor 0..64 also check
  inplace_margin of the final stream equals the floor. No device needed for the proof of the unpack; hardware boot stays [O].

## 4. Risks
- Floor too low: the bootstrap hangs before MAIN (finding 152): unit stays at the logo, recovery needs the startup key [O] (finding 146 sec 6). Mitigate: depack_check on the real depacker with the exact stream; keep >= a few hundred bytes of slack rather than 0.
- The depacker's byte-at-a-time read is [D] (one read, 152 item 2), second-agent check of 0x8000041c wanted before [V]; a prefetch of a word would need floor >= 4.
- Container limit: OS-upgrade stage 2 MiB, container_max 0x1FFE00 (146 sec 4); A/B only shrink the container, no new receiver risk. C adds a section the receiver
  accepts as bytes but a bootstrap table walk of unknown ids is unread [O].
- The lane's own numbers moved (size shows 2,149,568 / 52,928 over with card-guard in the set; caller figure 5,888 over): recompute the bound with the final runtime.
