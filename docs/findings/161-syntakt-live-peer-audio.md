# Syntakt 1.42 runtime peer audio

The opt-in `core/peer-audio` adapter publishes PCM after boot. Its Rust publisher runs on MAIN's UI task. The peer receives bounded messages in its audio ISR, keeps PCM in private DDR and mixes one voice into the first digital lane. The [boot preload proof](160-syntakt-peer-sample-proof.md) established the underlying image and render hooks.

## Contract

- MAIN takes a store lease while it reads and sends native mono PCM16 at 48 kHz. The peer receives at most 224 sample words per command. There is one command in flight. MAIN writes the request sequence last, and the peer writes the response sequence last.
- The receiver holds two banks of 32,768 mono frames each. It accepts sequential writes to a spare bank and publishes the generation only after all frames arrive. A trigger selects the published generation and sets gain. The previous bank stays live until the peer finishes the active voice. If both banks are in use, MAIN reports busy.
- **[D]** Firmware layout on 1.42. Section 7 is extended after stock verification. The peer stub reserves `0x47fdf000..0x48000000` by lowering the initial stock stack. The receiver occupies the first page and PCM banks start at `0x47fe0000` and `0x47ff0000`. The 500-byte mailbox is at peer `0x600`, which MAIN sees as `0x10000600`. The C frame routine is copied with the wrapper and has position-relative code references within the copied section.

## Checks

- Six Rust unit tests covered chunk geometry, file lease release, the command handshake, stale generation refusal, timeout reporting and late acknowledgment. Two host checks covered the peer's read and write bounds, preservation of a playing bank, and the copied receiver's relocations.
- **[D]** The emulator booted with the versioned receiver and two files in the store. The UI task published generation 1 with 31,200 frames and triggered it. It then published generation 2 with 960 frames in the other bank while generation 1 was still playing. A new upload returned busy at that point. After retirement the active bank returned to zero, and a second trigger played generation 2. The peer reported zero bus faults.
- **[D]** The proof image used 1,979,824 of 2,096,640 container bytes, with an 8.0 KiB MAIN bootstrap margin. The DT2 and DN2 Shot packs matched the baseline byte for byte.

The emulator check uses the `test/peer-audio` fixture and [the adapter's reproduction recipe](../../runtime/platform/peer-audio/README.md). The fixture drives firmware calls from the stock UI task. The recorded stereo output includes the stock MIDI note that opens the lane's mixer path.

## Machine dispatch still open

**[D]** MAIN's engine tick at `0x400a38ce` builds the digital track block at `0x80002000` and sends 0x1b0 bytes to the peer. The peer ISR at `0x40000934` copies eight 0x14-byte track records and reads the event masks at window offsets `0x180..0x183`. Before voice dispatch, `0x40004324` reads each track record's first byte, clamps the stock type to `0..0x2d`, and maps it through the signed-byte table at `0x40014950`. It dispatches mapped types below 12 through the two stock tables at `0x40014920` and `0x400148f0`. On MAIN, stock type bytes are at `0x80002f50`. [Peer audio path](136-syntakt-peer-cpu.md) · [MAIN frame paths](141-syntakt-analog-and-nor-paths.md)

**[O]** The Syntakt image has zero loadable machine rows and the existing DKM DSP section is a SHARC payload. A loadable Shot target needs an adapter for Syntakt machine selection and note dispatch, and a loader contract that can place and retire a peer ColdFire renderer. The checkpoint uses a fixed lane and a test trigger. Hardware validation of peer memory and timing remains open.
