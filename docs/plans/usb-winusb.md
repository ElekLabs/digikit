# WinUSB for the Fast USB bulk interface

Status: **built; emulator and hardware checks open** (PR #583). Follows [usb-fast-audio-bulk.md](usb-fast-audio-bulk.md). USB facts: [finding 42](../findings/42-usb-transfer-routes.md).

## Goal

Windows binds WinUSB to the Fast USB bulk interface (interface 5, EP2 OUT, vendor class 0xFF) that `core/usb-bulk-audio` adds to the USB AUDIO/MIDI configuration (port 5). No manual driver install. The desktop app (nusb) and Chrome WebUSB on Windows can then use Fast USB. Today Windows stays on SysEx.

## Where

Inside `core/usb-bulk-audio`. It owns interface 5 and its attached state. Not a new module, and not usb-fast: a card module loads after USB enumeration, and re-enumeration (take-over) was removed.

## Mechanism

A `before` hook (`cf.usb.hook.setup_dispatch`) on the entry of the EP0 setup dispatcher `FUN_40005ff6` (0x40005ff6 on DT2 1.17 and DN2 1.12). The one caller, the USB interrupt (`jsr (d16,PC)` at 0x40006ff4), cannot reach module code, so the hook sits on the dispatcher's first instruction instead: `movea.w (abs).l,A0`, 6 bytes, position-independent, and no branch in the function targets its middle. The stub `ua_setup_hook` calls `ua_setup_handle`; on 1 (answered) it returns to the interrupt, on 0 it jumps to the generated tail, which runs the displaced instruction and continues in the stock dispatcher.

The handler reads the 8-byte setup packet the ISR stored (anchor `cf.usb.data.setup`; DT2 0x47db4198, DN2 0x4664bf10; wire order). It acts only when the active port is 5 (`cf.usb.value.active_port == 5`) and interface 5 is attached. Then it answers three requests itself through the firmware EP0 send (`cf.usb.fn.ep0_send`, `FUN_40005f30`; it sends min(len, wLength) with data and status stages) and returns 1:

1. GET_DESCRIPTOR device: port 5's own descriptor, read from the port table entry (VID 0x1935, PID 0x102b on DT2, 0x1034 on DN2). bcdUSB is 0x0201. bcdDevice is 0x0101 (was 0x0001). A new bcdDevice gives Windows a fresh cache key.
2. GET_DESCRIPTOR BOS (type 0x0F).
3. Vendor IN request: bmRequestType 0xC0, bRequest 0x44, wIndex 7. The answer is the MS OS 2.0 descriptor set.

Every other request, and every request in another USB mode, goes to the stock dispatcher (`cf.usb.fn.setup_dispatch`) unchanged.

Stock behaviour to record: BOS gets no answer at all (no data, no stall; the host times out). Unknown vendor requests stall. Free 0xC0 vendor codes are 3 and 6..0xFF.

EP0 send copies: `FUN_40005f30` returns at once when len > 0x1000, else `FUN_40005be2` copies the bytes with the CPU into one of four EP0 pages in the uncached DDR alias (DT2 0x4fe53000 + slot * 0x1000) and primes a data dTD and a status dTD. The controller never reads the caller's buffer, so the handler's RAM copy of the device descriptor needs no cache push. Stall: the stock firmware sets ENDPTCTRL0 TXS (bit 16) with `bset.l #0x10` in the bad-request path, or TXS and RXS with `ori.l #0x10001`; a SETUP clears both. Data cache: ACR0 makes 0x40000000..0x47FFFFFF copyback; the alias from 0x48000000 is cache-inhibited (CACR default). All [D], static read of DT2 1.17 and DN2 1.12.

## Bytes

Source: Microsoft OS 2.0 Descriptors Specification, July 2018 (local copy `out/refs/ms-os-2.0/`), cross-checked with the TinyUSB `webusb_serial` example.

Constants: vendor code 0x44. DeviceInterfaceGUID `{7F45739F-0563-45C1-823A-43A14AD7F745}` (digikit's own; never reuse the GUID of another project).

BOS, 33 bytes = 5 (header) + 28 (platform capability):

- Header: `05 0F 21 00 01`
- Platform capability: `1C 10 05 00`, UUID `DF 60 DD D8 89 45 C7 4C 9C D2 65 9D 9E 64 8A 9F`, dwWindowsVersion `00 00 03 06`, wMSOSDescriptorSetTotalLength `B2 00`, bMS_VendorCode `44`, bAltEnumCode `00`.

Descriptor set, 178 bytes = 10 + 8 + 8 + 20 + 132:

| Part | Bytes | Content |
|---|---|---|
| Set header | 10 | `0A 00`, `00 00`, `00 00 03 06`, `B2 00` |
| Configuration subset | 8 | `08 00`, `01 00`, bConfigurationValue `00`, `00`, `A8 00` |
| Function subset | 8 | `08 00`, `02 00`, bFirstInterface `05`, `00`, `A0 00` |
| Compatible ID | 20 | `14 00`, `03 00`, "WINUSB\0\0", 8 zero bytes |
| Registry property | 132 | `84 00`, `04 00`, data type `07 00` (REG_MULTI_SZ), name length `2A 00`, name, data length `50 00`, data |

bConfigurationValue `00` is the configuration index for Windows. TinyUSB also uses 0.

Arithmetic:

- Name "DeviceInterfaceGUIDs": 20 characters x 2 = 40, plus UTF-16 NUL 2 = 42 (`2A`).
- Data: the GUID string has 38 characters x 2 = 76, plus NUL 2, plus the list-ending NUL 2 = 80 (`50`).
- Registry property: 2 (length) + 2 (type) + 2 (data type) + 2 (name length) + 42 + 2 (data length) + 80 = 132 (`84`).
- Function subset: 8 + 20 + 132 = 160 (`A0`).
- Configuration subset: 8 + 160 = 168 (`A8`).
- Set: 10 + 168 = 178 (`B2`).

## Proof

Emulator device tests on DT2 and DN2 through a new `usb.ctrl` control RPC. It writes the setup packet, runs the dispatch call at interrupt level and captures what `ep0_send` sends. Tests:

- exact bytes of the three answers;
- truncation for a short wLength;
- unknown vendor requests still stall;
- USB MIDI mode unchanged.

Hardware, macOS, both devices:

- `dkbulk descriptors` reads and checks the bytes;
- USB audio and MIDI still work;
- Fast USB still works.

macOS also requests BOS once bcdUSB is 0x0201, so the Mac check exercises the new path.

## Windows checklist

For someone with Windows 10 or 11. Not testable here.

1. Plug in with USB CONFIG = USB AUDIO/MIDI.
2. Device Manager shows interface 5 of the device as a WinUSB device (Universal Serial Bus devices).
3. The workbench shows FAST USB on Samples.
4. The Chrome WebUSB site path works.

If the device was plugged into that PC with the old firmware, the new bcdDevice gives a fresh key. If it still does not bind, delete `HKLM\SYSTEM\CurrentControlSet\Control\UsbFlags\<VVVVPPPPRRRR>` and replug.

## Risk and gate

The hook runs on every EP0 setup in every USB mode. A bug can break enumeration on all operating systems. The per-request gate keeps other modes stock. Merge only after the emulator tests and the macOS hardware check on both devices. The Windows result follows later. If WinUSB does not bind, behaviour stays as today (SysEx).

## Next

1. Windows confirmation.
2. Open [O]: the optional vendor revision descriptor (spec Table 19). The spec requires it "in every function subset" for composite devices. Verify on Windows whether it is needed. Adding it makes the lengths 166 (function subset), 174 (configuration subset) and 184 (set).
