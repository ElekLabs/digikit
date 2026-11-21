# Resonator Bank

Resonator Bank replaces the selected track's filter with eight parallel bandpass filters. On the FLTR page, FREQ moves the center of the bank, SPRD sets the spacing between adjacent bands, and Q sets their resonance. FREQ follows the stock cutoff envelope and modulation. With SPRD at zero, the bands share a center frequency.

The module uses the SDK's resonator bank kernel. It gives each voice its own filter history and copies the incoming 32-sample block so all eight bands process the same input.

To try it on a sounding track in the emulator:

```sh
mise run dk:dev -- emulator --device dt2 --store 'digikit/resonator-bank*'
```

Open FLTR, hold FUNC and press SRC (SYN on Digitone II), then select **Resonator Bank**. The FLTR page exposes FREQ, SPRD, and Q on encoders E, F, and G.
