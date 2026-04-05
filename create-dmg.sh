#!/bin/bash
# Create a DMG installer for BeatForge Studio
set -e

# First build the app bundle
./bundle-macos.sh

APP_NAME="BeatForge Studio"
DMG_NAME="BeatForge-Studio-v0.3.0"
DMG_DIR="target/dmg"
DMG_PATH="target/${DMG_NAME}.dmg"

echo "Creating DMG..."
rm -rf "${DMG_DIR}" "${DMG_PATH}"
mkdir -p "${DMG_DIR}"

# Copy app bundle
cp -r "target/${APP_NAME}.app" "${DMG_DIR}/"

# Create Applications symlink
ln -s /Applications "${DMG_DIR}/Applications"

# Create DMG
hdiutil create -volname "${APP_NAME}" \
    -srcfolder "${DMG_DIR}" \
    -ov -format UDZO \
    "${DMG_PATH}"

rm -rf "${DMG_DIR}"

echo ""
echo "DMG created: ${DMG_PATH}"
echo "Size: $(du -sh "${DMG_PATH}" | cut -f1)"
echo ""
echo "To install: open the DMG and drag BeatForge Studio to Applications"
