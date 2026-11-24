//! The device facts of the effect pages: what the device files' `[fx]` say, generated into `tables.rs` (`patchkit gen`, kind `rust-fx-tables`) and picked by the device id of the core.

/// The facts of one device's effect pages (constants: the kit object's layout, the page numbers, the value indices). Every number is the device file's; the service adds its own
/// constants (row size, list capacity, mark value) and the parameter codes of its rows (`rows_info`) when it binds the table.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DeviceFacts {
    /// `[digikit] id`.
    pub device: u32,
    /// The page number of effect row 0.
    pub page_first: u32,
    /// Effect rows (`[limits] fx_slots`).
    pub slots: u32,
    /// Byte offset of the kit's effect value array in the kit object.
    pub array_off: u32,
    /// Number of value indices.
    pub values_n: u32,
    /// Stock parameter codes whose rows the knobs copy.
    pub template_percent: u32,
    pub template_bipolar: u32,
    pub template_mix: u32,
    /// The address of the firmware's pointer to the live kit object.
    pub kit_ptr: u32,
    /// The array index of the mark.
    pub mark_index: u32,
    /// Send cells (`[limits] send_slots`, 0 on a device without a send page).
    pub send_cells: u32,
    /// The Sound array index of send cell 0 and 1.
    pub send_index: [u32; 2],
    /// The stock send row whose owner and key words a send knob copies.
    pub send_template: u32,
    /// The first Sound object in the kit object, and the bytes between the 16 Sound objects.
    pub sound_off: u32,
    pub sound_stride: u32,
    /// The track FX page number (0: no send page).
    pub track_page: u32,
    /// Byte offset of the last word of the array that stock moves with the storage record.
    pub tail_off: u32,
    /// Byte offset of the spare halfwords in the storage record.
    pub store_off: u32,
    /// The value indices (the first `values_n` are used).
    pub value_indices: [u8; 32],
}

/// The facts of the device with `[digikit] id` `device`, or none (a device without effect pages).
pub fn device_facts(device: u32) -> Option<&'static DeviceFacts> {
    super::tables::DEVICES.iter().find(|f| f.device == device)
}

/// The facts of every device that has effect pages.
pub fn all_device_facts() -> &'static [DeviceFacts] {
    &super::tables::DEVICES
}
