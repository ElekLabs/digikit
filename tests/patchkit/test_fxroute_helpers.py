"""Synthetic card and safety proofs for the FX route data helpers."""

from __future__ import annotations

import json
import struct
import tempfile
import unittest
from pathlib import Path

from tools.coldfire import arena_safety
from tools.device import dksettings
from tools.patchkit import pk_device
from tools.patchkit import pk_digikit_fxroutes as routes


class FxRouteHelpersTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)

    def card(self, device):
        store = pk_device.store_of(device)
        card = self.folder / f"{device}.img"
        with card.open("wb") as file:
            file.truncate(store.end * 512)
        return card, store

    def watch(self):
        output = self.folder / "watch.jsonl"
        (self.folder / "watch-0.jsonl").write_text("")
        (self.folder / "watch-0.summary.json").write_text(
            json.dumps({"events": 0, "flush_hits": [], "cpushl": 1})
        )
        return output

    def overlay(self, *sectors):
        path = self.folder / "overlay.bin"
        path.write_bytes(
            b"".join(struct.pack("<Q", s) + bytes(64 + 512) for s in sectors)
        )
        return path

    def test_seed_round_trip_preserves_card_and_other_settings_copy(self):
        for device in ("dt2", "dn2"):
            with self.subTest(device=device):
                card, store = self.card(device)
                size = card.stat().st_size
                with card.open("r+b") as file:
                    file.write(b"caller-owned")
                entries = [
                    {"key": "fx/route/test/send", "value_hex": "00"},
                    {"key": "fx/send/test/send", "value_hex": bytes(range(16)).hex()},
                ]
                result = routes.seed_card(card, device, entries)
                self.assertEqual(result["bytes"] % 512, 0)
                self.assertEqual(card.stat().st_size, size)
                with card.open("rb") as file:
                    self.assertEqual(file.read(12), b"caller-owned")
                    file.seek(store.set_a * 512)
                    raw = file.read(result["bytes"])
                    record = dksettings.decode(raw)
                    self.assertIsNotNone(record)
                    self.assertEqual(record.seq, 1)
                    self.assertEqual(record.get(entries[0]["key"]), b"\x00")
                    self.assertEqual(record.get(entries[1]["key"]), bytes(range(16)))
                    self.assertEqual(raw[record.used :], bytes(len(raw) - record.used))
                    file.seek(store.set_b * 512)
                    self.assertEqual(file.read(512), bytes(512))
                with self.assertRaisesRegex(ValueError, "empty"):
                    routes.seed_card(card, device, entries)

    def test_seed_rejects_malformed_entries_before_writing(self):
        card, store = self.card("dn2")
        bad_entries = [
            {},
            [{}],
            [{"key": "a", "value_hex": "zz"}],
            [{"key": "space key", "value_hex": "00"}],
            [{"key": "a", "value_hex": "00"}] * 2,
            [{"key": "a", "value_hex": "00" * (dksettings.VALUE_MAX + 1)}],
            [{"key": "a", "value_hex": "", "extra": 1}],
            [{"key": "é", "value_hex": ""}],
            [
                {
                    "key": "a" * dksettings.KEY_MAX,
                    "value_hex": "00" * dksettings.VALUE_MAX,
                }
                for _ in range(dksettings.AREA // dksettings.VALUE_MAX + 1)
            ],
        ]
        for entries in bad_entries:
            with (
                self.subTest(entries=str(entries)[:100]),
                self.assertRaises(ValueError),
            ):
                routes.seed_card(card, "dn2", entries)
        with card.open("rb") as file:
            file.seek(store.set_a * 512)
            self.assertEqual(
                file.read(store.set_copy_sectors * 512),
                bytes(store.set_copy_sectors * 512),
            )

    def test_seed_rejects_short_card_and_occupied_copy_b(self):
        card = self.folder / "short.img"
        card.write_bytes(b"caller-owned")
        with self.assertRaisesRegex(ValueError, "too small"):
            routes.seed_card(card, "dt2", [])
        self.assertEqual(card.read_bytes(), b"caller-owned")
        card, store = self.card("dt2")
        with card.open("r+b") as file:
            file.seek(store.set_b * 512 + 511)
            file.write(b"x")
        with self.assertRaisesRegex(ValueError, "empty"):
            routes.seed_card(card, "dt2", [])

    def test_route_policy_allows_only_legacy_kit_and_settings_sectors(self):
        output = self.watch()
        for device in ("dt2", "dn2"):
            with self.subTest(device=device):
                store = pk_device.store_of(device)
                lo, hi = routes.KIT_AREAS[device][0]
                overlay = self.overlay(
                    store.set_a, store.set_b, lo, hi - 1, store.sel_a, 0x58000
                )
                self.assertFalse(
                    arena_safety.runtime_safety(device, output, [overlay])["pass"]
                )
                result = arena_safety.runtime_safety(
                    device, output, [overlay], policy="fx-routes"
                )
                self.assertTrue(result["pass"])
                self.assertEqual(
                    set(result["policy"]["store_extra"]), {store.set_a, store.set_b}
                )
                for sector in (
                    lo - 1,
                    hi,
                    store.first,
                    store.set_a + 1,
                    store.set_b + 1,
                ):
                    overlay = self.overlay(sector)
                    self.assertFalse(
                        arena_safety.runtime_safety(
                            device, output, [overlay], policy="fx-routes"
                        )["pass"]
                    )

    def test_route_policy_rejects_malformed_evidence_and_unknown_policy(self):
        output = self.watch()
        overlay = self.overlay()
        with self.assertRaisesRegex(ValueError, "unknown"):
            arena_safety.runtime_safety("dt2", output, [overlay], policy="all")
        overlay.write_bytes(b"torn")
        with self.assertRaises(ValueError):
            arena_safety.runtime_safety("dt2", output, [overlay], policy="fx-routes")
        overlay.write_bytes(b"")
        (self.folder / "watch-0.summary.json").write_text('{"events":')
        with self.assertRaises(ValueError):
            arena_safety.runtime_safety("dt2", output, [overlay], policy="fx-routes")


if __name__ == "__main__":
    unittest.main()
