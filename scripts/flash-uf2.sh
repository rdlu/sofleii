#!/usr/bin/env bash
# Flash one .uf2 onto a half in bootloader mode, after checking that the
# mounted drive really is that half of this keyboard (#98): `sofleii flash
# identify` reads the firmware on the chip from the drive's CURRENT.UF2.
#
#   scripts/flash-uf2.sh <file.uf2> <left|right|any> [what to tell the user]
#
# `any` (settings_reset) only checks that it is this keyboard. FORCE=1 skips
# the check, e.g. for a chip whose flash identifies as unknown (erased, or
# reset twice). NICENANO_MOUNT overrides where the drive mounts. Used by the
# mise flash-* and build-flash tasks.
set -euo pipefail

UF2="$1"
HALF="$2"
WHO="${3:-the ${HALF^^} half}"
MOUNT="${NICENANO_MOUNT:-/run/media/$USER/NICENANO}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOFLEII="$ROOT/sofleii/target/release/sofleii"

[ -f "$UF2" ] || { echo "ERROR: $UF2 not found"; exit 1; }
case "$HALF" in left | right | any) ;; *) echo "ERROR: half must be left, right or any"; exit 1 ;; esac

if [ "${FORCE:-0}" != 1 ]; then
  # Built before the reset tap, so the bootloader isn't kept waiting.
  cargo build -q --release --manifest-path "$ROOT/sofleii/Cargo.toml"
fi

echo ""
echo "==============================================="
echo ">> Double-tap RESET on $WHO now"
echo "==============================================="
for _ in $(seq 1 60); do [ -d "$MOUNT" ] && break; sleep 1; done
[ -d "$MOUNT" ] || { echo "ERROR: NICENANO not detected within 60s"; exit 1; }
sleep 1 # give udisks a moment to finish mounting

if [ "${FORCE:-0}" = 1 ]; then
  echo ">> FORCE=1: flashing without checking which half this is"
else
  expect=()
  [ "$HALF" = any ] || expect=(--expect "$HALF")
  if ! "$SOFLEII" flash identify --mount "$MOUNT" "${expect[@]}"; then
    echo ""
    echo "ERROR: not flashing: the drive is not $WHO (see above)."
    echo "       Double-tap reset on the correct half and run this again, or FORCE=1 to skip the check."
    exit 1
  fi
fi

echo ">> Flashing $UF2..."
cp "$UF2" "$MOUNT/"
sync
echo "✅ $UF2 flashed."
