#!/usr/bin/env python3
"""Import a raw ColdFire image as its own analysis image (Ghidra project, dump, cfdb).

    uv run python -m tools.coldfire.imagepipe NAME SECTION_FILE --base 0x40000400 \
        --code 0x40000400 0x4005df10 [--steps import,seeds,...] [--dry-run] [--force]

NAME is the image name (for example `<firmware>-peer`). Every path derives from it:

    Ghidra project   out/ghidra-projects/NAME-emac  (program /NAME/<file name>)
    seeds, RTTI      out/symbols/NAME-seeds.json, out/symbols/NAME-rtti.json
    dump             out/ghidra/NAME-emac
    cfdb             out/cfdb/NAME-emac.sqlite and its .fts.sqlite

Steps, in order:
    import   tools/coldfire/ghidra.sh import, then McfLabels.java
    seeds    tools.coldfire.codeseeds
    rtti     tools.coldfire.rttiscan
    apply    tools.coldfire.ghidraapply seeds --analyze, then rtti (pyghidra)
    dump     tools.coldfire.ghidradump (pyghidra)
    cfdb     tools.coldfire.cfdb build
    fts      tools.coldfire.cfdb fts

A default run skips a step whose output exists. `apply` runs only after `import`
ran in the same call, because it changes the project. Name `--steps` to run
any step again, and use `--force` to redo a step whose output exists.
`--dry-run` prints the commands and runs nothing.

Ghidra's pyghidra steps need `uv run --offline --with pyghidra --with jpype1`;
the project virtualenv does not include them.
"""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys

from tools import REPO_ROOT, python_env

STEPS = ("import", "seeds", "rtti", "apply", "dump", "cfdb", "fts")
LANGUAGE = "68000:BE:32:ColdfireEMAC"
PYGHIDRA = ("--with", "pyghidra==3.1.0", "--with", "jpype1==1.5.2")


class Paths:
    def __init__(self, name: str, section: str) -> None:
        self.name = name
        self.section = os.path.abspath(section)
        self.file = os.path.basename(self.section)
        self.project_dir = os.path.join(REPO_ROOT, "out", "ghidra-projects")
        self.project_name = name + "-emac"
        self.project = os.path.join(self.project_dir, self.project_name)
        self.folder = name
        self.program = "/%s/%s" % (name, self.file)
        self.seeds = os.path.join(REPO_ROOT, "out", "symbols", name + "-seeds.json")
        self.rtti = os.path.join(REPO_ROOT, "out", "symbols", name + "-rtti.json")
        self.dump = os.path.join(REPO_ROOT, "out", "ghidra", name + "-emac")
        self.cfdb = os.path.join(REPO_ROOT, "out", "cfdb", name + "-emac.sqlite")
        self.fts = os.path.join(REPO_ROOT, "out", "cfdb", name + "-emac.fts.sqlite")


def hexs(v: int) -> str:
    return "%#x" % v


def py(*module_args: str) -> list[str]:
    return [sys.executable, "-m", *module_args]


def pyghidra_py(*module_args: str) -> list[str]:
    return ["uv", "run", "--offline", *PYGHIDRA, "python", "-m", *module_args]


def commands(
    step: str, p: Paths, base: int, code: list[tuple[int, int]], threads: int
) -> list[tuple[list[str], dict[str, str]]]:
    """-> [(argv, extra environment)] for one step."""
    ranges: list[str] = []
    for lo, hi in code:
        ranges += ["--code", hexs(lo), hexs(hi)]
    ghidra_env = {
        "GHIDRA_PROJ": p.project,
        "GHIDRA_NAME": p.project_name,
        "GHIDRA_FOLDER": p.folder,
        "GHIDRA_LANG": LANGUAGE,
        "GHIDRA_IMG": p.section,
    }
    proj = ["--project", p.project, "--project-name", p.project_name]
    prog = ["--program", p.program]
    sh = os.path.join("tools", "coldfire", "ghidra.sh")
    if step == "import":
        return [
            ([sh, "import"], ghidra_env),
            ([sh, "run", "McfLabels.java"], ghidra_env),
        ]
    if step == "seeds":
        return [
            (
                py(
                    "tools.coldfire.codeseeds",
                    p.section,
                    "--base",
                    hexs(base),
                    *ranges,
                    "--json",
                    p.seeds,
                ),
                {},
            )
        ]
    if step == "rtti":
        return [
            (
                py(
                    "tools.coldfire.rttiscan",
                    p.section,
                    "--base",
                    hexs(base),
                    *ranges,
                    "--json",
                    p.rtti,
                ),
                {},
            )
        ]
    if step == "apply":
        return [
            (
                pyghidra_py(
                    "tools.coldfire.ghidraapply",
                    "seeds",
                    p.seeds,
                    *proj,
                    *prog,
                    "--analyze",
                ),
                {},
            ),
            (
                pyghidra_py("tools.coldfire.ghidraapply", "rtti", p.rtti, *proj, *prog),
                {},
            ),
        ]
    if step == "dump":
        return [
            (
                pyghidra_py(
                    "tools.coldfire.ghidradump",
                    "--out",
                    p.dump,
                    *proj,
                    *prog,
                    "--image",
                    p.section,
                    "--rtti",
                    p.rtti,
                    "--threads",
                    str(threads),
                ),
                {},
            )
        ]
    if step == "cfdb":
        return [(py("tools.coldfire.cfdb", "build", p.dump), {})]
    if step == "fts":
        return [(py("tools.coldfire.cfdb", "fts", p.name + "-emac"), {})]
    raise SystemExit("unknown step %s" % step)


def done(step: str, p: Paths) -> bool:
    """True if the step's output exists."""
    if step == "import":
        return os.path.exists(os.path.join(p.project, p.project_name + ".gpr"))
    if step == "seeds":
        return os.path.exists(p.seeds)
    if step == "rtti":
        return os.path.exists(p.rtti)
    if step == "dump":
        return os.path.exists(os.path.join(p.dump, "manifest.json"))
    if step == "cfdb":
        return os.path.exists(p.cfdb)
    if step == "fts":
        return os.path.exists(p.fts)
    return False  # apply has no output of its own


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("name", help="image name, e.g. <firmware>-peer")
    ap.add_argument("section", help="raw section file (a flat ColdFire image)")
    ap.add_argument("--base", type=lambda s: int(s, 0), required=True)
    ap.add_argument(
        "--code",
        action="append",
        nargs=2,
        type=lambda s: int(s, 0),
        metavar=("LO", "HI"),
        required=True,
        help="code range, repeatable",
    )
    ap.add_argument("--steps", help="comma list of: " + ", ".join(STEPS))
    ap.add_argument("--threads", type=int, default=4, help="decompile threads")
    ap.add_argument(
        "--force", action="store_true", help="run steps whose output exists"
    )
    ap.add_argument("--dry-run", action="store_true", help="print commands only")
    return ap.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if not args.dry_run and not os.path.isfile(args.section):
        raise SystemExit("no section file %s" % args.section)
    p = Paths(args.name, args.section)
    explicit = args.steps.split(",") if args.steps else None
    if explicit:
        for s in explicit:
            if s not in STEPS:
                raise SystemExit("unknown step %s (one of %s)" % (s, ", ".join(STEPS)))
    ran_import = False
    for step in STEPS:
        if explicit is not None:
            if step not in explicit:
                continue
        elif step == "apply":
            if not ran_import:
                print("# skip apply (project not imported in this run)")
                continue
        elif done(step, p) and not args.force:
            print("# skip %s (output exists)" % step)
            continue
        for argv_, extra in commands(step, p, args.base, args.code, args.threads):
            env_txt = " ".join("%s=%s" % (k, shlex.quote(v)) for k, v in extra.items())
            print("+ %s%s" % (env_txt + " " if env_txt else "", shlex.join(argv_)))
            if args.dry_run:
                continue
            env = python_env()
            env.update(extra)
            subprocess.run(argv_, cwd=REPO_ROOT, env=env, check=True)
        ran_import = ran_import or step == "import"
    return 0


if __name__ == "__main__":
    sys.exit(main())
