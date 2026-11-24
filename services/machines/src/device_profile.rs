//! The profile of the device the runtime runs on.

/// The profile of the device the runtime runs on, by the device id (the generated table `dkrt_abi::profiles`): the run-time machine types of the device.
/// `None` for an id without a profile.
pub(crate) fn profile<C: dkrt_abi::core_api::CoreApi>(
    core: &C,
) -> Option<dkrt_abi::platform::DeviceProfile> {
    dkrt_abi::platform::DeviceProfile::by_id(core.device_id())
}
