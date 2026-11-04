> A copy of the scripts the Karttahimmeli project uses for Lantmäteriet's Laserdata Skog (Sweden): listing and downloading the sheets (`stac_list.py`,
> `stac_expand.py`, `se_download.sh`), running `pullauta` on them in parallel (`se_proc_tile.sh`) and building one PMTiles archive per block of sheets
> (`se_build_pmtiles.py`). For a plain folder of sheets use `../laz-to-pmtiles.sh`. Paths and environment variables are at the top of each script.

# MapAnt vector tiles of Sweden (Lantmäteriet Laserdata Skog)

The same pipeline as Finland (`../README.md`), with Lantmäteriet's laser data instead of MML's: karttapullautin per 2.5 km sheet
(`vector_output=1`), then `pullauta vectortiles --crs sweref99tm` per block of sheets, then the archives are copied to tile2. The app side
is `kartta-himmli/doc/mapant-vector-layer.md` (section Sweden).

## Data

- STAC `https://api.lantmateriet.se/stac-hojd/v1`, collection `dsm-skoglig-copc`: COPC-LAZ (LAS 1.4, point format 6), 2.5 km sheets, EPSG:5845
  (SWEREF 99 TM + RH2000), CC BY 4.0, 0.9-2.3 points/m2 in the sheets looked at. Every position was scanned more than once (2019-2026):
  `stac_list.py` keeps the **newest** scan. The listing needs no login; the downloads (`dl1.lantmateriet.se`) need Basic authentication.
- pullautin reads the COPC files as they are.

## Steps

```bash
# 1. the sheets inside +-20 km of a point (the lon/lat of the town centre)
python3 stac_list.py skelleftea 20.95 64.75 20000  >  sheets.tsv      # area <TAB> url <TAB> size
python3 stac_list.py umea       20.263 63.826 20000 >> sheets.tsv
# 2. download (resumes, checks sizes; credentials only in the environment, never in a file)
LM_USER=... LM_PASSWORD=... ./se_download.sh sheets.tsv /data/se-lidar 2
# 3. pullautin on every sheet, 4 in parallel (INI: pullauta.ini with vector_output=1 and output_dxf=0)
export PULLAUTA=.../pullauta INI=.../pullauta.ini PROCESSED=/data/se-processed
awk -F'\t' '{n=split($2,p,"/"); id=p[n]; sub(/\.copc\.laz$/,"",id); print $1, "/data/se-lidar/" $1 "/" p[n], id}' sheets.tsv \
  | xargs -P 4 -L 1 ./se_proc_tile.sh
# 4. one PMTiles archive per block of 5 x 5 sheets (12.5 km), 2 builds in parallel
PMTILES_OUT=/data/se-pmtiles ./se_build_pmtiles.py 2
```

Blocks keep the vegetation mosaic small (it is one byte per metre: a 40 x 40 km area would need several GB) and are rebuilt only when their
sheets change. **Tile2 merges the archives that share a tile and serves every `*.pmtiles` of its directory** (`osm-mapsforge/serve.go`,
`mergeTiles`; the directory is re-read every 60 s), so the blocks are separate files `se-<area>_<x>_<y>.pmtiles` next to `tampere-test.pmtiles`,
with no change to the server or the app's URL. Copy them as in `../../doc/mapant-vector-tiles.md` (hidden temporary name, checksum, rename).
After copying, add the area's box to `SWE_MAPANT_VECTOR_AREAS` in the app (`SWE/MapAntVectorSWE.ts`): the layer asks only for tiles inside those boxes.

## Measured (2026-11-04, 8-core workstation, external USB disk)

| Step | Result |
|---|---|
| pullautin, 61 sheets, 4 parallel (`se_proc_tile.sh`) | 464 s wall in all; **36 sheets in 240 s (6.7 s per sheet)** once the disk stopped thrashing; one sheet alone takes 6-10 s |
| `pullauta vectortiles`, 14 blocks (1-13 sheets), 2 builds in parallel, 2 threads each | 175.8 s wall (498 s CPU); archives 0.1-3.5 MB, 13.6 MB in all |

Lessons:

- **Do not read and write the same USB disk from many jobs.** With downloads writing and four `cp` of 100 MB LAZ files reading, a copy took
  minutes; one reader at a time (`flock` in `se_proc_tile.sh`) and no downloads during processing fixed it (a read is then 33 MB/s, 3 s per sheet).
  Work in `/dev/shm`.
- A block of ten sheets in the town centre took 126 s (others 3-80 s); the vegetation polygons of a busy area are much slower than in the Finnish
  test area (5 s for nine sheets) and were not profiled yet.
- **tile3 caches an empty tile for a day.** Asking for a tile before tile2 has re-read the new archives (60 s) leaves the empty answer in tile3's
  cache; wait about two minutes after copying before testing, and test on tile2 first (`curl http://127.0.0.1:8094/14/x/y.pbf`).
