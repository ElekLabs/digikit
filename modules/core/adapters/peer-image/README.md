# core/peer-image

The Syntakt has a second ColdFire CPU. Its program is section 7 of the OS container (383,760 bytes), stored raw by Elektron. MAIN reads it from the NOR flash into a
transmit buffer and streams the buffer to the second CPU byte for byte. With the section raw, the patched image is 32,672 bytes over the 2 MiB stage of the OS upgrade.
This adapter lets patchkit store section 7 packed (264,194 bytes, the image is then about 85 KB under the limit) and unpacks it in MAIN before the stream starts.
Runs on the hardware: the `device-peer` image and the full `device` image boot on a unit, and the second CPU loads the unpacked section. Facts and the address evidence: [finding 151](../../../../docs/findings/151-syntakt-section7-pack.md).

- **Hook.** The peer thread loads A2 with the NOR read routine (`lea`, 6 bytes, anchor `cf.peer.site.nor_read_lea`) and calls `jsr (a2)` for section 6 and section 7. A `hook`
  op (mode `replace`) loads A2 with `peer_read` instead. Same stack signature `[ret][src][len][dst]`.
- **Section 6** (`dst != 0x4f84f2c6`): the stock NOR read, unchanged.
- **Section 7** (`dst == 0x4f84f2c6`): read the packed stream to the end of the buffer (`0x4f8af2c1 - length`), check the header (length, byte sum), unpack it forward with the
  firmware's depacker (`cf.peer.fn.depack`), check the output length and its SHA-256 against the stock section, store the unpacked length big endian at 0x4f84f2c2.
- **Failure.** Any failed check pops the return address and the three arguments and jumps to the thread's own path for a missing section (`cf.peer.site.status_fail`): status 1,
  the DMA is never armed, the second CPU gets no bytes, MAIN shows its boot failure screen ("OOPS! SOMETHING WENT WRONG"). A wrong image never reaches the second CPU, which loads its own FPGA bitstream from it.
- **patchkit.** `[[repack]]` in `module.toml` names the buffer (`dest`, `stage_end`) and the stock section (`len`, `sha256`). patchkit stores the section packed only when this
  module is in the set, checks the stock section against `len` and `sha256`, and refuses a layout whose in-place margin is 0 or less (9,444 bytes now). The same constants
  are in `src/cf/peer.c`; `packages/patchkit/tests/repack.rs` checks that both name the same values.
- **Cost.** About 0.25 to 0.4 s more boot time (unpack, byte sum, rolled SHA-256 over 383,760 bytes through the uncached alias), before the handshake timeout starts **[O]**.
  Code and tables: about 1.4 KB in cf.expansion.
- **Devices.** Syntakt 1.42 only (`ports = ["st-1.42"]`). The device file allows it with `[container] repack = ["cf2"]`.

Boot it in the emulator (not run when this was written): `panel_drive` (header of `packages/emulator/boot/examples/panel_drive.rs`) boots a patched image headless and its
`peer` step reports the bytes the second CPU took and the SHA-256 of each image; that of image 7 must be the stock section 7 hash in `module.toml`. Read the status word
0x41bbf448 (6 = OK) with `peek`:

```text
node tools/nx/rust-env.mjs -- cargo run --release -p elektron-native-boot --example panel_drive -- OUT/st-peer.syx \
  --base firmware/Syntakt_OS1.42.syx --after 130M --steps wait:1M,peek:0x41bbf448:4,peek:0x41bb32b0:4,peer
```
