#!/usr/bin/env bash
# Re-ports the trial mods (keeping what was translated), installs them all into one
# instance, builds it once, and checks the ports given after `--check`:
#   tools/play_all.sh "porter trials" --check farmers-delight-refabricated beautify-refabricated
# Ports live in ai-test/trials/ports/<slug>-port, jars in ai-test/survey/<slug>/jar.
set -u
cd "$(dirname "$0")/.."
instance="$1"; shift
checks=()
if [ "${1:-}" = "--check" ]; then shift; checks=("$@"); fi
exe=target-dev/debug/horizon-porter.exe
[ -x "$exe" ] || exe=target-dev/debug/horizon-porter
home="${APPDATA:-$HOME/.local/share}/horizon/instances/$(echo "$instance" | tr ' ' '-')"
rm -f "$home"/horizon-mods/* "$home"/horizon-native-mods/* "$home/horizon-mods.txt"
last=""
for port in ai-test/trials/ports/*-port; do
  slug=$(basename "$port" -port)
  jar=$(ls ai-test/survey/"$slug"/jar/*.jar 2>/dev/null | head -1)
  [ -n "$jar" ] && "$exe" port "$jar" --out "$port" --force > /dev/null 2>&1
  "$exe" install "$port" --instance "$instance" > /dev/null 2>&1 && last="$port"
done
echo "installed: $(ls "$home/horizon-mods" | wc -l) content, $(ls "$home/horizon-native-mods" | wc -l) native"
"$exe" install "$last" --instance "$instance" --build --trust-native-code > ai-test/play-all-build.log 2>&1 \
  && echo "built" || { echo "build failed, see ai-test/play-all-build.log"; exit 1; }
grep -i "put back" ai-test/play-all-build.log | head -5
for slug in "${checks[@]}"; do
  log="ai-test/trials/ports/$slug-port/.porter/check.log"
  "$exe" check "ai-test/trials/ports/$slug-port" --instance "$instance" > "$log" 2>&1
  echo "$slug: $(grep -E 'blocks place' "$log" | tail -1) $(grep -c '^FAULT' "$log") faults"
done
