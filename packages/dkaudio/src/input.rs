use cpal::traits::DeviceTrait;
use cpal::{Device, FromSample, Sample, SampleFormat, Stream, SupportedStreamConfig};

/// Called by the audio thread with native interleaved frames. Never wait or do I/O in the callback.
pub trait InputSink: Send + 'static {
    fn write<T: Sample>(&mut self, samples: &[T], channels: usize)
    where
        f32: FromSample<T>;
}

pub fn open_input(
    device: &Device,
    supported: SupportedStreamConfig,
    mut sink: impl InputSink,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Stream, String> {
    let channels = supported.channels() as usize;
    let config = supported.config();
    macro_rules! open {
        ($sample:ty) => {
            device.build_input_stream(
                config,
                move |samples: &[$sample], _| sink.write(samples, channels),
                on_error,
                None,
            )
        };
    }
    match supported.sample_format() {
        SampleFormat::F32 => open!(f32),
        SampleFormat::F64 => open!(f64),
        SampleFormat::I8 => open!(i8),
        SampleFormat::I16 => open!(i16),
        SampleFormat::I24 => open!(cpal::I24),
        SampleFormat::I32 => open!(i32),
        SampleFormat::I64 => open!(i64),
        SampleFormat::U8 => open!(u8),
        SampleFormat::U16 => open!(u16),
        SampleFormat::U24 => open!(cpal::U24),
        SampleFormat::U32 => open!(u32),
        SampleFormat::U64 => open!(u64),
        format => return Err(format!("Unsupported audio sample format: {format}")),
    }
    .map_err(|error| format!("Open audio input: {error}"))
}
