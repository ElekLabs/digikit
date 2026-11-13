"""Which product is this firmware, and what is on its front panel?

`tools/firmware/config.py` answers "which file", and `tools/coldfire/symbols.py` answers "which
addresses". Neither answers "which DEVICE" -- config settles a tie between
firmware files by hardcoded FILENAME, which is how running one product under
another's name happens silently.

A device file under `devices/` is the missing piece. It carries only what the
image cannot tell us:

  * identity, keyed by the firmware's SHA-256. The filename is a hint for
    error messages; people rename firmware files, hashes do not change.
  * the wire mapping. The link carries (channel, bit) and the firmware reports
    a control code; channels 0..5 are linear and channel 6 is not, differently
    on each product.
  * display grouping, which is editorial -- it says how to arrange controls,
    not what they are.

Deliberately NOT here: control names and guest addresses. Names come from the
firmware's own factory-test tables via `tools/coldfire/symbols.py`, and addresses are
resolved by signature. Freezing either into a data file would mean a new file
per firmware release and would lose exactly the property that makes both
products work from one code path.
"""

import hashlib
import os
import tomllib

# the repository's devices/, whatever the working directory is
DEVICES_DEFAULT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
    "devices",
)

BUTTON = "button"
ENCODER = "encoder"


class DeviceError(SystemExit):
    """No device matched, or a device file is malformed. The message says how."""


class UnknownFirmware(DeviceError):
    """The device files are fine; none of them lists this firmware's hash."""


class Firmware:
    """One known firmware release of a device."""

    def __init__(self, version, sha256, filename=None, raw=None):
        self.version = version
        self.sha256 = sha256.lower()
        self.filename = filename
        # the whole [[firmware]] table, for the optional keys (`main_sha256`, `port`, `dsp`)
        self.raw = dict(raw or {})

    def __repr__(self):
        return "<Firmware %s %s>" % (self.version, self.sha256[:12])


class Group:
    """A set of control codes and how to arrange them. Display only."""

    def __init__(self, name, kind, codes, layout="row", columns=None):
        self.name = name
        self.kind = kind
        self.codes = tuple(codes)
        self.layout = layout
        self.columns = columns

    def __repr__(self):
        return "<Group %s %s %d>" % (self.name, self.kind, len(self.codes))


class Device:
    """A product: its identity, its wire mapping and its panel arrangement."""

    def __init__(
        self,
        name,
        short,
        firmwares,
        linear_channels,
        encoders,
        exceptions,
        groups,
        path=None,
        capabilities=None,
        container=None,
        acceptance=None,
        has_panel=True,
        syx_id=None,
        manual=None,
    ):
        self.name = name
        self.short = short
        self.firmwares = tuple(firmwares)
        self.linear_channels = linear_channels
        self.encoders = encoders
        self.exceptions = dict(exceptions)
        self.groups = tuple(groups)
        self.path = path
        # [capabilities], [container], [acceptance] of the device file (docs/design/device-data.md); empty when the table is absent.
        self.capabilities = dict(capabilities or {})
        self.container = dict(container or {})
        self.acceptance = dict(acceptance or {})
        # False: the file has no [panel], so the wire mapping above is only a default and means nothing.
        self.has_panel = has_panel
        # `[device] syx_id` (the device byte of the .syx framing) and `[device] manual` (glob of the user manual under out/refs/); None when absent.
        self.syx_id = syx_id
        self.manual = manual

    def __repr__(self):
        return "<Device %s>" % self.name

    def wire_for(self, code):
        """-> (channel, bit) for a BUTTON code, or None if this product has none.

        A code with no wire position is not an error: Digitakt simply has
        fewer channel-6 controls than Digitone.
        """
        if code in self.exceptions:
            return self.exceptions[code]
        if 1 <= code <= self.linear_channels * 8:
            return ((code - 1) // 8, (code - 1) % 8)
        return None

    def code_at(self, channel, bit):
        """-> the BUTTON code reported by a (channel, bit), or None."""
        for code, pos in self.exceptions.items():
            if pos == (channel, bit):
                return code
        if 0 <= channel < self.linear_channels and 0 <= bit < 8:
            return channel * 8 + bit + 1
        return None

    def encoder_channel(self, code):
        """-> the wire channel for an ENCODER rotation code, or None.

        Rotation codes are a separate space from the encoder push buttons:
        rotation `code` is wire channel `code - 1`.
        """
        if 1 <= code <= self.encoders:
            return code - 1
        return None

    def button_codes(self):
        """-> every button code this product's groups declare, in order."""
        return tuple(c for g in self.groups if g.kind == BUTTON for c in g.codes)

    def image(self, fw):
        """-> the image key of one of this device's firmwares: "<short>-<version>"."""
        return "%s-%s" % (self.short, fw.version)

    def firmware(self, version):
        """-> the Firmware of this device with `version`, or None."""
        for fw in self.firmwares:
            if fw.version == version:
                return fw
        return None

    def has_dsp(self):
        """True when the device has a SHARC DSP stream (`[capabilities] dsp = "sharc"`)."""
        return self.capabilities.get("dsp") == "sharc"

    def firmware_for_sha256(self, sha256):
        for fw in self.firmwares:
            if fw.sha256 == sha256.lower():
                return fw
        return None


def _require(table, key, where):
    if key not in table:
        raise DeviceError("%s: missing [%s]" % (where, key))
    return table[key]


def load(path):
    """-> Device parsed from one TOML device file."""
    return _from_raw(_read(path), path)


def _read(path):
    with open(path, "rb") as fh:
        return tomllib.load(fh)


def _from_raw(raw, path):
    dev = _require(raw, "device", path)
    # [panel] is optional: an identity-only device file has none
    panel = raw.get("panel", {})
    firmwares = [
        Firmware(f.get("version"), _require(f, "sha256", path), f.get("filename"), f)
        for f in raw.get("firmware", ())
    ]
    # TOML bare keys are strings even when they look like integers, so the
    # exception table's control codes arrive as '49' rather than 49.
    exceptions = {}
    for code, pos in panel.get("exceptions", {}).items():
        if len(pos) != 2:
            raise DeviceError("%s: exception %s must be [channel, bit]" % (path, code))
        exceptions[int(code)] = (int(pos[0]), int(pos[1]))
    groups = [
        Group(
            g.get("name"),
            g.get("kind", BUTTON),
            g.get("codes", ()),
            g.get("layout", "row"),
            g.get("columns"),
        )
        for g in panel.get("group", ())
    ]
    return Device(
        name=_require(dev, "name", path),
        short=dev.get("short"),
        firmwares=firmwares,
        linear_channels=int(panel.get("linear_channels", 6)),
        encoders=int(panel.get("encoders", 9)),
        exceptions=exceptions,
        groups=groups,
        path=path,
        capabilities=raw.get("capabilities"),
        container=raw.get("container"),
        acceptance=raw.get("acceptance"),
        has_panel="panel" in raw,
        syx_id=dev.get("syx_id"),
        manual=dev.get("manual"),
    )


def devices_dir():
    return os.environ.get("DT2_DEVICES", DEVICES_DEFAULT)


def load_all(dirpath=None):
    """-> every Device under `dirpath`, sorted by name."""
    dirpath = dirpath or devices_dir()
    if not os.path.isdir(dirpath):
        raise DeviceError("No devices directory at %s" % dirpath)
    out = []
    for n in sorted(os.listdir(dirpath)):
        if not n.endswith(".toml"):
            continue
        path = os.path.join(dirpath, n)
        raw = _read(path)
        # devices/ also holds sidecar files (the *-leds.toml LED maps),
        # which have no [device] table
        if "device" in raw:
            out.append(_from_raw(raw, path))
    return sorted(out, key=lambda d: d.name)


def by_short(short, dirpath=None):
    """-> the Device whose `[device] short` is `short` (for example "dt2"); refuses, listing the known ones."""
    devices = load_all(dirpath)
    for d in devices:
        if d.short == short:
            return d
    raise DeviceError(
        "No device %r (known: %s)" % (short, ", ".join(d.short for d in devices))
    )


def by_image(image, dirpath=None):
    """-> the Device of an image key ("<short>-<version>"); refuses, listing the known ones."""
    return by_short(image.partition("-")[0], dirpath)


def by_syx_id(syx_id, dirpath=None):
    """-> the Device with `[device] syx_id` equal to `syx_id`, or None."""
    for d in load_all(dirpath):
        if d.syx_id == syx_id:
            return d
    return None


def dsp_shorts(dirpath=None):
    """-> the short names of the devices with a SHARC DSP, sorted: the `choices` of a DSP tool's --device."""
    return sorted(d.short for d in load_all(dirpath) if d.has_dsp())


def acceptance_images(dirpath=None):
    """-> {image key: firmware filename} of the firmware each device's acceptance tests run (`[acceptance] firmware`)."""
    out = {}
    for d in load_all(dirpath):
        version = d.acceptance.get("firmware")
        fw = d.firmware(version) if version else None
        if fw is not None and fw.filename:
            out[d.image(fw)] = fw.filename
    return out


class BootImage:
    """A firmware whose MAIN OS the emulator boots: its device, its Firmware, and the image key of its boot base."""

    def __init__(self, device, firmware, base):
        self.device = device
        self.firmware = firmware
        self.base = base


def boot_images(dirpath=None):
    """-> {image key: BootImage}: the firmwares that name a MAIN OS (`main_sha256`), in file order.

    The first one of a device is the base whose profile boots the later ones (a later release is booted from it with
    `--base-sha`); its `base` is None.
    """
    out = {}
    for d in load_all(dirpath):
        base = None
        for fw in d.firmwares:
            if "main_sha256" not in fw.raw:
                continue
            out[d.image(fw)] = BootImage(d, fw, base)
            base = base or d.image(fw)
    return out


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def identify(firmware_path, dirpath=None):
    """-> (Device, Firmware) for a firmware file, by content hash.

    Refuses rather than guesses. An unknown hash is a real answer: it means
    this is a firmware release nobody has mapped, not that it is one of the
    two we know.
    """
    sha = sha256_of(firmware_path)
    for device in load_all(dirpath):
        fw = device.firmware_for_sha256(sha)
        if fw is not None:
            return device, fw
    raise UnknownFirmware(
        "No device file matches %s\n\n"
        "  sha256 %s\n\n"
        "Known releases are listed in %s/. This is either a firmware version\n"
        "nobody has mapped yet or a different product; either way, guessing\n"
        "which one would run it under the wrong panel."
        % (firmware_path, sha, dirpath or devices_dir())
    )
