#!/usr/bin/env bash
# Makes the app icon for each platform from assets/icon/darkroom-1024.png, the design system's
# app icon ("Darkroom" › Logos › darkroom-app-icon-1024.png):
#
#   darkroom.icns     macOS, for Darkroom.app
#   darkroom.ico      Windows, for the installer and its Start menu shortcut
#   darkroom-256.png  Linux, for the AppImage and its desktop entry
#
# The results are committed, so a build needs none of the tools here. Run it again (on macOS,
# which has sips and iconutil) when the icon changes.
#
# Usage: scripts/icons.sh
set -euo pipefail
cd "$(dirname "$0")/../assets/icon"

source=darkroom-1024.png
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

size() { sips -z "$1" "$1" "$source" --out "$2" > /dev/null; }

mkdir "$work/darkroom.iconset"
for s in 16 32 128 256 512; do
    size "$s" "$work/darkroom.iconset/icon_${s}x${s}.png"
    size "$((s * 2))" "$work/darkroom.iconset/icon_${s}x${s}@2x.png"
done
iconutil -c icns -o darkroom.icns "$work/darkroom.iconset"

size 256 darkroom-256.png

# An .ico can hold PNGs as they are (Windows Vista and later), so this only writes the directory.
sizes="16 24 32 48 64 128 256"
for s in $sizes; do
    size "$s" "$work/ico-$s.png"
done
python3 -I - "$work" darkroom.ico $sizes <<'PY'
import struct, sys
work, out, sizes = sys.argv[1], sys.argv[2], [int(s) for s in sys.argv[3:]]
images = [open(f"{work}/ico-{s}.png", "rb").read() for s in sizes]
offset = 6 + 16 * len(images)
head = struct.pack("<HHH", 0, 1, len(images))
for s, data in zip(sizes, images):
    # 256 is written as 0: the field is a byte.
    head += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
open(out, "wb").write(head + b"".join(images))
PY

ls -l darkroom.icns darkroom.ico darkroom-256.png
