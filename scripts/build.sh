#!/usr/bin/env bash
# Builds a release of Darkroom for the platform this runs on and packages it in dist/:
#
#   macOS    Darkroom.app for Apple silicon and Intel in one (lipo), ad-hoc signed, in a .dmg
#   Linux    an AppImage, and the bare darkroom binary in a .tar.gz
#   Windows  an .msi installer (WiX 5), and the bare darkroom.exe in a .zip (run it from Git Bash)
#
# Linux and Windows can't be built from a Mac (system libraries, linkers); the release workflow
# (.github/workflows/release.yml) runs this script on a runner of each. The packaging tools are
# fetched on first use, at pinned versions: appimagetool and its runtime (checked against their
# SHA-256) into target/tools, and WiX as a .NET global tool. The icons come from assets/icon
# (scripts/icons.sh makes them).
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

# Downloads $1 to $2 unless it is there, and stops unless its SHA-256 is $3.
fetch() {
    if [ ! -f "$2" ]; then
        mkdir -p "$(dirname "$2")"
        curl -fsSL -o "$2.partial" "$1"
        mv "$2.partial" "$2"
    fi
    echo "$3  $2" | sha256sum -c --quiet - || { rm -f "$2"; echo "build.sh: $2 isn't the file pinned here" >&2; exit 1; }
}

APPIMAGETOOL=1.9.1
APPIMAGE_RUNTIME=20251108
WIX=5.0.2

outputs=()

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
        mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
        cp assets/icon/darkroom.icns "$app/Contents/Resources/"
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
    <key>CFBundleIconFile</key><string>darkroom</string>
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
        # The disk image opens on the app beside a link to Applications, to drag it across.
        stage="$dist/dmg"
        rm -rf "$stage"
        mkdir "$stage"
        cp -R "$app" "$stage/"
        ln -s /Applications "$stage/Applications"
        out="$dist/darkroom-$version-macos-universal.dmg"
        rm -f "$out"
        hdiutil create -quiet -volname Darkroom -srcfolder "$stage" -fs HFS+ -format UDZO "$out"
        rm -rf "$stage"
        outputs+=("$out")
        ;;
    Linux)
        build
        arch=$(uname -m)
        out="$dist/darkroom-$version-linux-$arch.tar.gz"
        tar -czf "$out" -C target/release darkroom
        outputs+=("$out")

        case $arch in
            x86_64) tool_sum=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0 runtime_sum=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d ;;
            aarch64) tool_sum=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158 runtime_sum=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444 ;;
            *) echo "build.sh: no AppImage tools pinned for $arch" >&2; exit 1 ;;
        esac
        tool=target/tools/appimagetool-$APPIMAGETOOL-$arch.AppImage
        runtime=target/tools/appimage-runtime-$APPIMAGE_RUNTIME-$arch
        fetch "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL/appimagetool-$arch.AppImage" "$tool" "$tool_sum"
        fetch "https://github.com/AppImage/type2-runtime/releases/download/$APPIMAGE_RUNTIME/runtime-$arch" "$runtime" "$runtime_sum"
        chmod +x "$tool"

        appdir="$dist/Darkroom.AppDir"
        rm -rf "$appdir"
        mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps"
        cp target/release/darkroom "$appdir/usr/bin/"
        cp packaging/darkroom.desktop "$appdir/usr/share/applications/"
        cp assets/icon/darkroom-256.png "$appdir/usr/share/icons/hicolor/256x256/apps/darkroom.png"
        # appimagetool wants the entry and icon at the top too, and AppRun is what the image runs.
        cp packaging/darkroom.desktop "$appdir/"
        cp assets/icon/darkroom-256.png "$appdir/darkroom.png"
        ln -s usr/bin/darkroom "$appdir/AppRun"
        out="$dist/darkroom-$version-linux-$arch.AppImage"
        rm -f "$out"
        # Extracted rather than mounted: build machines and containers often have no FUSE.
        ARCH=$arch APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream --runtime-file "$runtime" "$appdir" "$out"
        rm -rf "$appdir"
        outputs+=("$out")
        ;;
    MINGW* | MSYS* | CYGWIN*)
        build
        out="$dist/darkroom-$version-windows-x86_64.zip"
        rm -f "$out"
        powershell -NoProfile -Command "Compress-Archive -Path target/release/darkroom.exe -DestinationPath '$out'"
        outputs+=("$out")

        # .NET global tools land in ~/.dotnet/tools, which a fresh shell may not have on PATH.
        PATH="$HOME/.dotnet/tools:$PATH"
        if ! wix --version 2> /dev/null | grep -q "^$WIX"; then
            dotnet tool uninstall --global wix > /dev/null 2>&1 || true
            dotnet tool install --global wix --version "$WIX"
        fi
        out="$dist/darkroom-$version-windows-x86_64.msi"
        rm -f "$out"
        wix build -arch x64 -d "Version=$version" -d Exe=target/release/darkroom.exe -d Icon=assets/icon/darkroom.ico -o "$out" packaging/darkroom.wxs
        outputs+=("$out")
        ;;
    *)
        echo "build.sh: no recipe for $(uname -s)" >&2
        exit 1
        ;;
esac

printf '%s\n' "${outputs[@]}"
