#!/bin/sh
# Probe: Chrome DevTools Protocol as a structured channel into Electron apps.
#
# `--remote-debugging-port` is a Chromium switch, not an Electron fuse, so most Electron apps
# accept it unless their own code refuses. With it you get every renderer's DOM, JS evaluation and
# push events (DOM.*, Runtime.*, Network.*) over a WebSocket: a far richer tree than their AX.
# The fuses only gate Node-level debugging (--inspect, NODE_OPTIONS, ELECTRON_RUN_AS_NODE).
#
#   sh probes/cdp.sh list                      Electron apps and their Node-debugging fuses
#   sh probes/cdp.sh launch "Obsidian" [9222]  launch an app (quit it first!) with a CDP port
#   sh probes/cdp.sh targets [9222]            list the pages/targets exposed on that port
#
# Single-instance apps forward argv to the running instance and exit, so the app must be quit
# before `launch`. Then, e.g.:  npx wscat -c <webSocketDebuggerUrl>  and send
#   {"id":1,"method":"Runtime.evaluate","params":{"expression":"document.title"}}

case "$1" in
list)
  # Fuse wire: sentinel, then version byte, length byte, then one byte per fuse: '0' off, '1' on,
  # 'r' removed. Order: RunAsNode, CookieEncryption, NodeOptionsEnv, NodeCliInspect,
  # AsarIntegrity, OnlyLoadFromAsar, V8SnapshotPerProcess, FileProtocolPrivileges, ...
  printf '%-28s %-7s %-7s %-7s %s\n' app runNode nodeOpt inspect electron
  for app in /Applications/*.app "$HOME"/Applications/*.app; do
    fw="$app/Contents/Frameworks/Electron Framework.framework"
    [ -d "$fw" ] || continue
    bin="$fw/Electron Framework"
    off=$(LC_ALL=C grep -obUa 'dL7pKGdnNz796PbbjQWNKmHXBZaB9tsX' "$bin" | head -1 | cut -d: -f1)
    wire=$([ -n "$off" ] && dd if="$bin" bs=1 skip=$((off + 34)) count=4 2>/dev/null)
    ver=$(defaults read "$fw/Resources/Info.plist" CFBundleVersion 2>/dev/null)
    f() { echo "$wire" | cut -c"$1" | sed 's/1/on/;s/0/off/;s/r/removed/'; }
    printf '%-28s %-7s %-7s %-7s %s\n' "$(basename "$app" .app)" "$(f 1)" "$(f 3)" "$(f 4)" "$ver"
  done
  ;;
launch)
  open -na "$2" --args --remote-debugging-port="${3:-9222}"
  echo "launched $2; try: sh $0 targets ${3:-9222}"
  ;;
targets)
  curl -s "http://127.0.0.1:${2:-9222}/json/list" || echo "nothing listening on ${2:-9222}"
  ;;
*)
  sed -n '2,17p' "$0"
  ;;
esac
