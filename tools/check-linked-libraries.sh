#!/bin/sh
# The shared libraries a Linux release binary needs to start (its NEEDED
# entries) against the C library's own. The window system's (libxcb,
# libxkbcommon, Wayland) are opened at run time, by the graphical editor
# only (crates/kalem-ui/src/libraries.rs): linked, they stopped the binary
# at once on a server or in a container, even for `kalem tui`.
#
#   tools/check-linked-libraries.sh BINARY
set -eu
needed=$(readelf -d "$1" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')
echo "$1 needs:" $needed
extra=$(echo "$needed" | grep -Ev '^(libc|libm|libgcc_s|libdl|libpthread|librt|libutil)\.so\.[0-9]+$|^ld-linux[-a-z0-9_]*\.so\.[0-9]+$' || true)
if [ -n "$extra" ]; then
  echo "It links libraries beyond the C library's:" $extra >&2
  echo "Open them at run time instead, as crates/kalem-ui/src/libraries.rs says." >&2
  exit 1
fi
