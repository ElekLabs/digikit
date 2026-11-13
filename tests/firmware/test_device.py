# pyright: reportMissingImports=false
"""Device files: identity by hash, and the wire mapping.

`tools/firmware/config.py` settles a tie between firmware files by hardcoded FILENAME,
which is how one product silently runs under another's name. A device file
keys on the firmware's SHA-256 instead. These tests cover that, the
(channel, bit) <-> control-code mapping including each product's non-linear
channel 6, and that the real files in devices/ parse and round-trip.

No firmware and no emulator: the shipped device files are plain data, and
everything else is built in a temp directory.
"""

import os
import tempfile
import unittest

from support.paths import REPO_ROOT

from tools.firmware import device

REPO = str(REPO_ROOT)
DEVICES = os.path.join(REPO, "devices")

SYNTHETIC = """
[device]
name = "Test Device"
short = "td"

[[firmware]]
version = "9.9Z"
sha256 = "AABBCC"
filename = "Test_OS9.9Z.syx"

[panel]
linear_channels = 2
encoders = 3

[panel.exceptions]
20 = [6, 1]

[[panel.group]]
name = "keys"
kind = "button"
codes = [1, 2]
layout = "row"

[[panel.group]]
name = "odd"
kind = "button"
codes = [20]
layout = "single"

[[panel.group]]
name = "encoders"
kind = "encoder"
codes = [1, 2, 3]
layout = "row"
"""


def write_device(dirpath, text=SYNTHETIC, name="test.toml"):
    path = os.path.join(dirpath, name)
    with open(path, "w") as fh:
        fh.write(text)
    return path


class ParseTest(unittest.TestCase):
    def test_parses_identity_and_panel(self):
        with tempfile.TemporaryDirectory() as d:
            dev = device.load(write_device(d))
        self.assertEqual(dev.name, "Test Device")
        self.assertEqual(dev.short, "td")
        self.assertEqual(dev.linear_channels, 2)
        self.assertEqual(dev.encoders, 3)
        self.assertEqual(len(dev.groups), 3)

    def test_sha256_is_lowercased(self):
        # Hashes get pasted in both cases; comparison must not care.
        with tempfile.TemporaryDirectory() as d:
            dev = device.load(write_device(d))
        self.assertEqual(dev.firmwares[0].sha256, "aabbcc")
        self.assertIsNotNone(dev.firmware_for_sha256("AaBbCc"))

    def test_exception_codes_are_integers(self):
        # TOML bare keys are strings even when they look like integers, so
        # '20' must arrive as 20 or every lookup silently misses.
        with tempfile.TemporaryDirectory() as d:
            dev = device.load(write_device(d))
        self.assertEqual(dev.exceptions[20], (6, 1))

    def test_missing_section_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            path = write_device(d, "[panel]\nencoders = 1\n")
            with self.assertRaises(device.DeviceError):
                device.load(path)

    def test_identity_only_file_has_no_panel(self):
        with tempfile.TemporaryDirectory() as d:
            path = write_device(d, '[device]\nname = "X"\nshort = "x"\n')
            dev = device.load(path)
        self.assertFalse(dev.has_panel)
        self.assertEqual(dev.capabilities, {})

    def test_capabilities_and_acceptance_are_exposed_and_looked_up_by_short(self):
        with tempfile.TemporaryDirectory() as d:
            write_device(
                d,
                SYNTHETIC
                + '\n[capabilities]\ndsp = "sharc"\n\n[acceptance]\nfirmware = "9.9Z"\n',
            )
            dev = device.by_short("td", d)
            self.assertEqual(dev.capabilities["dsp"], "sharc")
            self.assertEqual(dev.acceptance["firmware"], "9.9Z")
            self.assertTrue(dev.has_panel)
            with self.assertRaises(device.DeviceError) as e:
                device.by_short("nope", d)
            self.assertIn("td", str(e.exception))


class WireMappingTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dev = device.load(write_device(self.tmp.name))

    def tearDown(self):
        self.tmp.cleanup()

    def test_linear_region(self):
        self.assertEqual(self.dev.wire_for(1), (0, 0))
        self.assertEqual(self.dev.wire_for(9), (1, 0))
        self.assertEqual(self.dev.wire_for(16), (1, 7))

    def test_exception_wins_over_the_formula(self):
        self.assertEqual(self.dev.wire_for(20), (6, 1))
        self.assertEqual(self.dev.code_at(6, 1), 20)

    def test_code_beyond_this_product_has_no_wire_position(self):
        # Not an error: one product simply carries fewer controls.
        self.assertIsNone(self.dev.wire_for(40))

    def test_encoder_codes_are_a_separate_space(self):
        self.assertEqual(self.dev.encoder_channel(1), 0)
        self.assertEqual(self.dev.encoder_channel(3), 2)
        self.assertIsNone(self.dev.encoder_channel(4))


class IdentifyTest(unittest.TestCase):
    def test_unknown_hash_is_refused_not_guessed(self):
        with tempfile.TemporaryDirectory() as d:
            write_device(d)
            target = os.path.join(d, "firmware.syx")
            with open(target, "wb") as fh:
                fh.write(b"not a known firmware")
            with self.assertRaises(device.DeviceError):
                device.identify(target, d)

    def test_matches_by_content_not_filename(self):
        with tempfile.TemporaryDirectory() as d:
            payload = b"pretend firmware"
            import hashlib

            sha = hashlib.sha256(payload).hexdigest()
            write_device(d, SYNTHETIC.replace('"AABBCC"', '"%s"' % sha))
            # A name that matches no filename field in the device file.
            target = os.path.join(d, "renamed-by-the-user.syx")
            with open(target, "wb") as fh:
                fh.write(payload)
            dev, fw = device.identify(target, d)
            self.assertEqual(dev.name, "Test Device")
            self.assertEqual(fw.version, "9.9Z")

    def test_missing_directory_is_refused(self):
        with self.assertRaises(device.DeviceError):
            device.load_all("/nonexistent-devices-dir")


class ShippedDeviceFilesTest(unittest.TestCase):
    """The real files in devices/ -- data only, no firmware needed."""

    def setUp(self):
        self.devices = device.load_all(DEVICES)

    def test_every_device_states_its_capabilities(self):
        for dev in self.devices:
            for key in ("dsp", "seal", "sample_pool", "card"):
                self.assertIn(key, dev.capabilities, dev.name)
            self.assertIn("firmware", dev.acceptance, dev.name)
            self.assertTrue(
                any(f.version == dev.acceptance["firmware"] for f in dev.firmwares),
                dev.name,
            )

    def test_every_button_code_round_trips(self):
        for dev in self.devices:
            for code in dev.button_codes():
                pos = dev.wire_for(code)
                self.assertIsNotNone(pos, "%s code %d" % (dev.name, code))
                self.assertEqual(
                    dev.code_at(*pos), code, "%s code %d" % (dev.name, code)
                )


class ImageKeyTest(unittest.TestCase):
    def test_a_broken_device_directory_raises_instead_of_keying_by_stem(self):
        import pathlib
        from unittest import mock

        from tools.firmware import config

        with tempfile.TemporaryDirectory() as d:
            os.mkdir(os.path.join(d, "devices"))
            write_device(
                os.path.join(d, "devices"), '[device]\nshort = "x"\n'
            )  # no name
            fw = os.path.join(d, "a.syx")
            with open(fw, "wb") as fh:
                fh.write(b"x")
            with (
                mock.patch.object(config, "REPO", pathlib.Path(d)),
                self.assertRaises(device.DeviceError),
            ):
                config.image_key(fw)

    def test_an_unknown_image_is_keyed_by_its_stem(self):
        import pathlib
        from unittest import mock

        from tools.firmware import config

        with tempfile.TemporaryDirectory() as d:
            os.mkdir(os.path.join(d, "devices"))
            write_device(os.path.join(d, "devices"))
            fw = os.path.join(d, "a.syx")
            with open(fw, "wb") as fh:
                fh.write(b"x")
            with mock.patch.object(config, "REPO", pathlib.Path(d)):
                self.assertEqual(config.image_key(fw), "a")


if __name__ == "__main__":
    unittest.main()
