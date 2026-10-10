#!/bin/sh
# The component plugins released in the index run with this build of
# Kalem (publish_todo 1.4): each is installed into a configuration folder
# of its own and started once by `kalem plugin check`, a viewer as
# opening a file would and an extension plugin as starting it would (not
# activated), which fails when one was built for another version of the
# plugin API. Run by the "Released plugins" workflow, and before tagging
# a release.
#
#   tools/check-released-plugins.sh [KALEM]
set -eu
kalem=${1:-kalem}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
KALEM_CONFIG_DIR="$tmp/config"
KALEM_STATE_DIR="$tmp/state"
export KALEM_CONFIG_DIR KALEM_STATE_DIR
index=${KALEM_PLUGIN_INDEX:-https://raw.githubusercontent.com/getkalem/plugins/main/index.json}
names=$(curl -sSfL "$index" | python3 -c '
import json, sys
for p in json.load(sys.stdin)["plugins"]:
    if p.get("kind") != "declarative" and p.get("download"):
        print(p["id"].rsplit(".", 1)[-1])
')
[ -n "$names" ] || { echo "no component plugins released in $index" >&2; exit 1; }
for n in $names; do
  "$kalem" plugin install --yes "$n"
done
"$kalem" plugin check
