#!/bin/bash
# usage: se_proc_tile.sh <area> <laz path> <sheet id>
# Runs pullautin (vector_output=1) on one sheet in a tmpfs work dir and keeps the files `pullauta vectortiles` reads in
# $PROCESSED/<area>/<sheet id>/temp. Environment: PULLAUTA (binary), INI (a pullauta.ini with vector_output=1, output_dxf=0), PROCESSED.
area=$1; laz=$2; id=$3
: "${PULLAUTA:?}" "${INI:?}" "${PROCESSED:?}"
OUT=$PROCESSED/$area/$id
[ -f "$OUT/.done" ] && { echo "have $id"; exit 0; }
W=/dev/shm/se_work_$id; rm -rf "$W"; mkdir -p "$W"
cp "$INI" "$W/pullauta.ini"
flock /tmp/se_cp.lock cp "$laz" "$W/t.laz"   # one reader at a time: parallel reads thrash an external drive
( cd "$W" && "$PULLAUTA" t.laz > run.log 2>&1 ) || { echo "FAILED $id"; tail -3 "$W/run.log"; rm -rf "$W"; exit 1; }
mkdir -p "$OUT/temp"
for f in vegetation.png vegetation.pgw water.txt out2.dxf.bin formlines.dxf.bin c2g.dxf.bin c3g.dxf.bin dotknolls.dxf.bin; do
  cp "$W/temp/$f" "$OUT/temp/" || echo "missing $f in $id"
done
touch "$OUT/.done"; rm -rf "$W"; echo "ok $area $id"
