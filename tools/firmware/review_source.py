"""Build a SHARC catalog and review directly from a registered stock SysEx."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from tools.firmware import catalog_page, device, extract
from tools.firmware.artifacts import FirmwareArtifacts
from tools.firmware.query_db import ROOT, FwError
from tools.firmware.query_target import select_core
from tools.sharc_dev import fw_query as sharc
from tools.sharc_dev.sharc_catalog import DSP_ROOTS


def sharc_image(path: Path, cpu: str | None) -> str:
    try:
        dev, firmware = device.identify(path)
    except (OSError, device.UnknownFirmware) as exc:
        raise FwError(str(exc)) from exc
    release = dev.image(firmware)
    dsp = select_core(release, "dsp", cpu)
    if dsp.cpu != "sharc" or dsp.image not in DSP_ROOTS:
        raise FwError(f"{release} has no SHARC catalog build from SysEx")
    return dsp.image


def check_overlap(path: Path, others: list[Path]) -> None:
    resolved = path.resolve()
    if any(
        resolved == other.resolve()
        or (path.is_file() and other.is_file() and path.samefile(other))
        for other in others
    ):
        raise FwError(
            f"output path overlaps a firmware input or another artifact: {path}"
        )


def check_paths(
    sources: dict[str, Path], sections: dict[str, Path], outputs: list[Path]
) -> None:
    build_paths: list[Path] = []
    for name in sources:
        artifacts = FirmwareArtifacts(name, "sharc")
        build_paths.extend(
            (
                artifacts.database,
                artifacts.legacy_database,
                *artifacts.catalog_exports(),
            )
        )

    section_dirs = {directory.resolve() for directory in sections.values()}
    for source in sources.values():
        check_overlap(source, build_paths)
    used = [*sources.values(), *build_paths]
    for path in outputs:
        check_overlap(path, used)
        if path.is_dir():
            raise FwError(f"review output is a directory: {path}")
        used.append(path)

    for path in (*sources.values(), *outputs):
        path = path.resolve()
        if path.parent in section_dirs and (
            path.match("section_*.bin") or path.name == extract.SOURCE_MARKER
        ):
            raise FwError(f"path overlaps a generated firmware section: {path}")


def review_from_syx(
    args: argparse.Namespace, core: str | None
) -> list[catalog_page.ReviewOutput]:
    if args.image or args.device or args.firmware or args.other:
        raise FwError("--syx selects the release; pair files with --other-syx")
    if core not in (None, "dsp"):
        raise FwError("--syx review builds the SHARC DSP catalog; select --core dsp")

    image = sharc_image(args.syx, args.cpu)
    sources = {image: args.syx}
    other = None
    if args.other_syx is not None:
        other = sharc_image(args.other_syx, args.cpu)
        if other == image:
            raise FwError("--other-syx must select a different SHARC image")
        sources[other] = args.other_syx

    primary = FirmwareArtifacts(image, "sharc")
    alternate = args.out and Path(args.out).resolve() != primary.review.resolve()
    primary_args = argparse.Namespace(**(vars(args) | {"out": None})) if other else args
    outputs = list(catalog_page.review_paths(primary, primary_args.out))
    if other:
        outputs.extend(
            catalog_page.review_paths(FirmwareArtifacts(other, "sharc"), None)
        )
        if alternate:
            outputs.extend(catalog_page.review_paths(primary, args.out))
    sections = {name: Path(ROOT) / "out" / "sections" / name for name in sources}
    check_paths(sources, sections, outputs)

    for name, path in sources.items():
        extract.extract(path, sections[name])
    sharc.build_images(image, other, report=lambda line: print(line, file=sys.stderr))

    first = catalog_page.export_review(image, None, "sharc", primary_args)
    if other is None:
        return [first]
    peer_args = argparse.Namespace(
        **(vars(primary_args) | {"labels": None, "label_catalog": None})
    )
    peer = catalog_page.export_review(other, None, "sharc", peer_args)
    if alternate:
        first = catalog_page.export_review(image, None, "sharc", args)
    return [first, peer]
