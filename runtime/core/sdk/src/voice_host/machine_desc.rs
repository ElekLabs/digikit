/// `dk_machine_desc` of `sdk/include/dk/gen/machine.h`: the facts a machine module states about its voices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct MachineDesc {
    magic: u32,
    contract: u32,
    n_params: u32,
    voices: u32,
    state_bytes: u32,
    state_clear_bytes: u32,
    max_call_depth: u32,
    stack_bytes: u32,
}

impl MachineDesc {
    /// `DK_MAGIC` of `machine_consts.h`.
    const MAGIC: u32 = 0x444b_4d41;
    /// `DK_MACHINE_V1`.
    const CONTRACT_V1: u32 = 1;

    pub fn new(
        n_params: u32,
        voices: u32,
        state_bytes: u32,
        state_clear_bytes: u32,
        max_call_depth: u32,
        stack_bytes: u32,
    ) -> Self {
        Self {
            magic: Self::MAGIC,
            contract: Self::CONTRACT_V1,
            n_params,
            voices,
            state_bytes,
            state_clear_bytes,
            max_call_depth,
            stack_bytes,
        }
    }

    /// Voices the module supports.
    pub fn voices(&self) -> u32 {
        self.voices
    }
}
