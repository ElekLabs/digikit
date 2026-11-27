# 144 Stock device identity over MidiRpc

Which stock MidiRpc request names the device without the digikit runtime. Images: dt2-1.17, dn2-1.12, st-1.42. Marks: [D] read once, [O] open.

## Result

**Ping (type 0x01) names the device.** Its reply carries the model name string, and the string equals `[device] name` of `devices/*.toml`: `Digitakt II`, `Digitone II`, `Syntakt`. Every firmware answers it (the dispatcher builds it in its first branch). **[D]**
SoftwareVersion (type 0x02) carries the OS version but no product. Together they give the model and the OS release. No reply carries `[device] syx_id`. **[D]**

| Image | Dispatcher | Ping name literal | Ping branch constants |
| --- | --- | --- | --- |
| dt2-1.17 | `FUN_401246a6` | `s_Digitakt_II_40241393`, pushed at 0x401247a8 | blob size 0x2c (44 commands), byte 0x2a after the strings |
| dn2-1.12 | `FUN_40125f9e` | `s_Digitone_II_4021a78a`, pushed at 0x401260a0 | blob size 0x16 (22 commands), byte 0x2b |
| st-1.42 | `FUN_40108fbe` | `s_Syntakt_402511f5`, pushed at 0x401090c0 | blob size 0x16 (22 commands), byte 0x1e |

The name literal has no other use in the Ping branch. The same dispatcher also builds the SoftwareVersion branch (DT2: after 0x40124968, response constructor `Sp_counted_ptr_inplace<MidiRpcSoftwareVersionResponse>`) and the DeviceUID branch (DT2: 0x40124b3c, `MidiRpcDeviceUIDResponse::ctor_dtor` 0x401d464c). **[D]**

## Ping reply layout

Payload after the type byte 0x81: `u8 blobLen`, the blob of advertised request types (DT2 1.16: 44 commands, finding 25 and MIDI-SYSEX-RPC.md section "Device-advertised command set" **[V]**), then NUL-terminated strings. The first string after the blob is the model name. The DT2 1.16 emulator reply is 66 payload bytes and DN2 1.11 is 41 (finding 25). **[D]**
The branch builds a second string after the name: DT2 from the object at 0x4099d5a4 (`FUN_400c1160`), DN2 at 0x405ce878 (`FUN_400bb790`), Syntakt at 0x40653420 (`FUN_4006f264`). Its content is not read (hardware revision or serial?). The byte 0x2a, 0x2b or 0x1e that follows is not decoded. **[O]**
The blob sizes 44, 22 and 22 fit the advertised sets: the DN2 has no StorageSpace handler (finding 25) and the Syntakt has no FsSample handlers (finding 142). Not checked against a DN2 1.12 or Syntakt reply **[O]**: the emulator has no recorded Ping reply for those two.
A client must not depend on the blob or the trailing bytes: read `blobLen`, skip the blob, take the first NUL-terminated string. **[C]** A real Syntakt 1.42 reply (emulator, 2026-11-19) is length-prefixed: `u8 rest` in front of the plain layout, `rest` being the number of bytes after the `blobLen` byte. The payload is `1e 16 <22 request types> "Syntakt" 00` (32 bytes, `1e` = 22 + 8). A reader tells the layouts apart by the framing (the first byte is the payload length minus two) and reads the name as the first NUL-terminated string after the blob (`parsePing`, `flash_rpc::parse_ping`; vectors in `tests/fixtures/ping-vectors.json`). The layout of the DT2 and DN2 replies is not rechecked **[O]**.

## SoftwareVersion

Request has no payload. Reply: NUL-terminated strings `"00"`, `"79"`, `"1.16"` on DT2 1.16 (finding 25: 12 payload bytes on DT2 1.16 and DN2 1.11). The first two are the build string `0079` split in two; the build string is at ELE3 `+0x08` (finding 01); the third is the OS version. The version record behind it is the 32-byte ELE3 header read from flash at 0x80000 (`FUN_4013641c` on DT2: magic `ELE3`, word at +4 equal to 0x2b); `FUN_4013641c` only checks the magic and the 0x2b. **[D]**
`parseSoftwareVersion` (packages/workbench-core/src/transfer/connection-core.ts) already takes the OS version from it. The OS version alone is not a product id: DT2 1.17 and DN2 1.12 differ today, but a later DN2 may use 1.17.

## Other candidates

- **DeviceUID (type 0x03):** one big-endian u32 (5 bytes on the wire after 7-bit packing), from `FUN_4012ddea` through `FUN_40123d1c` on DT2. It is not a product id: the source is not decoded **[O]**, but the reply is the same size for DT2 and DN2.
- **Product code in the ELE3 header +4:** DT2 0x2b and DN2 0x34 (finding 01); the Syntakt value is not read **[O]**. It is flash content, not in any RPC reply.
- **SysEx device inquiry** `F0 7E 7F 06 01 F7`: the firmware routes it into the Sample Dump state machine and answers `F0 7E 00 7C 00 F7` (WAIT), with no identity (MIDI-SYSEX-RPC.md section 1). **[D]**
- **`syx_id`** (0x14, 0x15, 0x16): the device byte of the OS-upgrade framing (finding 01). The RPC route uses device byte 0x10 for DT2 and DN2 (MIDI-SYSEX-RPC.md section 4); the descriptor table at 0x402b4098 lists it. The Syntakt device table is not read **[O]**. No MidiRpc reply returns `syx_id`, so the registry cannot be matched by it from RPC.
- **Query (type 0x09):** a feature-key string request (`sample_file.interleaved_stereo_support`, `sample_file.mono_support`, `sample_file.sample_rates`); no key names the product. **[D]**
- **USB product string and id:** not examined. The MIDI port name carries the model ("Elektron Digitakt II"); `deviceOfPort` in packages/workbench-core/src/data/device-profiles.ts already maps it to a device id. **[O]** for the USB descriptor itself.

## Existing code

- packages/dk-session/src/identify.ts holds `RPC_PING`, `RPC_SOFTWARE_VERSION`, `parsePing`, `parseSoftwareVersion` and `identifyDevice`. connection-core.ts (packages/workbench-core/src/transfer) re-exports them.
- `identifyDevice` sends Ping first and takes the model name from the reply. It matches the name with the registry. Then it sends SoftwareVersion for the OS version. Then it sends one INFO with a 2.5 s timeout and one retry; E_BUSY is retried. Only a lost INFO reply means "no runtime".
- When Ping is silent, the id comes from the `port` field of the INFO reply. When Ping is silent and INFO is lost too, `identifyDevice` throws.
- The [desktop Samples session](../../apps/desktop/ui/src/samples/device-session.ts) and tools/devtools/wb.mjs identify devices with `identifyDevice`.
- `askSession` (firmware-ask.ts) and `osVersion` / `probeInfo` (transfer-backend.ts) use `parseSoftwareVersion` for the OS only. The Firmware page's `askDevice` returns `model` only from INFO (`info.port`), so it names no stock device.
- `RpcPort.rpc(type, payload, timeoutMs)` carries the Ping request unchanged.
