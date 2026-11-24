# Syntakt 1.42 peer sample playback

The opt-in `test/peer-sample` image fixture plays one mono sample from the digikit store using the second ColdFire. The fixture uses the real peer boot stream, frame interrupt, and MAIN stereo mix in the emulator. The normal patch sets continue to send the stock peer program. [Peer CPU](136-syntakt-peer-cpu.md) · [Stock slots](142-syntakt-sample-storage.md)

## Boot and frame path

- **[D]** The fixture reads `samples/peer-proof` on MAIN's boot task at `0x4008287e`, after the stock bank is read and before the peer tasks start. It leases the file during the read and stages at most 4,096 PCM bytes. It accepts native big-endian signed PCM16 mono at 48 kHz, with a 64-byte header and an optional 16-byte trailer.
- **[D]** `core/peer-image` verifies the stock section 7 length and full SHA-256 before offering the staged stream to the optional extender. The stock stream is 383,760 bytes in a 393,211-byte buffer. The fixture validates the stock entry and the operands it will change, appends the renderer and sample, and returns the new length before DMA is armed.
- **[D]** The peer stub runs before the stock entry enables the caches. It copies code, cursor and PCM into the reserved top 8 KiB of peer DDR (`0x47ffe000..0x48000000`). The stock startup instruction at `0x4000107a` initializes SP immediately below the reservation. The section 6 DDR test runs before the program is loaded. Stock sample chunks still travel through window `+0x800` and fill the stock table at `0x4404f754`.
- **[D]** The frame ISR still calls the renderer at `0x4000168e` with its five original arguments. The wrapper then adds up to 32 PCM samples to digital lane 1 of the frame at `0x80003840` before eDMA sends it to MAIN. The additions saturate to Q31. MAIN applies its existing lane gains and mix.
- **[D]** The emulator proof writes a one-shot trigger at shared-window offset `0x600` (MAIN `0x10000600`). The peer consumes it once and begins at sample zero. The renderer then stays idle until the next trigger. This address and value belong to the fixture. Hardware cache behavior and trigger delivery remain open.

## Results

At 48 kHz the peer has 667 µs per 32-sample frame, or 176,000 clocks of a 264 MHz core. These results count emulated instructions. Hardware ISR timing remains **[O]**.

| Check | Syntakt 1.42 emulator result |
| --- | --- |
| Pool output **[D]** | The <code>midi</code> and <code>poke</code> A/B with 64 populated slots isolated a stereo difference of 1,920 samples (40.0 ms). Steady zero crossings were 109 samples apart (440.4 Hz against a 440 Hz source). The strongest whole-window FFT bin was 432 Hz. The emulator resynced 12 times in each capture window. Peak 0.650, RMS 0.433, crest factor 1.50. |
| Stock slots and output **[D]** | The two images produced identical 512-byte stock slot tables, with 64 populated entries, and identical 246,016-byte stock headers and PCM block in peer RAM. The same channel 1 MIDI note produced a stock digital voice on both images. The captures had 0.990 correlation after time alignment, with through-mixer peaks of 0.111 and 0.110. |
| Missing and malformed file **[D]** | An empty card image and a card with a 44.1 kHz native header each sent the stock 383,760-byte section 7 with its original SHA-256. The valid 48 kHz file extended the section to 387,800 bytes. |
| Peer cost **[D]** | The measured idle ISR peak increased from 88,494 to 88,515 instructions. In the paired active and idle runs the same 200 peer interrupts took 23,220 more instructions with the sample triggered. That is 387 extra instructions per rendered frame over 60 frames. |
| Image budget **[D]** | The fixture image was 1,972,384 of 2,096,640 bytes, leaving 124,256 bytes free. The MAIN bootstrap margin stayed at 8.0 KiB. Resident peer code, cursor and the proof sample used 3,988 of the 8,192 reserved bytes. |

**[O]** A bank filled with 64 maximum-length slots causes an early MAIN bus exception in both stock firmware and the patched baseline emulator image. The byte-for-byte comparison above used 64 short, populated slots. Twinshot audition and real-device playback, including the maximum-length bank, remain open.

## Run the emulator proof

Build the stock baseline and the fixture from Syntakt 1.42. Build Patchkit first with `pnpm nx:pilot run patchkit:cli`.

```sh
dist/patchkit/patchkit build core/peer-image test/peer-sample
dist/patchkit/patchkit apply --firmware firmware/Syntakt_OS1.42.syx \
  --modules core/runtime,core/card-guard,core/ui,core/health,core/input,core/peer-image,test/peer-sample \
  --image-only -o out/st-peer-sample.syx
```

Prepare a Syntakt card image with a 40 ms, 440 Hz WAV transferred to the digikit pool as `peer-proof`. The capture uses a MIDI note on channel 1 to give the fixed lane a known routing through the stock mixer. Run the fixture twice, changing only the word written by `poke`. The zero-word run provides the stock track's PCM for subtraction.

```sh
DIGI_CF_IPS=190000000 node tools/nx/rust-env.mjs -- cargo run --release -p elektron-native-boot --example panel_drive -- \
  out/st-peer-sample.syx --after 111M --max 1900M --card-image out/st-card.img \
  --steps 'peerpeek:0x4404f754:512,midi:903c64,wait:18M,peer,pcmreset,poke:0x10000600:44535031,wait:18M,midi:803c00,wait:10M,peek:0x10000600:4,wav:out/st-peer-sample.wav,peer'
```

For the 64-slot case, the card holds a short `SAMP` record in each slot at the stock offsets in [finding 142](142-syntakt-sample-storage.md#1-where-and-how-the-slot-record). The same runner accepts `--card-extent 0x2000:PATH` to overlay that bank on the card. Compare the peer slot table and a stock MIDI note with the baseline image built from the same module list with the fixture omitted. For the missing and invalid file cases, compare the received section 7 digest with the digest in `core/peer-image`.

The paired run used 190,000,000 emulated instructions per second. At the default rate, the runner stopped at 288,999,999 instructions when a frame interrupt changed the PC without advancing its instruction count.

## Next contract

Keep the store path and file lease on MAIN. The production adapter should publish a bounded peer memory region and a versioned command and acknowledgement for a voice, sample generation, trigger and parameters. A voice should refer to a published generation until it retires. MAIN can then release that generation or reuse its memory after acknowledgment. Keep all card I/O out of the peer frame handler. The fixture establishes the boot preload and digital frame path for this design. Shot will need its own peer renderer and a loadable payload contract.
