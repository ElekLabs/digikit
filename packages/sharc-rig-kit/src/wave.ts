/** The mono/stereo formats a render or an external recording can bring to the comparison. */
export interface Wave {
  rate: number;
  channels: number;
  format: 'float32' | 'pcm';
  samples: number[];
}

function waveLayout(bytes: Buffer) {
  if (bytes.toString('latin1', 0, 4) !== 'RIFF' || bytes.toString('latin1', 8, 12) !== 'WAVE')
    throw new Error('Expected a little-endian RIFF/WAVE file.');
  const end = 8 + bytes.readUInt32LE(4);
  if (end > bytes.length || end < 12) throw new Error('Truncated WAV.');
  let fmt: Buffer | undefined;
  let data: Buffer | undefined;
  for (let pos = 12; pos + 8 <= end;) {
    const tag = bytes.toString('latin1', pos, pos + 4);
    const size = bytes.readUInt32LE(pos + 4);
    const next = pos + 8 + size;
    if (next > end) throw new Error(`Truncated WAV ${tag.trim()} chunk.`);
    if (tag === 'fmt ' && !fmt) fmt = bytes.subarray(pos + 8, next);
    if (tag === 'data' && !data) data = bytes.subarray(pos + 8, next);
    pos = next + (size & 1);
  }
  if (!fmt || fmt.length < 16 || !data) throw new Error('WAV is missing a format or data chunk.');

  let tag = fmt.readUInt16LE(0);
  const channels = fmt.readUInt16LE(2);
  const rate = fmt.readUInt32LE(4);
  const align = fmt.readUInt16LE(12);
  const bits = fmt.readUInt16LE(14);
  if (
    tag === 0xfffe &&
    fmt.length >= 40 &&
    fmt.readUInt16LE(16) >= 22 &&
    fmt.subarray(26, 40).toString('hex') === '000000001000800000aa00389b71'
  )
    tag = fmt.readUInt16LE(24);
  if (!((tag === 3 && bits === 32) || (tag === 1 && [8, 16, 24, 32].includes(bits))))
    throw new Error('Supported WAV encodings are float32 and integer PCM (8, 16, 24, 32 bit).');
  if (
    channels < 1 ||
    channels > 2 ||
    rate < 100 ||
    align !== (bits / 8) * channels ||
    data.length === 0 ||
    data.length % align !== 0
  )
    throw new Error('Expected a nonempty mono or stereo WAV with complete frames.');
  return { tag, bits, channels, rate, data };
}

/** Check the format and frame boundaries without decoding samples. */
export function validateWave(bytes: Buffer): void {
  waveLayout(bytes);
}

export function parseWave(bytes: Buffer): Wave {
  const { tag, bits, channels, rate, data } = waveLayout(bytes);
  const scale = 2 ** (bits - 1);
  const stride = bits / 8;
  const samples = Array.from({ length: data.length / stride }, (_, i) => {
    const at = i * stride;
    const v = tag === 3 ? data.readFloatLE(at) : (bits === 8 ? data[at]! - 128 : data.readIntLE(at, stride)) / scale;
    if (!Number.isFinite(v)) throw new Error('WAV contains non-finite samples.');
    return v;
  });
  return { rate, channels, format: tag === 3 ? 'float32' : 'pcm', samples };
}
