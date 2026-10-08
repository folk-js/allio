#!/bin/sh
# Builds the two text-services probes into bundles under ./build.
#
#   sh probes/textservices/build.sh            build only
#   sh probes/textservices/build.sh install    also copy to ~/Library/Services and refresh pbs
#   sh probes/textservices/build.sh uninstall  remove them again
#
# After installing: pick "en (Allio)" in System Settings > Keyboard > Text Input > Edit… > Spelling
# (set it back to "Automatic by Language" afterwards). The Services appear in every app's
# app menu > Services when text is selected.
set -e
cd "$(dirname "$0")"
out=build
services="$HOME/Library/Services"

if [ "$1" = uninstall ]; then
  pkill -x AllioSpell || true
  pkill -x AllioService || true
  rm -rf "$services/AllioSpell.service" "$services/AllioService.service"
  /System/Library/CoreServices/pbs -update
  echo "removed"
  exit 0
fi

bundle() { # name, info-plist-services-xml
  dir="$out/$1.service/Contents"
  mkdir -p "$dir/MacOS"
  cat >"$dir/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>org.folkjs.allio.probe.$1</string>
  <key>CFBundleName</key><string>$1</string>
  <key>CFBundleExecutable</key><string>$1</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSBackgroundOnly</key><true/>
  <key>NSServices</key><array>$2</array>
</dict></plist>
EOF
}

rm -rf "$out"
mkdir -p "$out"

bundle AllioSpell '
  <dict>
    <key>NSExecutable</key><string>AllioSpell</string>
    <key>NSLanguages</key><array><string>en</string></array>
    <key>NSSpellChecker</key><string>Allio</string>
  </dict>'
swiftc -O -o "$out/AllioSpell.service/Contents/MacOS/AllioSpell" spell.swift

types='<array>
      <string>public.utf8-plain-text</string><string>public.rtf</string><string>com.apple.flat-rtfd</string>
      <string>public.html</string><string>public.url</string><string>public.file-url</string>
      <string>public.tiff</string><string>public.png</string><string>com.adobe.pdf</string>
    </array>'
bundle AllioService "
  <dict>
    <key>NSMenuItem</key><dict><key>default</key><string>Allio: Inspect Selection</string></dict>
    <key>NSMessage</key><string>inspect</string>
    <key>NSPortName</key><string>AllioService</string>
    <key>NSSendTypes</key>$types
  </dict>
  <dict>
    <key>NSMenuItem</key><dict><key>default</key><string>Allio: Uppercase Selection</string></dict>
    <key>NSMessage</key><string>uppercase</string>
    <key>NSPortName</key><string>AllioService</string>
    <key>NSSendTypes</key><array><string>public.rtf</string><string>public.utf8-plain-text</string></array>
    <key>NSReturnTypes</key><array><string>public.rtf</string><string>public.utf8-plain-text</string></array>
  </dict>"
swiftc -O -o "$out/AllioService.service/Contents/MacOS/AllioService" service.swift

codesign -s - --force "$out/AllioSpell.service" "$out/AllioService.service" 2>/dev/null || true
echo "built $out/AllioSpell.service $out/AllioService.service"

if [ "$1" = install ]; then
  rm -rf "$services/AllioSpell.service" "$services/AllioService.service"
  cp -R "$out/AllioSpell.service" "$out/AllioService.service" "$services/"
  /System/Library/CoreServices/pbs -update
  echo "installed to $services; logs in ~/Library/Logs/allio-{spell,service}.log"
fi
