#!/bin/sh
# The output report of an LFO shape module: tools/devtools/lfo_report.sh <author/name> [--device dn2|dt2]  (mise run lfo:report)
# Runs runtime/core/tests/lfo/lfo-report.manual.device.test.ts (sdk/templates/lfo-shape/README.md "Output report").
set -eu
id=
device=dn2
while [ $# -gt 0 ]; do
  case $1 in
    --device) device=$2; shift 2 ;;
    -h|--help) echo "usage: lfo_report.sh <author/name> [--device dn2|dt2]"; exit 0 ;;
    -*) echo "unknown option $1" >&2; exit 2 ;;
    *) id=$1; shift ;;
  esac
done
case $device in
  dn2|dt2) ;;
  *) echo "usage: lfo_report.sh <author/name> [--device dn2|dt2]  (got device '$device')" >&2; exit 2 ;;
esac
if [ -z "$id" ]; then
  echo "usage: lfo_report.sh <author/name> [--device dn2|dt2]" >&2
  exit 2
fi
case $id in
  */*) ;;
  *) echo "lfo_report.sh: the full module id AUTHOR/NAME is needed, got '$id'" >&2; exit 2 ;;
esac
cd "$(dirname "$0")/../.."
# the patchkit CLI, plusdrive and the daemons, the module, the lfo service and its bound file, the patched images (cached when current)
pnpm nx:pilot run-many -t cli,daemons -p patchkit,plusdrive-format,digi-control
pnpm nx:pilot run "$id:pack"
pnpm nx:pilot run-many -t pack,bind -p core/services/lfo
pnpm nx:pilot run patchkit:device-images
# Vitest prints the runtime log with it: the table is printed again at the end
DIGI_LFO_MODULE=$id DIGI_LFO_DEVICE=$device DIGIKIT_TIER=full REGRESSION_JOBS=1 \
  pnpm --filter @digi/regression test:runtime ../../runtime/core/tests/lfo/lfo-report.manual.device.test.ts
cat "out/runs/lfo-shape-report/$device/${id##*/}/summary.txt"
