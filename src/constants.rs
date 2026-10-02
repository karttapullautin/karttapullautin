//! Shared constants used throughout the crate.

/// The size of a megabyte in bytes. Used for Read/Write buffer sizes for large files, instead of
/// the default 8KB.
pub const ONE_MEGABYTE: usize = 1024 * 1024;

/// Memory budget for buffering points during LAZ -> XyzRecord conversion.
pub const LAZ_BUFFER_MEMORY_BYTES: usize = 50 * ONE_MEGABYTE;

/// Ground-metre padding added around a tile's bounds when extracting points from neighbouring LAZ
/// files.
pub const TILE_EXTRACTION_PADDING_M: f64 = 127.0;

/// Render pixels per ground metre at 600 dpi and a 1:10,000 map scale (before `scalefactor`).
pub const PX_PER_M: f64 = 600.0 / 254.0;

/// Extra pixels added to a crop canvas so float rounding/truncation never clips the overlaid image.
pub const CROP_CANVAS_MARGIN_PX: f64 = 2.0;

/// One pixel of the native vegetation/undergrowth-bit grid equals one ground metre.
pub const GRID_PIXEL_SIZE_M: f64 = 1.0;

/// Elevation grid points this close (in metres) to a contour level are nudged off it, to avoid
/// crossing/touching contours.
pub const ELEVATION_SNAP_M: f64 = 0.02;

/// Same as [`ELEVATION_SNAP_M`], but applied to the corners of the contour cells.
pub const CONTOUR_CORNER_SNAP_M: f64 = 0.05;

/// Added to `smoothing` to avoid dividing by zero when smoothing contours.
pub const SMOOTHING_EPSILON: f64 = 0.01;

/// Added to denominators when computing hit ratios, to avoid dividing by zero.
pub const RATIO_EPSILON: f64 = 0.01;

/// Line width of intermediate contours; also used to recognise them when drawing.
pub const INTERMEDIATE_CONTOUR_WIDTH: f64 = 1.5;

/// Coordinates are quantised to millimetres when used as hash keys.
pub const FIXED_POINT_SCALE: f64 = 1000.0;

/// Shapefile road line widths, in pixels.
pub const ROAD_WIDTH_PX: f32 = 20.0;
pub const ROAD_BRIDGE_WIDTH_PX: f32 = 14.0;
pub const ROAD_EDGE_WIDTH_PX: f32 = 26.0;

/// ISOM symbol code for marshes in the shapefile mapping.
pub const ISOM_MARSH: &str = "306";
