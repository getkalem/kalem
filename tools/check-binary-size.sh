#!/bin/sh
# The size of a release binary against its ceiling (roadmap R2.1).
#
#   tools/check-binary-size.sh BINARY NAME
#
# The ceilings in tools/binary-size.txt are the sizes measured when they
# were last lowered, plus a margin; the targets beside them are those of
# the Book's performance page. A change that grows the binary past its
# ceiling fails here: lower the ceiling with each reduction, never raise
# it without a decision recorded in the changelog.
set -eu
bin=$1
name=$2
line=$(grep "^$name " "$(dirname "$0")/binary-size.txt")
[ -n "$line" ] || { echo "no ceiling for $name in tools/binary-size.txt" >&2; exit 1; }
ceiling=$(echo "$line" | awk '{print $2}')
target=$(echo "$line" | awk '{print $3}')
bytes=$(wc -c < "$bin")
mib=$(awk "BEGIN { printf \"%.1f\", $bytes / 1048576 }")
echo "$name: $mib MiB (ceiling $ceiling MiB, target $target MiB)"
if awk "BEGIN { exit !($bytes > $ceiling * 1048576) }"; then
  echo "The $name binary grew past its ceiling of $ceiling MiB." >&2
  exit 1
fi
