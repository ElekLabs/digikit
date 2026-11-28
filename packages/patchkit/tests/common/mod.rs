//! A flat RAM for the tests that run committed objects on the ColdFire core (`coldfire::Cpu`).
use coldfire::{Bus, BusError};

pub struct Ram(pub Vec<u8>);

impl Bus for Ram {
    fn read8(&mut self, addr: u32) -> Result<u8, BusError> {
        self.0
            .get(addr as usize)
            .copied()
            .ok_or(BusError { addr, write: false })
    }
    fn read16(&mut self, addr: u32) -> Result<u16, BusError> {
        Ok(u16::from_be_bytes([
            self.read8(addr)?,
            self.read8(addr + 1)?,
        ]))
    }
    fn read32(&mut self, addr: u32) -> Result<u32, BusError> {
        Ok(u32::from_be_bytes([
            self.read8(addr)?,
            self.read8(addr + 1)?,
            self.read8(addr + 2)?,
            self.read8(addr + 3)?,
        ]))
    }
    fn write8(&mut self, addr: u32, value: u8) -> Result<(), BusError> {
        *self
            .0
            .get_mut(addr as usize)
            .ok_or(BusError { addr, write: true })? = value;
        Ok(())
    }
    fn write16(&mut self, addr: u32, value: u16) -> Result<(), BusError> {
        for (i, b) in value.to_be_bytes().iter().enumerate() {
            self.write8(addr + i as u32, *b)?;
        }
        Ok(())
    }
    fn write32(&mut self, addr: u32, value: u32) -> Result<(), BusError> {
        for (i, b) in value.to_be_bytes().iter().enumerate() {
            self.write8(addr + i as u32, *b)?;
        }
        Ok(())
    }
}
