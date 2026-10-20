#!/bin/sh
# Every manifest carries the crate version. The launcher downloads the release
# package.json names, so a manifest that lags ships a plugin running an old
# server. With an argument, the version must also equal it (a release tag).

set -eu

field() {
	sed -n "s/^ *\"*version\"* *[=:] *\"\([^\"]*\)\".*/\1/p" "$1" | head -n 1
}

crate=$(field Cargo.toml)
status=0
for manifest in package.json .claude-plugin/plugin.json; do
	found=$(field "$manifest")
	if [ "$found" != "$crate" ]; then
		echo "$manifest has $found, Cargo.toml has $crate" >&2
		status=1
	fi
done
if [ $# -gt 0 ] && [ "${1#v}" != "$crate" ]; then
	echo "tag $1, crate version $crate" >&2
	status=1
fi
exit $status
