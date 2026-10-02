use image::{GrayImage, Luma};
use imageproc::drawing::draw_line_segment_mut;
use log::info;
use rustc_hash::FxHashMap as HashMap;
use std::error::Error;
use std::path::Path;

use crate::config::Config;
use crate::constants::{ELEVATION_SNAP_M, FIXED_POINT_SCALE};
use crate::geometry::{BinaryDxf, Bounds, Classification, Geometry, Point2, Points, Polylines};
use crate::io::bytes::FromToBytes;
use crate::io::fs::FileSystem;
use crate::io::heightmap::HeightMap;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Dotknolls {
    pub dotknolls: Vec<Dotknoll>,
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Dotknoll {
    pub x: f64,
    pub y: f64,
    pub is_knoll: bool,
}

pub fn dotknolls(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
) -> Result<(), Box<dyn Error>> {
    info!("Identifying dotknolls...");

    let scalefactor = config.scalefactor;

    let heightmap_in = tmpfolder.join("xyz_knolls.hmap");
    let hmap = HeightMap::from_bytes(&mut fs.open(heightmap_in)?)?;

    // in world coordinates
    let xstart = hmap.xoffset;
    let ystart = hmap.yoffset;

    // in grid coordinates
    let xmax = (hmap.grid.width() - 1) as f64;
    let ymax = (hmap.grid.height() - 1) as f64;
    let size = hmap.scale;

    let mut im = GrayImage::from_pixel(
        (xmax * size / scalefactor) as u32,
        (ymax * size / scalefactor) as u32,
        Luma([0xff]),
    );

    let data = BinaryDxf::from_reader(&mut fs.open(tmpfolder.join("out2.dxf.bin"))?)?;
    let Geometry::Polylines3(lines) = data.take_geometry().swap_remove(0) else {
        return Err(anyhow::anyhow!("out2.dxf.bin should contain polylines").into());
    };

    for (line, _) in lines.iter() {
        for i in 1..line.len() {
            draw_line_segment_mut(
                &mut im,
                (
                    ((line[i - 1].x - xstart) / scalefactor).floor() as f32,
                    ((line[i - 1].y - ystart) / scalefactor).floor() as f32,
                ),
                (
                    ((line[i].x - xstart) / scalefactor).floor() as f32,
                    ((line[i].y - ystart) / scalefactor).floor() as f32,
                ),
                Luma([0x0]),
            )
        }
    }

    let mut dotknoll_points = Points::new();

    let dotknolls: Dotknolls =
        crate::util::read_object(&mut fs.open(tmpfolder.join("dotknolls.bin"))?)?;

    for dot in dotknolls.dotknolls {
        let Dotknoll { x, y, is_knoll } = dot;

        let mut ok = true;
        let mut i = (x - xstart) / scalefactor - 3.0;
        while i < (x - xstart) / scalefactor + 4.0 && ok {
            if (i as u32) >= im.width() {
                ok = false;
                break;
            }
            let mut j = (y - ystart) / scalefactor - 3.0;
            while j < (y - ystart) / scalefactor + 4.0 && ok {
                if (j as u32) >= im.height() {
                    ok = false;
                    break;
                }
                let pix = im.get_pixel(i as u32, j as u32);
                if pix[0] == 0 {
                    ok = false;
                    break;
                }
                j += 1.0;
            }
            i += 1.0;
        }

        let layer2 = match (ok, is_knoll) {
            (true, true) => Classification::Dotknoll,
            (true, false) => Classification::Udepression,
            (false, true) => Classification::UglyDotknoll,
            (false, false) => Classification::UglyUdepression,
        };

        dotknoll_points.push(Point2::new(x, y), layer2);
    }

    let dxf = BinaryDxf::new(
        Bounds::new(xstart, xmax * size + xstart, ystart, ymax * size + ystart),
        vec![dotknoll_points.into()],
    );

    // write binary
    let mut f = fs
        .create(tmpfolder.join("dotknolls.dxf.bin"))
        .expect("Unable to create file");
    dxf.to_writer(&mut f)
        .expect("could not write dotknolls.dxf.bin");

    if config.output_dxf {
        dxf.to_dxf(&mut fs.create(tmpfolder.join("dotknolls.dxf"))?)?;
    }

    info!("Done");
    Ok(())
}
pub fn knolldetector(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
) -> anyhow::Result<()> {
    info!("Detecting knolls...");
    let scalefactor = config.scalefactor;
    let contour_interval = config.contour_interval;

    let halfinterval = contour_interval / 2.0 * scalefactor;

    let contours_ratio = contour_interval / 5.0 * scalefactor;

    let interval = 0.3 * scalefactor;

    let heightmap_in = tmpfolder.join("xyz_03.hmap");
    let hmap = HeightMap::from_bytes(&mut fs.open(heightmap_in)?)?;

    // in world coordinates
    let xstart = hmap.xoffset;
    let ystart = hmap.yoffset;
    let size = hmap.scale;

    // in grid coordinates
    let (xmin, ymin) = (0, 0);
    let xmax = (hmap.grid.width() - 1) as u64;
    let ymax = (hmap.grid.height() - 1) as u64;

    // Temporary hashmap to store the xyz values (TODO: replace with direct hmap lookup!)
    let mut xyz: HashMap<(u64, u64), f64> = HashMap::default();
    for (x, y, h) in hmap.grid.iter() {
        xyz.insert((x as u64, y as u64), h);
    }

    let data = BinaryDxf::from_reader(&mut fs.open(tmpfolder.join("contours03.dxf.bin"))?)?;
    let Geometry::Polylines2(lines) = data.take_geometry().swap_remove(0) else {
        anyhow::bail!("contours03.dxf.bin should contain polylines");
    };

    let detected_bounds = Bounds::new(xmin as f64, xmax as f64, ymin as f64, ymax as f64);
    let mut detected_lines = Polylines::<Point2, Classification>::new();

    // Internal type used to index into the hashmaps and vectors.
    // Since using f64 coordinates directly has problems with rounding (and do not impl Eq and
    // Hash), we can use an integer representation of the coordinates to index into the HashMaps.
    // By multiplying by 1000, we can keep a precision of 3 decimal places, which is sufficient for
    // what we need.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct Key {
        x: i64,
        y: i64,
    }
    impl Key {
        fn new(x: f64, y: f64) -> Self {
            Key {
                x: (x * FIXED_POINT_SCALE) as i64,
                y: (y * FIXED_POINT_SCALE) as i64,
            }
        }
        /// Just a unique key for the case where we don't have a valid point.
        fn none() -> Self {
            Self {
                x: i64::MAX,
                y: i64::MAX,
            }
        }
    }

    let mut heads1: HashMap<Key, usize> = HashMap::default();
    let mut heads2: HashMap<Key, usize> = HashMap::default();
    let mut heads = Vec::<Key>::with_capacity(lines.len());
    let mut tails = Vec::<Key>::with_capacity(lines.len());
    let mut el_x = Vec::<Vec<f64>>::with_capacity(lines.len());
    let mut el_y = Vec::<Vec<f64>>::with_capacity(lines.len());

    for (j, (line, _c)) in lines.iter().enumerate() {
        // TODO; might need to lower to 200
        if line.len() < 201 {
            let first = line.first().unwrap();
            let last = line.last().unwrap();

            let head = Key::new(first.x, first.y);
            let tail = Key::new(last.x, last.y);

            heads.push(head);
            tails.push(tail);

            // TODO: this is not very efficient (collecting all x and y separately into Vecs), but it means the logic further down can stay the same
            el_x.push(line.iter().map(|p| p.x).collect::<Vec<_>>());
            el_y.push(line.iter().map(|p| p.y).collect::<Vec<_>>());

            if *heads1.get(&head).unwrap_or(&0) == 0 {
                heads1.insert(head, j);
            } else {
                heads2.insert(head, j);
            }
            if *heads1.get(&tail).unwrap_or(&0) == 0 {
                heads1.insert(tail, j);
            } else {
                heads2.insert(tail, j);
            }
        } else {
            heads.push(Key::none());
            tails.push(Key::none());
            el_x.push(vec![]);
            el_y.push(vec![]);
        }
    }

    for l in 0..lines.len() {
        let mut to_join = 0;
        if !el_x[l].is_empty() {
            let mut end_loop = false;
            while !end_loop {
                let tmp = *heads1.get(&heads[l]).unwrap_or(&0);
                if tmp != 0 && tmp != l && !el_x[tmp].is_empty() {
                    to_join = tmp;
                } else {
                    let tmp = *heads2.get(&heads[l]).unwrap_or(&0);
                    if tmp != 0 && tmp != l && !el_x[tmp].is_empty() {
                        to_join = tmp;
                    } else {
                        let tmp = *heads2.get(&tails[l]).unwrap_or(&0);
                        if tmp != 0 && tmp != l && !el_x[tmp].is_empty() {
                            to_join = tmp;
                        } else {
                            let tmp = *heads1.get(&tails[l]).unwrap_or(&0);
                            if tmp != 0 && tmp != l && !el_x[tmp].is_empty() {
                                to_join = tmp;
                            } else {
                                end_loop = true;
                            }
                        }
                    }
                }
                if !end_loop {
                    if tails[l] == heads[to_join] {
                        heads2.insert(tails[l], 0);
                        heads1.insert(tails[l], 0);
                        let mut to_append = el_x[to_join].to_vec();
                        el_x[l].append(&mut to_append);
                        let mut to_append = el_y[to_join].to_vec();
                        el_y[l].append(&mut to_append);
                        tails[l] = tails[to_join];
                        el_x[to_join].clear();
                        el_y[to_join].clear();
                    } else if tails[l] == tails[to_join] {
                        heads2.insert(tails[l], 0);
                        heads1.insert(tails[l], 0);
                        let mut to_append = el_x[to_join].to_vec();
                        to_append.reverse();
                        el_x[l].append(&mut to_append);
                        let mut to_append = el_y[to_join].to_vec();
                        to_append.reverse();
                        el_y[l].append(&mut to_append);
                        tails[l] = heads[to_join];
                        el_x[to_join].clear();
                        el_y[to_join].clear();
                    } else if heads[l] == tails[to_join] {
                        heads2.insert(heads[l], 0);
                        heads1.insert(heads[l], 0);
                        let to_append = el_x[to_join].to_vec();
                        el_x[l].splice(0..0, to_append);
                        let to_append = el_y[to_join].to_vec();
                        el_y[l].splice(0..0, to_append);
                        heads[l] = heads[to_join];
                        el_x[to_join].clear();
                        el_y[to_join].clear();
                    } else if heads[l] == heads[to_join] {
                        heads2.insert(heads[l], 0);
                        heads1.insert(heads[l], 0);
                        let mut to_append = el_x[to_join].to_vec();
                        to_append.reverse();
                        el_x[l].splice(0..0, to_append);
                        let mut to_append = el_y[to_join].to_vec();
                        to_append.reverse();
                        el_y[l].splice(0..0, to_append);
                        heads[l] = tails[to_join];
                        el_x[to_join].clear();
                        el_y[to_join].clear();
                    }
                }
            }
        }
    }

    let mut elevation: HashMap<u64, f64> = HashMap::default();
    for l in 0..lines.len() {
        let mut skip = false;
        let el_x_len = el_x[l].len();
        if el_x_len > 0 {
            if el_x_len > 121 {
                skip = true;
                el_x[l].clear();
                el_y[l].clear();
            }
            if el_x_len < 9 {
                let mut p = 0;
                let mut dist = 0.0;
                while p < el_x_len - 1 {
                    dist += ((el_x[l][p] - el_x[l][p + 1]).powi(2)
                        + (el_y[l][p] - el_y[l][p + 1]).powi(2))
                    .sqrt();
                    p += 1;
                }
                if dist < 5.0 || el_x_len < 3 {
                    skip = true;
                    el_x[l].clear();
                    el_y[l].clear();
                }
            }
            if el_x[l].first() != el_x[l].last() || el_y[l].first() != el_y[l].last() {
                skip = true;
                el_x[l].clear();
                el_y[l].clear();
            }
            if !skip
                && el_x_len < 122
                && el_x[l].first() == el_x[l].last()
                && el_y[l].first() == el_y[l].last()
            {
                let tailx = *el_x[l].first().unwrap();
                let mut xl = el_x[l].to_vec();
                xl.push(tailx);
                let taily = *el_y[l].first().unwrap();
                let mut yl = el_y[l].to_vec();
                yl.push(taily);
                let mut mm = ((el_x_len as f64 / 3.0).floor() - 1.0) as i32;
                if mm < 0 {
                    mm = 0;
                }
                let mut m = mm as usize;
                let mut h = 0.0;
                while m < xl.len() {
                    let xm = xl[m];
                    let ym = yl[m];
                    let xo = (xm - xstart) / size;
                    let yo = (ym - ystart) / size;
                    if xo == xo.floor() {
                        let h1 = *xyz
                            .get(&(xo.floor() as u64, yo.floor() as u64))
                            .unwrap_or(&0.0);
                        let h2 = *xyz
                            .get(&(xo.floor() as u64, yo.floor() as u64 + 1))
                            .unwrap_or(&0.0);
                        h = h1 * (yo.floor() + 1.0 - yo) + h2 * (yo - yo.floor());
                        h = (h / interval + 0.5).floor() * interval;
                        break;
                    } else if m < (el_x_len - 3) && yo == yo.floor() {
                        let h1 = *xyz
                            .get(&(xo.floor() as u64, yo.floor() as u64))
                            .unwrap_or(&0.0);
                        let h2 = *xyz
                            .get(&(xo.floor() as u64 + 1, yo.floor() as u64))
                            .unwrap_or(&0.0);
                        h = h1 * (xo.floor() + 1.0 - xo) + h2 * (xo - xo.floor());
                        h = (h / interval + 0.5).floor() * interval;
                    }
                    m += 1;
                }
                elevation.insert(l as u64, h);

                let mut mm = ((el_x_len as f64 / 3.0).floor() - 1.0) as i32;
                if mm < 0 {
                    mm = 0;
                }
                let mut m = mm as usize;
                let mut xa = xl[m];
                let mut ya = yl[m];
                while m < xl.len() {
                    let xm = xl[m];
                    let ym = yl[m];
                    let xo = (xm - xstart) / size;
                    let yo = (ym - ystart) / size;
                    if m < xl.len() - 3 && yo == yo.floor() && xo != xo.floor() {
                        xa = xo.floor() * size + xstart;
                        ya = ym.floor();
                        break;
                    }
                    m += 1;
                }
                let h_center = *xyz
                    .get(&(
                        ((xa - xstart) / size).floor() as u64,
                        ((ya - ystart) / size).floor() as u64,
                    ))
                    .unwrap_or(&0.0);
                let mut hit = 0;
                let xtest = ((xa - xstart) / size).floor() * size + xstart + 0.000000001;
                let ytest = ((ya - ystart) / size).floor() * size + ystart + 0.000000001;

                let mut n = 0;
                let mut y0 = 0.0;
                let mut x0 = 0.0;
                while n < (el_x_len - 1) {
                    let x1 = el_x[l][n];
                    let y1 = el_y[l][n];
                    if n > 0
                        && ((y0 <= ytest && ytest < y1) || (y1 <= ytest && ytest < y0))
                        && (xtest < ((x1 - x0) * (ytest - y0) / (y1 - y0) + x0))
                    {
                        hit += 1;
                    }
                    n += 1;
                    x0 = x1;
                    y0 = y1;
                }

                if (h_center < h) && (hit % 2 == 1) || (h_center > h) && (hit % 2 != 1) {
                    skip = true;
                    el_x[l].clear();
                    el_y[l].clear();
                }
            }
        }
        if skip {
            el_x[l].clear();
            el_y[l].clear();
        }
    }

    struct Head {
        id: u64,
        xtest: f64,
        ytest: f64,
    }
    let mut heads = Vec::<Head>::new();
    for l in 0..lines.len() {
        if !el_x[l].is_empty() {
            if el_x[l].first() == el_x[l].last() && el_y[l].first() == el_y[l].last() {
                heads.push(Head {
                    id: l as u64,
                    xtest: el_x[l][0],
                    ytest: el_y[l][0],
                });
            } else {
                el_x[l].clear();
                el_y[l].clear();
            }
        }
    }
    struct Top {
        id: u64,
        xtest: f64,
        ytest: f64,
    }
    let mut tops = Vec::<Top>::new();
    struct BoundingBox {
        minx: f64,
        maxx: f64,
        miny: f64,
        maxy: f64,
    }
    let mut bb: HashMap<usize, BoundingBox> = HashMap::default();
    for l in 0..lines.len() {
        let mut skip = false;
        if !el_x[l].is_empty() {
            let mut x = el_x[l].to_vec();
            let tailx = *el_x[l].first().unwrap();
            x.push(tailx);

            let mut y = el_y[l].to_vec();
            let taily = *el_y[l].first().unwrap();
            y.push(taily);

            let mut minx = f64::MAX;
            let mut miny = f64::MAX;
            let mut maxx = f64::MIN;
            let mut maxy = f64::MIN;

            for k in 0..x.len() {
                if x[k] > maxx {
                    maxx = x[k]
                }
                if x[k] < minx {
                    minx = x[k]
                }
                if y[k] > maxy {
                    maxy = y[k]
                }
                if y[k] < miny {
                    miny = y[k]
                }
            }
            bb.insert(
                l,
                BoundingBox {
                    minx,
                    maxx,
                    miny,
                    maxy,
                },
            );

            for head in heads.iter() {
                let &Head { id, xtest, ytest } = head;

                if !skip
                    && *elevation.get(&id).unwrap() > *elevation.get(&(l as u64)).unwrap()
                    && id != (l as u64)
                    && xtest < maxx
                    && xtest > minx
                    && ytest < maxy
                    && ytest > miny
                {
                    let mut hit = 0;
                    let mut n = 0;
                    let mut x0 = 0.0;
                    let mut y0 = 0.0;
                    while n < x.len() {
                        let x1 = x[n];
                        let y1 = y[n];

                        if n > 0
                            && ((y0 <= ytest && ytest < y1) || (y1 <= ytest && ytest < y0))
                            && (xtest < ((x1 - x0) * (ytest - y0) / (y1 - y0) + x0))
                        {
                            hit += 1;
                        }
                        x0 = x1;
                        y0 = y1;
                        n += 1;
                    }
                    if hit % 2 == 1 {
                        skip = true;
                    }
                }
            }
            if !skip {
                tops.push(Top {
                    id: l as u64,
                    xtest: x[0],
                    ytest: y[0],
                });
            }
        }
    }
    struct Candidate {
        id: u64,
        xtest: f64,
        ytest: f64,
        topid: u64,
    }
    let mut canditates = Vec::<Candidate>::new();

    for l in 0..lines.len() {
        let mut skip = true;
        if !el_x[l].is_empty() {
            let mut x = el_x[l].to_vec();
            let tailx = *el_x[l].first().unwrap();
            x.push(tailx);

            let mut y = el_y[l].to_vec();
            let taily = *el_y[l].first().unwrap();
            y.push(taily);

            let &BoundingBox {
                minx,
                maxx,
                miny,
                maxy,
            } = bb.get(&l).unwrap();

            let mut topid = 0;
            for head in tops.iter() {
                let &Top { id, xtest, ytest } = head;
                let ll = l as u64;

                if *elevation.get(&ll).unwrap() < (*elevation.get(&id).unwrap() - 0.1)
                    && *elevation.get(&ll).unwrap() > (*elevation.get(&id).unwrap() - 4.6)
                    && skip
                    && xtest < maxx
                    && xtest > minx
                    && ytest < maxy
                    && ytest > miny
                {
                    let mut hit = 0;
                    let mut n = 0;

                    let mut x0 = 0.0;
                    let mut y0 = 0.0;
                    while n < x.len() {
                        let x1 = x[n];
                        let y1 = y[n];

                        if n > 0
                            && ((y0 <= ytest && ytest < y1) || (y1 <= ytest && ytest < y0))
                            && (xtest < ((x1 - x0) * (ytest - y0) / (y1 - y0) + x0))
                        {
                            hit += 1;
                        }
                        x0 = x1;
                        y0 = y1;

                        n += 1;
                    }
                    if hit % 2 == 1 {
                        skip = false;
                        topid = id;
                    }
                }
            }
            if !skip {
                canditates.push(Candidate {
                    id: l as u64,
                    xtest: x[0],
                    ytest: y[0],
                    topid,
                });
            } else {
                el_x[l].clear();
                el_y[l].clear();
            }
        }
    }

    let mut best: HashMap<u64, u64> = HashMap::default();
    let mut mov: HashMap<u64, f64> = HashMap::default();

    for head in canditates.iter() {
        let &Candidate { id, topid, .. } = head;
        let el = *elevation.get(&id).unwrap();
        let test = (el / halfinterval + 1.0).floor() * halfinterval - el;

        if !best.contains_key(&topid) {
            best.insert(topid, id);
            mov.insert(id, test);
        } else {
            let tid = *best.get(&topid).unwrap();
            if *mov.get(&tid).unwrap() < 1.75 * contours_ratio
                && (*elevation.get(&topid).unwrap()
                    - *elevation.get(&tid).unwrap()
                    - 0.6 * contours_ratio)
                    .abs()
                    < 0.2
            {
                // no action
            } else if *mov.get(&tid).unwrap() > test {
                best.insert(topid, id);
                mov.insert(id, test);
            }
        }
    }
    let mut new_candidates = Vec::<Candidate>::new();
    for head in canditates.iter() {
        let &Candidate {
            id,
            xtest,
            ytest,
            topid,
        } = head;

        let x = el_x[id as usize].to_vec();
        if *best.get(&topid).unwrap() == id
            && (x.len() < 13
                || (*elevation.get(&topid).unwrap() > (*elevation.get(&id).unwrap() + 0.45)
                    || (*elevation.get(&id).unwrap()
                        - 2.5
                            * contours_ratio
                            * (*elevation.get(&id).unwrap() / (2.5 * contours_ratio)).floor())
                        > 0.45))
        {
            new_candidates.push(Candidate {
                id,
                xtest,
                ytest,
                topid,
            });
        } else {
            el_x[id as usize].clear();
            el_y[id as usize].clear();
        }
    }

    let canditates = new_candidates;

    let mut pins = Vec::new();

    for l in 0..lines.len() {
        let mut skip = false;
        let ll = l as u64;
        let mut ltopid = 0;
        if !el_x[l].is_empty() {
            let mut x = el_x[l].to_vec();
            let tailx = *el_x[l].first().unwrap();
            x.push(tailx);

            let mut y = el_y[l].to_vec();
            let taily = *el_y[l].first().unwrap();
            y.push(taily);

            let &BoundingBox {
                minx,
                maxx,
                miny,
                maxy,
            } = bb.get(&l).unwrap();

            for head in canditates.iter() {
                let &Candidate {
                    id,
                    xtest,
                    ytest,
                    topid,
                } = head;

                ltopid = topid;
                if id != ll && !skip && xtest < maxx && xtest > minx && ytest < maxy && ytest > miny
                {
                    let mut hit = 0;
                    let mut n = 0;

                    let mut x0 = 0.0;
                    let mut y0 = 0.0;
                    while n < x.len() {
                        let x1 = x[n];
                        let y1 = y[n];

                        if n > 0
                            && ((y0 <= ytest && ytest < y1) || (y1 <= ytest && ytest < y0))
                            && (xtest < ((x1 - x0) * (ytest - y0) / (y1 - y0) + x0))
                        {
                            hit += 1;
                        }
                        x0 = x1;
                        y0 = y1;

                        n += 1;
                    }
                    if hit % 2 == 1 {
                        skip = true;
                    }
                }
            }

            if !skip {
                let line = x
                    .iter()
                    .zip(y.iter())
                    .map(|(x, y)| Point2::new(*x, *y))
                    .collect::<Vec<_>>();
                detected_lines.push(line, Classification::Knoll1010);

                let mut xa = 0.0;
                let mut ya = 0.0;
                for k in 0..x.len() {
                    xa += x[k];
                    ya += y[k];
                }
                let xlen = x.len() as f64;
                xa /= xlen;
                ya /= xlen;

                x.push(x[0]);
                y.push(y[0]);
                pins.push(Pin {
                    xx: xa,
                    yy: ya,
                    ele: *elevation.get(&ll).unwrap(),
                    ele2: *elevation.get(&ltopid).unwrap(),
                    xlist: x,
                    ylist: y,
                });
            } else {
                el_x[l].clear();
                el_y[l].clear();
            }
        }
    }

    let detected_dxf = BinaryDxf::new(detected_bounds, vec![detected_lines.into()]);
    detected_dxf.to_writer(&mut fs.create(tmpfolder.join("detected.dxf.bin"))?)?;

    if config.output_dxf {
        detected_dxf.to_dxf(&mut fs.create(tmpfolder.join("detected.dxf"))?)?;
    }

    // write pins to file
    let file_pins = fs
        .create(tmpfolder.join("pins.bin"))
        .expect("Unable to create file");
    crate::util::write_object(file_pins, &pins).expect("Unable to write pins");

    info!("Done");
    Ok(())
}

/// Struct used to store temporary data about pins on disk
#[derive(serde::Serialize, serde::Deserialize)]
struct Pin {
    xx: f64,
    yy: f64,
    ele: f64,
    ele2: f64,
    xlist: Vec<f64>,
    ylist: Vec<f64>,
}

pub fn xyzknolls(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
) -> Result<(), Box<dyn Error>> {
    info!("Identifying knolls...");
    let scalefactor = config.scalefactor;
    let contour_interval = config.contour_interval;

    let interval = contour_interval / 2.0 * scalefactor;

    // load the binary file
    let heightmap_in = tmpfolder.join("xyz_03.hmap");
    let hmap = HeightMap::from_bytes(&mut fs.open(heightmap_in)?)?;

    let xmax = hmap.grid.width() - 1;
    let ymax = hmap.grid.height() - 1;
    let size = hmap.scale;
    let xstart = hmap.xoffset;
    let ystart = hmap.yoffset;

    let mut xyz2 = hmap.clone();

    for i in 2..=(xmax - 2) {
        for j in 2..=(ymax - 2) {
            let mut low = f64::MAX;
            let mut high = f64::MIN;
            let mut val = 0.0;
            let mut count = 0;
            for ii in (i - 2)..=(i + 2) {
                for jj in (j - 2)..=(j + 2) {
                    let tmp = hmap.grid[(ii, jj)];
                    if tmp < low {
                        low = tmp;
                    }
                    if tmp > high {
                        high = tmp;
                    }
                    count += 1;
                    val += tmp;
                }
            }
            let steepness = high - low;
            if steepness < 1.25 {
                let tmp = (1.25 - steepness) * (val - low - high) / (count as f64 - 2.0) / 1.25
                    + steepness * xyz2.grid[(i, j)] / 1.25;
                xyz2.grid[(i, j)] = tmp;
            }
        }
    }

    // read pins from file if it exists
    let pins_file_in = tmpfolder.join("pins.bin");
    let pins: Vec<Pin> = if fs.exists(&pins_file_in) {
        let pins_file_in = fs.open(pins_file_in).expect("Unable to open file");
        crate::util::read_object(pins_file_in).expect("Unable to write pins")
    } else {
        Vec::new()
    };

    // compute closest distance from each pin to another pin
    let mut dist: HashMap<usize, f64> = HashMap::default();
    for (l, pin) in pins.iter().enumerate() {
        let mut min = f64::MAX;
        let xx = ((pin.xx - xstart) / size).floor();
        let yy = ((pin.yy - ystart) / size).floor();
        for (k, pin2) in pins.iter().enumerate() {
            if k == l {
                continue;
            }

            let xx2 = ((pin2.xx - xstart) / size).floor();
            let yy2 = ((pin2.yy - ystart) / size).floor();
            let mut dis = (xx2 - xx).abs();
            let disy = (yy2 - yy).abs();
            if disy > dis {
                dis = disy;
            }
            if dis < min {
                min = dis;
            }
        }
        dist.insert(l, min);
    }

    for (l, line) in pins.into_iter().enumerate() {
        let Pin {
            xx,
            yy,
            ele,
            ele2,
            xlist: mut x,
            ylist: mut y,
        } = line;

        let elenew = ((ele - 0.09) / interval + 1.0).floor() * interval;
        let mut move1 = elenew - ele + 0.15;
        let mut move2 = move1 * 0.4;
        if move1 > 0.66 * interval {
            move2 = move1 * 0.6;
        }
        if move1 < 0.25 * interval {
            move2 = 0.0;
            move1 += 0.3;
        }
        move1 += 0.5;
        if ele2 + move1 > ((ele - 0.09) / interval + 2.0).floor() * interval {
            move1 -= 0.4;
        }
        if elenew - ele > 1.5 * interval / 2.5 * scalefactor && x.len() > 21 {
            for k in 0..x.len() {
                x[k] = xx + (x[k] - xx) * 0.8;
                y[k] = yy + (y[k] - yy) * 0.8;
            }
        }
        let mut touched: HashMap<String, bool> = HashMap::default();
        let mut minx = u64::MAX;
        let mut miny = u64::MAX;
        let mut maxx = u64::MIN;
        let mut maxy = u64::MIN;
        for k in 0..x.len() {
            x[k] = ((x[k] - xstart) / size + 0.5).floor();
            y[k] = ((y[k] - ystart) / size + 0.5).floor();
            let xk = x[k] as u64;
            let yk = y[k] as u64;
            if xk > maxx {
                maxx = xk;
            }
            if yk > maxy {
                maxy = yk;
            }
            if xk < minx {
                minx = xk;
            }
            if yk < miny {
                miny = yk;
            }
        }

        let xx = ((xx - xstart) / size).floor();
        let yy = ((yy - ystart) / size).floor();

        let mut x0 = 0.0;
        let mut y0 = 0.0;

        for ii in minx as usize..(maxx as usize + 1) {
            for jj in miny as usize..(maxy as usize + 1) {
                let mut hit = 0;
                let xtest = ii as f64;
                let ytest = jj as f64;
                for n in 0..x.len() {
                    let x1 = x[n];
                    let y1 = y[n];
                    if n > 1
                        && ((y0 <= ytest && ytest < y1) || (y1 <= ytest && ytest < y0))
                        && xtest < (x1 - x0) * (ytest - y0) / (y1 - y0) + x0
                    {
                        hit += 1;
                    }
                    x0 = x1;
                    y0 = y1;
                }
                if hit % 2 == 1 {
                    let tmp = xyz2.grid[(ii, jj)] + move1;
                    xyz2.grid[(ii, jj)] = tmp;
                    let coords = format!("{ii}_{jj}");
                    touched.insert(coords, true);
                }
            }
        }
        let mut range = *dist.get(&l).unwrap_or(&0.0) * 0.8 - 1.0;
        range = range.clamp(1.0, 12.0);

        for iii in 0..((range * 2.0 + 1.0) as usize) {
            for jjj in 0..((range * 2.0 + 1.0) as usize) {
                let ii: f64 = xx - range + iii as f64;
                let jj: f64 = yy - range + jjj as f64;
                if ii > 0.0 && ii < xmax as f64 && jj > 0.0 && jj < ymax as f64 {
                    let coords = format!("{ii}_{jj}");
                    if !*touched.get(&coords).unwrap_or(&false) {
                        xyz2.grid[(ii as usize, jj as usize)] +=
                            (range - (xx - ii).abs()) / range * (range - (yy - jj).abs()) / range
                                * move2;
                    }
                }
            }
        }
    }

    // As per https://github.com/karttapullautin/karttapullautin/discussions/154#discussioncomment-11393907
    // If elevation grid point elavion equals with contour interval steps you will get contour topology issues
    // (crossing/touching contours). This was implemented to avoid that. 0.02 (two centimeters) is just a random
    // small number to avoid that issue, insignificant enough to matter, but big buffer enough to hopefully make
    // it not get back to "bad value" for it getting rounded somewhere. Sure, it could be some fraction of
    // contour interval, but in real world 2 cm is insignificant enough.
    for (_, _, h) in xyz2.grid.iter_mut() {
        let tmp = (*h / interval + 0.5).floor() * interval;
        if (tmp - *h).abs() < ELEVATION_SNAP_M {
            if *h - tmp < 0.0 {
                *h = tmp - ELEVATION_SNAP_M;
            } else {
                *h = tmp + ELEVATION_SNAP_M;
            }
        }
    }

    // write the updated heightmap
    let heightmap_out = tmpfolder.join("xyz_knolls.hmap");
    let mut writer = fs.create(heightmap_out)?;
    xyz2.to_bytes(&mut writer)?;

    info!("Done");
    Ok(())
}
