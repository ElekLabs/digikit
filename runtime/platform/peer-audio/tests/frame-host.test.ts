import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";

const directory = import.meta.dirname;

test("peer frame validates the bank and retains a voice during replacement", () => {
  const build = mkdtempSync(join(tmpdir(), "peer-audio-host-"));
  try {
    const binary = join(build, "frame-host");
    execFileSync(
      "cc",
      [
        "-std=c11",
        "-Wall",
        "-Wextra",
        "-Werror",
        join(directory, "frame-host.c"),
        join(directory, "../src/peer/frame.c"),
        "-o",
        binary,
      ],
      { timeout: 20_000 },
    );
    execFileSync(binary, { timeout: 20_000 });
  } finally {
    rmSync(build, { recursive: true, force: true });
  }
});

test("the copied receiver refers only to symbols relative to itself", () => {
  const object = JSON.parse(readFileSync(join(directory, "../build/peer-audio.dko.json"), "utf8"));
  const section = ".text.peer_audio";
  const local = new Set(
    object.symbols
      .filter((s: { section: string }) => s.section === section)
      .map((s: { name: string }) => s.name),
  );
  const relocs = object.relocs.filter((r: { section: string }) => r.section === section);
  expect(relocs.length).toBeGreaterThan(0);
  for (const { type, symbol } of relocs) {
    expect(type).toMatch(/^cf\.pc/);
    expect(local.has(symbol)).toBe(true);
  }
});
