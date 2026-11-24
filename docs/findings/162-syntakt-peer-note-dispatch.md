# Syntakt 1.42 peer note dispatch

The opt-in peer adapter can bind its published sample to a digital track. Binding is volatile. The stock machine still runs, and MAIN receives the peer voice in place of that track's stock audio. The stock kit, project and machine type bytes keep their original representation. [Runtime publication](161-syntakt-live-peer-audio.md)

## Note path

- **[D]** A normal channel 1 MIDI Note On produced a one-frame bit at the peer's copied parameter byte `0x80008180`. At the final renderer call in `0x40000934`, the third argument is that byte with the mask at `0x80008182` cleared. The firmware uses the same mask earlier in the frame at `0x40004324`. The wrapper passes that argument to the peer adapter. The adapter uses the selected track bit to start the current published generation at frame zero. There are eight digital lanes of 32 samples.
- **[D]** The version 2 mailbox adds track selection and a count of starts caused by stock notes. Selecting a different track or restoring stock retires the active voice. A note on an unbound track leaves its stock audio in place. With a track selected and no sample playing, that track's output is silent. The compatibility test trigger is available while the adapter is unbound.
- **[D]** The Syntakt emulator loaded a 960-frame, 660 Hz sample and bound track 2. A channel 1 note produced stock output with zero peer sample starts. Two channel 2 notes advanced the start count to 1 and 2. The sampled output had a 661 Hz spectral peak in its main tone window. Releasing the binding restored track 2's stock audio, and the next channel 2 note left the sample start count at 2. The peer reported zero bus faults.

## Reproduce

Follow the [adapter build recipe](../../runtime/platform/peer-audio/README.md) with `samples/next` on the card. Use the same emulator options and substitute these steps.

```sh
--steps "peer,send:5073616d706c65732f6e65787400,wait:100M,send:53,peek:$result:4,send:4102,peek:$result:4,wait:3M,peek:0x100007f4:8,pcmreset,midi:903c64,wait:16M,peek:0x100007f4:8,wav:out/st-unbound-track.wav,midi:803c00,wait:12M,pcmreset,midi:913c64,wait:18M,peek:0x100007f4:8,wav:out/st-bound-track.wav,midi:913e64,wait:10M,peek:0x100007f4:8,midi:813c00,midi:813e00,send:4100,wait:3M,peek:0x100007f4:8,pcmreset,midi:913c64,wait:18M,peek:0x100007f4:8,wav:out/st-restored-track.wav,peer"
```

The two result words at `0x100007f4` are binding and stock-note starts, in that order. The expected pairs are `2,0` after binding and after the channel 1 note, `2,1` then `2,2` for the two channel 2 notes, and `0,2` after restoring stock and playing again.

## Native selection

**[D]** MAIN opens a `MachineListView` via `0x40030f06`. Its constructor at `0x4002f868` builds stock machine rows and formats names using `0x400a6704`, which bounds the type to less than `0x2d`. The selection routine at `0x4002f2ae` only selects a stock type within that range. The confirm callback at `0x40031242` applies a normal selection through `0x400265c4` and emits a kit change. The peer also clamps stock types before its dispatch lookup.

**[O]** The track assignment in this proof comes from `test/peer-audio`. Native selection needs a separate module assignment path alongside these stock checks. Verify that it previews, confirms, cancels and clears the assignment correctly and preserves a valid stock fallback when a module unloads, the kit changes, or the device restarts. Note pitch and velocity delivery to a loadable Shot payload and physical playback remain open.
