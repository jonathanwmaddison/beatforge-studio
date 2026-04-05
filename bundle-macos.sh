#!/bin/bash
# Build a macOS .app bundle for BeatForge Studio
set -e

echo "Building BeatForge Studio..."
cargo build --release

APP_NAME="BeatForge Studio"
APP_DIR="target/${APP_NAME}.app"
CONTENTS="${APP_DIR}/Contents"
MACOS="${CONTENTS}/MacOS"
RESOURCES="${CONTENTS}/Resources"

echo "Creating app bundle..."
rm -rf "${APP_DIR}"
mkdir -p "${MACOS}" "${RESOURCES}"

# Copy binary
cp target/release/beatforge-studio "${MACOS}/beatforge-studio"

# Create Info.plist
cat > "${CONTENTS}/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>beatforge-studio</string>
    <key>CFBundleIdentifier</key>
    <string>com.beatforge.studio</string>
    <key>CFBundleName</key>
    <string>BeatForge Studio</string>
    <key>CFBundleDisplayName</key>
    <string>BeatForge Studio</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSMicrophoneUsageDescription</key>
    <string>BeatForge Studio needs microphone access for audio sampling.</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>bfp</string>
            </array>
            <key>CFBundleTypeName</key>
            <string>BeatForge Project</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
        </dict>
    </array>
</dict>
</plist>
PLIST

echo "Done! App bundle created at: ${APP_DIR}"
echo "To install: cp -r '${APP_DIR}' /Applications/"
echo "Or just double-click the .app in Finder."
