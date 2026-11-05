#!/usr/bin/env bash
# From a folder of LAZ/LAS sheets to one PMTiles archive of vector tiles: the whole chain of this fork.
#
#   examples/laz-to-pmtiles.sh <folder of .laz/.las files> <output folder> [--crs <name>] [--jobs N] [--scan-year YYYY]
#
# 1. every sheet is run through `pullauta` in a folder of its own (<output>/runs/<sheet>), with `vector_output = 1` (no PNG maps) in its pullauta.ini,
#    N sheets at a time; only the files the tile builder needs are kept of its `temp` folder (the rest is hundreds of MB);
# 2. `pullauta vectortiles <output>/runs <output>/out` joins the sheets and writes <output>/out/map.pmtiles (z14 and the z13 overview) and metadata.json.
#
# Environment: PULLAUTA = the binary (default ./target/release/pullauta, build it with `cargo build --release`).
# --scan-year writes the laser scanning year of the sheets (shown by an application as "Laser scanning 2022"); without it the year is read from the
# names of Lantmäteriet sheets (y25g017-...) or left out. Large areas: run it per block of about 12 km and let a tile server merge the archives.
set -euo pipefail

PULLAUTA=${PULLAUTA:-./target/release/pullauta}
INI=${INI:-$(dirname "$0")/../pullauta.default.ini}
crs=tm35fin
jobs=$(nproc)
scan_year=
positional=()
while [ $# -gt 0 ]; do
  case $1 in
    --crs) crs=$2; shift 2 ;;
    --jobs) jobs=$2; shift 2 ;;
    --scan-year) scan_year=$2; shift 2 ;;
    *) positional+=("$1"); shift ;;
  esac
done
[ ${#positional[@]} -eq 2 ] || { sed -n 2,13p "$0"; exit 2; }
in=$(realpath "${positional[0]}"); out=$(mkdir -p "${positional[1]}" && realpath "${positional[1]}")
PULLAUTA=$(realpath "$PULLAUTA"); INI=$(realpath "$INI")
runs=$out/runs
mkdir -p "$runs"

# no PNG maps, no text DXF: the layers as .dxf.bin, the vegetation raster and the water cells are what the tiles are made from
sed -E 's/^vector_output[[:space:]]*=.*/vector_output = 1/; s/^output_dxf[[:space:]]*=.*/output_dxf = 0/' "$INI" > "$out/pullauta.vector.ini"

sheet() {
  laz=$1
  name=$(basename "$laz"); name=${name%.*}
  dir=$runs/$name
  [ -f "$dir/.done" ] && { echo "have $name"; return; }
  rm -rf "$dir"; mkdir -p "$dir"
  cp "$out/pullauta.vector.ini" "$dir/pullauta.ini"
  ( cd "$dir" && "$PULLAUTA" "$laz" > run.log 2>&1 ) || { echo "FAILED $name (see $dir/run.log)"; return 1; }
  [ -n "$scan_year" ] && echo "$scan_year" > "$dir/scan_year.txt"
  # keep only what `vectortiles` reads
  find "$dir/temp" -type f ! \( -name vegetation.png -o -name vegetation.pgw -o -name water.txt -o -name out2.dxf.bin \
    -o -name formlines.dxf.bin -o -name c2g.dxf.bin -o -name c3g.dxf.bin -o -name dotknolls.dxf.bin \) -delete
  touch "$dir/.done"; echo "ok $name"
}
export -f sheet
export PULLAUTA out runs scan_year

start=$(date +%s)
find "$in" -maxdepth 1 -type f \( -iname '*.laz' -o -iname '*.las' \) -print0 | sort -z \
  | xargs -0 -n 1 -P "$jobs" bash -c 'sheet "$0"'
mid=$(date +%s)

"$PULLAUTA" vectortiles "$runs" "$out/out" --crs "$crs" --no-tile-files
end=$(date +%s)
echo "sheets: $((mid - start)) s, tiles: $((end - mid)) s -> $out/out/map.pmtiles ($(du -h "$out/out/map.pmtiles" | cut -f1))"
