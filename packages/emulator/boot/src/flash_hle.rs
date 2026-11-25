use crate::common::*;
use coldfire::{Bus, Cpu};
use serde::Serialize;

/// The flash sector erase and page program functions of MAIN OS (`erase(addr)`, `program(addr, len, src)`, C calling convention):
/// the first 36 and 28 bytes, the same in DT2 1.16/1.17 and DN2 1.11/1.12. Absolute operands are masked (`data_mask`).
pub(crate) const FLASH_ERASE_SIG: &[u8] = &[
    0x4f, 0xef, 0xff, 0xf4, 0x48, 0xd7, 0x0c, 0x04, 0x4e, 0xba, 0xf3, 0xc2, 0x48, 0x79, 0x44, 0xe6,
    0x66, 0x48, 0x4e, 0xb9, 0x40, 0x00, 0x15, 0xa0, 0x48, 0x78, 0x00, 0x01, 0x4e, 0xb9, 0x40, 0x13,
    0x44, 0xdc,
];

pub(crate) const FLASH_PROGRAM_SIG: &[u8] = &[
    0x4f, 0xef, 0xff, 0xec, 0x48, 0xd7, 0x04, 0x3c, 0x28, 0x2f, 0x00, 0x18, 0x24, 0x2f, 0x00, 0x1c,
    0x2a, 0x2f, 0x00, 0x20, 0x4e, 0xba, 0xf9, 0xc0, 0x4a, 0x80, 0x6c, 0x04,
];

/// The container area of the flash that the OS upgrade rewrites: from the container to the settings record at 0x380000.
pub(crate) const FLASH_CONTAINER_END: usize = 0x38_0000;

/// The erase unit of the model: the writer's default when the (unmodelled) flash probe tells nothing (`DAT_402b67c4`, 0x10000).
pub(crate) const FLASH_ERASE_BYTES: usize = 0x1_0000;

/// What the guest did to the container area of the flash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct FlashWrites {
    pub erases: u64,
    pub programs: u64,
    pub bytes: u64,
}

/// The flash writes of the OS upgrade (finding 75: erase every sector from 0x80000, then program pages from the staged container),
/// done on the model's flash instead of the unmodelled QSPI controller. Only a write inside the container area
/// (`FLASH_SLOT..FLASH_CONTAINER_END`) is taken over: any other (the settings records, the bootstrap) returns `Ok(false)` and runs the
/// firmware's own code as before. Erase sets a sector to 0xFF; program copies the page (a real chip ANDs; the writer erases first).
/// Both return 0 in D0 and pop the return address.
pub(crate) fn hle_flash_write<const TRACE: bool>(
    bus: &mut LoggingBus<TRACE>,
    cpu: &mut Cpu,
    flash: &mut [u8],
    erase: bool,
    stats: &mut FlashWrites,
) -> Result<bool, String> {
    let sp = cpu.a[7];
    let arg = |bus: &mut LoggingBus<TRACE>, n: u32| {
        bus.board
            .read32(sp.wrapping_add(4 * n))
            .map_err(|_| format!("FlashWriteStack({n})"))
    };
    let ret = arg(bus, 0)?;
    let addr = arg(bus, 1)? as usize;
    if erase {
        let end = addr.saturating_add(FLASH_ERASE_BYTES);
        if addr < FLASH_SLOT || end > FLASH_CONTAINER_END {
            return Ok(false);
        }
        flash[addr..end].fill(0xff);
        stats.erases += 1;
    } else {
        let len = arg(bus, 2)? as usize;
        let src = arg(bus, 3)?;
        let end = addr.saturating_add(len);
        if addr < FLASH_SLOT || end > FLASH_CONTAINER_END {
            return Ok(false);
        }
        if !bus.board.ram_peek(src, &mut flash[addr..end]) {
            return Err(format!("FlashProgramSource({src:#010x}+{len})"));
        }
        stats.programs += 1;
        stats.bytes += len as u64;
    }
    cpu.d[0] = 0;
    cpu.a[7] = sp.wrapping_add(4);
    cpu.pc = ret;
    Ok(true)
}
