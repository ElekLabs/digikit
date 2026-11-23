"""Select a firmware release and its program independently of the instruction set."""

from __future__ import annotations

from functools import cache
from pathlib import Path
from typing import NamedTuple

from tools.firmware import device
from tools.firmware.query_db import FwError, short_image


class Program(NamedTuple):
    image: str
    cpu: str


@cache
def devices() -> tuple[device.Device, ...]:
    return tuple(device.load_all())


@cache
def known_release(image: str) -> tuple[device.Device, device.Firmware] | None:
    image = short_image(image).removesuffix("-peer")
    return next(
        (
            (dev, fw)
            for dev in devices()
            for fw in dev.firmwares
            if dev.image(fw) == image
        ),
        None,
    )


def select_image(
    image: str | None, device_name: str | None, version: str | None
) -> str | None:
    if not device_name:
        if version:
            raise FwError("--firmware selects a version and requires --device")
        return image
    if image:
        raise FwError("choose --image or --device")
    name = device_name.casefold()
    dev = next(
        (
            d
            for d in devices()
            if name
            in (d.short.casefold(), d.name.casefold(), Path(d.path).stem.casefold())
        ),
        None,
    )
    if dev is None:
        raise FwError(f"unknown device {device_name!r}")
    if version is None:
        version = dev.acceptance.get("firmware")
        if version is None and len(dev.firmwares) == 1:
            version = dev.firmwares[0].version
    firmware = dev.firmware(version)
    if firmware is None:
        versions = ", ".join(f.version for f in dev.firmwares)
        raise FwError(f"choose --firmware for {dev.name} ({versions})")
    return dev.image(firmware)


def dsp_program(image: str) -> Program | None:
    release = known_release(image)
    if release is None:
        return None
    dev, fw = release
    main = dev.image(fw)
    if "cf2" in dev.container:
        return Program(main + "-peer", "cf")
    if dev.has_dsp():
        return Program(fw.raw.get("dsp", {}).get("image", main), "sharc")
    return None


def select_core(image: str, core: str, cpu: str | None = None) -> Program:
    if core == "main":
        program = Program(short_image(image).removesuffix("-peer"), "cf")
    else:
        dsp = dsp_program(image)
        if dsp is None:
            raise FwError(f"no DSP program in the device profile for {image}")
        program = dsp
    if cpu and cpu != program.cpu:
        raise FwError(
            f"{image} {core} uses {program.cpu}, incompatible with --cpu {cpu}"
        )
    return program
