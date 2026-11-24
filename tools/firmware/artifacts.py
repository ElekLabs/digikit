"""Paths shared by firmware database builders, catalog exporters, and readers."""

from __future__ import annotations

import os
from contextlib import suppress
from dataclasses import dataclass, field
from pathlib import Path
from typing import NamedTuple

from tools.firmware.query_db import ATLAS_DIR, CF_DIR, ROOT, SHARC_DIR, short_image
from tools.firmware.query_target import devices, dsp_program, known_release

OUTPUT_ROOT = Path(ROOT) / "out"


class CatalogExports(NamedTuple):
    catalog: Path
    groups: Path
    graph: Path


@dataclass(frozen=True)
class FirmwareArtifacts:
    image: str
    cpu: str
    root: Path = field(default_factory=lambda: OUTPUT_ROOT)

    @property
    def directory(self) -> Path:
        image = short_image(self.image)
        core = "dsp" if self.cpu == "sharc" or image.endswith("-peer") else "main"
        release = image.removesuffix("-peer") if self.cpu == "cf" else image
        if self.cpu == "sharc" and known_release(image) is None:
            release = sharc_release(image)
        return self.root / "firmware" / release / core

    @property
    def database(self) -> Path:
        return self.directory / "firmware.sqlite"

    @property
    def review(self) -> Path:
        return self.directory / "review.html"

    @property
    def review_tsv(self) -> Path:
        return self.directory / "exports" / "review.tsv"

    @property
    def labels(self) -> Path:
        return self.directory / "labels"

    def catalog_exports(self, out_dir: str | None = None) -> CatalogExports:
        if out_dir is None:
            directory = self.directory / "exports"
            return CatalogExports(
                directory / "catalog.tsv",
                directory / "groups.tsv",
                directory / "graph.json",
            )
        directory = Path(out_dir)
        stem = short_image(self.image) + ("-dsp" if self.cpu == "sharc" else "")
        return CatalogExports(
            directory / f"{stem}-catalog.tsv",
            directory / f"{stem}-catalog-groups.tsv",
            directory / f"{stem}-catalog-graph.json",
        )

    @property
    def legacy_database(self) -> Path:
        short = short_image(self.image)
        if self.cpu == "sharc":
            return self.root / "sharcdb" / f"{short}.sqlite"
        return self.root / "cfdb" / f"{short}-emac.sqlite"

    def database_path(self, legacy_dir: str | None = None) -> Path:
        default = SHARC_DIR if self.cpu == "sharc" else CF_DIR
        if (
            legacy_dir is not None
            and Path(legacy_dir).resolve() != Path(default).resolve()
        ):
            return Path(legacy_dir) / self.legacy_database.name
        if not self.database.is_file() and self.legacy_database.is_file():
            return self.legacy_database
        return self.database

    def publish_legacy_alias(self) -> None:
        legacy = self.legacy_database
        legacy.parent.mkdir(parents=True, exist_ok=True)
        # An earlier build or a user-provided database may own this path.
        with suppress(FileExistsError):
            legacy.symlink_to(os.path.relpath(self.database, legacy.parent))

    def label_files(self, legacy_atlas: str = ATLAS_DIR) -> list[str]:
        legacy = Path(legacy_atlas) / "labels"
        if self.cpu == "sharc":
            legacy /= "sharc"
        paths = sorted(self.labels.glob("*.jsonl"))
        return [str(p) for p in paths or sorted((legacy / self.image).glob("*.jsonl"))]


def sharc_release(image: str) -> str:
    # A device profile can give the SHARC image a different name from its release.
    for dev in devices():
        for firmware in dev.firmwares:
            release = dev.image(firmware)
            program = dsp_program(release)
            if (
                program is not None
                and program.cpu == "sharc"
                and program.image == image
            ):
                return release
    return image


def database_path(image: str, cpu: str, legacy_dir: str | None = None) -> str:
    return str(FirmwareArtifacts(image, cpu).database_path(legacy_dir))


def coldfire_database_path(name: str, db_dir: str) -> str:
    # The low-level importer also accepts explicitly named, standalone dumps.
    if not name.endswith("-emac"):
        return os.path.join(db_dir, name + ".sqlite")
    return database_path(name, "cf", db_dir)
