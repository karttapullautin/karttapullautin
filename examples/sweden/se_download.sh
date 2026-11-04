#!/bin/bash
# usage: LM_USER=... LM_PASSWORD=... se_download.sh <sheets.tsv> <target dir> [parallel streams, default 2]
# Downloads the sheets of the list into <target dir>/<area>/, resumes partial files and checks the size. The credentials are only written to a
# temporary netrc (mode 600) that is removed at the end. Do not read the drive with other jobs at the same time: an external USB disk
# thrashes (the cp of the processing step went from 3 s to minutes per sheet while downloads were writing).
set -e
list=$1; target=$2; streams=${3:-2}
: "${LM_USER:?}" "${LM_PASSWORD:?}"
export NETRC; NETRC=$(mktemp); chmod 600 "$NETRC"; trap 'rm -f "$NETRC"' EXIT
printf 'machine dl1.lantmateriet.se login %s password %s\n' "$LM_USER" "$LM_PASSWORD" > "$NETRC"
export target
fetch() {
  area=$1; url=$2; size=$3; mkdir -p "$target/$area"; f=$target/$area/$(basename "$url")
  [ "$(stat -c %s "$f" 2>/dev/null)" = "$size" ] && return
  for try in 1 2 3 4; do
    curl -s -f -C - --netrc-file "$NETRC" -o "$f" "$url" && [ "$(stat -c %s "$f")" = "$size" ] && { echo "ok $area $(basename "$f")"; return; }
    sleep 10
  done
  echo "FAILED $(basename "$f")"
}
export -f fetch
tr '\t' ' ' < "$list" | xargs -P "$streams" -L 1 bash -c 'fetch "$@"' _
