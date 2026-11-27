/// The mailbox the UI-task hook polls. A host writes `arg`, then `cmd`; the runtime runs it, stores `result`, adds one
/// to `seq` and zeroes `cmd`.
#[repr(C)]
pub struct Mailbox {
    pub cmd: u32,
    pub seq: u32,
    pub result: i32,
    pub arg: [u8; 96],
}
