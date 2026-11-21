# LFO shapes (image half)

The image half of the `lfo` service (`services/lfo`): six pointer-slot stubs at the sites where the LFO tick and its second reader
compare the clamped wave with RAND. Without the service the image behaves as stock. See `docs/findings/149-lfo-waveforms.md` and `module.toml`.
