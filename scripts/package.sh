#!/usr/bin/env bash
# Builds a release and packages it into dist/. CI runs this, and it works locally too.
# Usage: scripts/package.sh [label]    The label goes in the file name, such as v1.2.0.
set -euo pipefail

label="${1:-dev}"
cd "$(dirname "$0")/.."
os="$(uname -s)"
arch="$(uname -m)"

# By default whisper.cpp compiles for the exact CPU of the build machine, and on another CPU the
# result can crash with an illegal instruction. These set a baseline that users' machines will have.
export GGML_NATIVE=OFF
case "$os-$arch" in
  Darwin-arm64) export GGML_CPU_ARM_ARCH=armv8.2-a+dotprod+fp16 ;;  # every Apple Silicon Mac
  *-x86_64) export GGML_SSE42=ON GGML_AVX=ON GGML_AVX2=ON GGML_FMA=ON GGML_F16C=ON GGML_BMI2=ON ;;  # 2013 onward
esac
export MACOSX_DEPLOYMENT_TARGET=13.0
if [ "$os" = Darwin ]; then
  # With a deployment target older than the SDK, whisper.cpp's OS version checks call into clang's
  # runtime library, which rustc doesn't link by itself.
  export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=$(clang --print-runtime-dir)/libclang_rt.osx.a"
fi

cargo build --release --locked -p mp-app

target_dir="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"
binary="$target_dir/release/mouthpiece"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

notices=(LICENSE THIRD_PARTY.md crates/mp-export/LICENSE-LAME crates/mp-analysis/assets/LICENSE-CMUDICT)

rm -rf dist
mkdir -p dist

case "$os" in
  Darwin)
    app=dist/Mouthpiece.app
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$binary" "$app/Contents/MacOS/mouthpiece"
    cp "${notices[@]}" "$app/Contents/Resources/"
    cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Mouthpiece</string>
  <key>CFBundleDisplayName</key><string>Mouthpiece</string>
  <key>CFBundleIdentifier</key><string>app.mouthpiece.Mouthpiece</string>
  <key>CFBundleExecutable</key><string>mouthpiece</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
    # An ad-hoc signature: Apple Silicon won't run an unsigned bundle. It isn't notarized.
    codesign --force --deep --sign - "$app"
    ditto -c -k --keepParent "$app" "dist/Mouthpiece-$label-macos-$arch.zip"
    ;;
  Linux)
    stage=dist/mouthpiece
    mkdir -p "$stage"
    cp "$binary" "$stage/mouthpiece"
    cp README.md "${notices[@]}" "$stage/"
    tar -C dist -czf "dist/Mouthpiece-$label-linux-$arch.tar.gz" mouthpiece
    ;;
  *)
    echo "Unsupported platform: $os" >&2
    exit 1
    ;;
esac

ls -lh dist
