#!/usr/bin/env bash
# Installs, builds and checks each port folder given, one at a time, in one instance:
#   tools/recheck.sh "porter trials" ai-test/trials/ports/naturalist-port work/mcwbridges-port
# Each one's output goes to <port>/.porter/recheck.log; a summary line per port to stdout.
set -u
instance="$1"; shift
exe="$(dirname "$0")/../target-dev/debug/horizon-porter.exe"
[ -x "$exe" ] || exe="$(dirname "$0")/../target-dev/debug/horizon-porter"
home="${APPDATA:-$HOME/.local/share}/horizon/instances/$(echo "$instance" | tr ' ' '-')"
for port in "$@"; do
  mkdir -p "$port/.porter"
  log="$port/.porter/recheck.log"
  rm -f "$home"/horizon-mods/* "$home"/horizon-native-mods/* "$home/horizon-mods.txt"
  if "$exe" install "$port" --instance "$instance" --build --trust-native-code > "$log" 2>&1 \
     && "$exe" check "$port" --instance "$instance" >> "$log" 2>&1; then
    echo "$(basename "$port"): $(grep -E 'blocks place' "$log" | tail -1) $(grep -c '^FAULT' "$log") faults"
  else
    echo "$(basename "$port"): failed, see $log"
  fi
done
