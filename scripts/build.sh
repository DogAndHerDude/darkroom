#!/usr/bin/env bash
# Builds a release of Darkroom for the platform this runs on and packages it in dist/:
#
#   macOS    Darkroom.app for Apple silicon and Intel in one (lipo), ad-hoc signed, zipped
#   Linux    the darkroom binary in a .tar.gz
#   Windows  darkroom.exe in a .zip (run it from Git Bash)
#
# Linux and Windows can't be built from a Mac (system libraries, linkers); the release workflow
# (.github/workflows/release.yml) runs this script on a runner of each.
#
# Usage: scripts/build.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# The workspace version: the first top-level `version = ` in Cargo.toml, under [workspace.package].
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
dist=dist
mkdir -p "$dist"

build() {
    cargo build --release --locked -p darkroom "$@"
}

case "$(uname -s)" in
    Darwin)
        targets="aarch64-apple-darwin x86_64-apple-darwin"
        rustup target add $targets
        # Both slices on Apple silicon's minimum. Intel's default (10.12) predates Swift in the OS,
        # so the linker loads a Swift library through an rpath the app doesn't have, and it won't start.
        export MACOSX_DEPLOYMENT_TARGET=11.0
        for t in $targets; do
            build --target "$t"
        done
        app="$dist/Darkroom.app"
        rm -rf "$app"
        mkdir -p "$app/Contents/MacOS"
        lipo -create -output "$app/Contents/MacOS/darkroom" target/aarch64-apple-darwin/release/darkroom target/x86_64-apple-darwin/release/darkroom
        cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Darkroom</string>
    <key>CFBundleDisplayName</key><string>Darkroom</string>
    <key>CFBundleIdentifier</key><string>com.github.dogandherdude.darkroom</string>
    <key>CFBundleExecutable</key><string>darkroom</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
        # Ad hoc: no Developer ID, so a downloaded copy is quarantined and Gatekeeper asks
        # (System Settings › Privacy & Security › Open Anyway, or `xattr -dr com.apple.quarantine`).
        codesign --force --sign - "$app"
        out="$dist/darkroom-$version-macos-universal.zip"
        rm -f "$out"
        # Without extended attributes, which other unzippers would leave behind as ._ files.
        ditto -c -k --norsrc --noextattr --keepParent "$app" "$out"
        ;;
    Linux)
        build
        out="$dist/darkroom-$version-linux-$(uname -m).tar.gz"
        tar -czf "$out" -C target/release darkroom
        ;;
    MINGW* | MSYS* | CYGWIN*)
        build
        out="$dist/darkroom-$version-windows-x86_64.zip"
        rm -f "$out"
        powershell -NoProfile -Command "Compress-Archive -Path target/release/darkroom.exe -DestinationPath '$out'"
        ;;
    *)
        echo "build.sh: no recipe for $(uname -s)" >&2
        exit 1
        ;;
esac

echo "$out"
