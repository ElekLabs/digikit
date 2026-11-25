use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, DeviceId, Host, SupportedStreamConfig};

#[derive(serde::Serialize)]
pub struct AudioInput {
    id: String,
    name: String,
}

impl AudioInput {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

pub fn inputs() -> Result<Vec<AudioInput>, String> {
    let host = cpal::default_host();
    let mut candidates = Vec::new();
    for device in host
        .input_devices()
        .map_err(|error| format!("List audio inputs: {error}"))?
    {
        if let (Ok(id), Ok(description)) = (device.id(), device.description()) {
            candidates.push((
                id,
                description.name().to_owned(),
                stereo_config(&device, true, None).is_ok(),
            ));
        }
    }
    let mut inputs = Vec::new();
    for (id, name, usable) in &candidates {
        if *usable
            && candidates
                .iter()
                .filter(|(other, _, _)| other == id)
                .count()
                == 1
        {
            inputs.push(AudioInput {
                id: id.to_string(),
                name: name.clone(),
            });
        }
    }
    inputs.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Ok(inputs)
}

pub fn exact_device(host: &Host, id: &DeviceId) -> Result<Device, String> {
    let mut matches = host
        .devices()
        .map_err(|error| format!("List audio devices: {error}"))?
        .filter(|candidate| candidate.id().ok().as_ref() == Some(id));
    let device = matches.next().ok_or("Audio device disconnected")?;
    if matches.next().is_some() {
        return Err("The audio device ID is ambiguous. Refresh the audio inputs.".into());
    }
    Ok(device)
}

/// Prefer the device's stereo default. When a rate is requested, never resample or fall back to a different rate.
pub fn stereo_config(
    device: &Device,
    input: bool,
    require_rate: Option<u32>,
) -> Result<SupportedStreamConfig, String> {
    let usable =
        |format: cpal::SampleFormat| format.is_int() || format.is_uint() || format.is_float();
    let default = if input {
        device.default_input_config()
    } else {
        device.default_output_config()
    };
    let mut desired_rate = require_rate.unwrap_or(48_000);
    if let Ok(default) = default {
        desired_rate = require_rate.unwrap_or_else(|| default.sample_rate().clamp(1, 768_000));
        if default.channels() >= 2
            && default.sample_rate() == desired_rate
            && usable(default.sample_format())
        {
            return Ok(default);
        }
    }
    let ranges: Vec<_> = if input {
        device.supported_input_configs().map(Iterator::collect)
    } else {
        device.supported_output_configs().map(Iterator::collect)
    }
    .map_err(|error| error.to_string())?;
    ranges
        .into_iter()
        .filter(|r| {
            r.channels() >= 2
                && usable(r.sample_format())
                && r.min_sample_rate() <= 768_000
                && r.max_sample_rate() > 0
        })
        .filter(|r| {
            require_rate
                .is_none_or(|rate| (r.min_sample_rate()..=r.max_sample_rate()).contains(&rate))
        })
        .min_by_key(|r| {
            (
                desired_rate
                    .clamp(r.min_sample_rate(), r.max_sample_rate())
                    .abs_diff(desired_rate),
                r.channels(),
            )
        })
        .map(|r| {
            let rate =
                desired_rate.clamp(r.min_sample_rate().max(1), r.max_sample_rate().min(768_000));
            r.with_sample_rate(rate)
        })
        .ok_or_else(|| {
            require_rate.map_or_else(
                || "The device has no usable stereo PCM format.".into(),
                |rate| format!("The device has no usable stereo PCM format at {rate} Hz."),
            )
        })
}
