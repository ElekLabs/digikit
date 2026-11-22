//! Compare the hardware identity behind Windows' separate audio endpoints.

use cpal::DeviceId;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_ContainerId;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToGUID};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, STGM_READ};
use windows::core::{GUID, HSTRING};

pub(super) fn same_container(input: &DeviceId, output: &DeviceId) -> Option<bool> {
    // CPAL initializes COM on this thread when it enumerates WASAPI devices.
    let enumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()? };
    let input = container(&enumerator, input)?;
    let output = container(&enumerator, output)?;
    Some(input == output)
}

fn container(enumerator: &IMMDeviceEnumerator, id: &DeviceId) -> Option<GUID> {
    let id = HSTRING::from(id.id());
    // Read the endpoint property using the exact, opaque WASAPI ID CPAL
    // returned. The property variant must be cleared even if conversion fails.
    unsafe {
        let device = enumerator.GetDevice(&id).ok()?;
        let properties = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = properties.GetValue(&PKEY_Device_ContainerId).ok()?;
        let container = PropVariantToGUID(&value).ok();
        let _ = PropVariantClear(&mut value);
        container.filter(|id| *id != GUID::zeroed())
    }
}
