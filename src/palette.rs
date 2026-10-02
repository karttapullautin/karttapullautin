use image::{Luma, Rgb, Rgba};

use crate::config::Config;

/// Number of entries in the indexed PNG palette.
const PALETTE_SIZE: usize = 256;

/// Maximum number of green shades; this many palette entries are reserved for them.
const MAX_GREEN_SHADES: usize = 64;

/// Palette index of the first green shade.
const GREEN_SHADE_PALETTE_OFFSET: u8 = 16;

// All colours used by the renderers are defined here.

pub const WHITE_RGB: Rgb<u8> = Rgb([255, 255, 255]);
pub const BLACK_RGB: Rgb<u8> = Rgb([0, 0, 0]);
pub const WHITE_RGBA: Rgba<u8> = Rgba([255, 255, 255, 255]);
pub const BLACK_RGBA: Rgba<u8> = Rgba([0, 0, 0, 255]);
pub const TRANSPARENT_WHITE_RGBA: Rgba<u8> = Rgba([255, 255, 255, 0]);
pub const TRANSPARENT_BLACK_RGBA: Rgba<u8> = Rgba([0, 0, 0, 0]);
pub const BLACK_LUMA: Luma<u8> = Luma([0]);

pub const BLUE_RGBA: Rgba<u8> = Rgba([29, 190, 255, 255]);
pub const YELLOW2_RGBA: Rgba<u8> = Rgba([255, 219, 166, 255]);
pub const UNDERGROWTH_RGBA: Rgba<u8> = Rgba([64, 121, 0, 255]);

/// Colour of contours and dot knolls in the contour render.
pub const CONTOUR_BROWN_RGBA: Rgba<u8> = Rgba([166, 85, 43, 255]);
/// Colour of the north lines.
pub const NORTH_LINE_RGBA: Rgba<u8> = Rgba([0, 0, 200, 255]);

/// Cliff colours used when `cliffdebug` is enabled.
pub const CLIFF2_DEBUG_RGBA: Rgba<u8> = Rgba([100, 0, 100, 255]);
pub const CLIFF3_DEBUG_RGBA: Rgba<u8> = Rgba([0, 100, 100, 255]);
pub const CLIFF4_DEBUG_RGBA: Rgba<u8> = Rgba([100, 100, 0, 255]);

/// Colours used by the shapefile renderer.
pub const SHAPEFILE_BROWN_RGBA: Rgba<u8> = Rgba([255, 150, 80, 255]);
pub const SHAPEFILE_YELLOW_RGBA: Rgba<u8> = Rgba([255, 184, 83, 255]);
pub const SHAPEFILE_MARSH_RGBA: Rgba<u8> = Rgba([0, 10, 220, 255]);
pub const SHAPEFILE_OLIVE_RGBA: Rgba<u8> = Rgba([194, 176, 33, 255]);

/// Our own wrapper around and image buffer that automatically handles drawing with a palette.
#[derive(Clone)]
pub struct PalettedImage {
    /// the inner paletted image
    image: image::ImageBuffer<PaletteColor, Vec<u8>>,
}

impl imageproc::drawing::Canvas for PalettedImage {
    type Pixel = PaletteColor;

    fn dimensions(&self) -> (u32, u32) {
        self.image.dimensions()
    }

    fn get_pixel(&self, x: u32, y: u32) -> Self::Pixel {
        *self.image.get_pixel(x, y)
    }

    fn draw_pixel(&mut self, x: u32, y: u32, color: Self::Pixel) {
        // custom logic to honor transparency Also ignore any background white
        if color == PaletteColorEnum::Transparent.to_color()
            || color == PaletteColorEnum::BackgroundWhite.to_color()
        {
            return;
        }
        self.image.put_pixel(x, y, color);
    }
}

impl PalettedImage {
    pub fn new(width: u32, height: u32, fill: PaletteColor) -> Self {
        let mut image = image::ImageBuffer::new(width, height);

        for p in image.pixels_mut() {
            *p = fill;
        }

        Self { image }
    }

    pub fn width(&self) -> u32 {
        self.image.width()
    }
    pub fn height(&self) -> u32 {
        self.image.height()
    }

    pub fn median_filter(&self, x_radius: u32, y_radius: u32) -> Self {
        let filtered_image = imageproc::filter::median_filter(&self.image, x_radius, y_radius);
        Self {
            image: filtered_image,
        }
    }

    pub fn draw_filled_rect(&mut self, rect: imageproc::rect::Rect, color: PaletteColor) {
        imageproc::drawing::draw_filled_rect_mut(&mut self.image, rect, color);
    }

    pub fn pixels(&self) -> impl Iterator<Item = &PaletteColor> {
        self.image.pixels()
    }

    /// Overlay other ontop of this image, blending the colors (taking top color if not transparent)
    pub fn overlay(&mut self, top: &Self, x: i64, y: i64) {
        image::imageops::overlay(&mut self.image, &top.image, x, y);
    }

    /// writes an indexed PNG to the specified writer
    pub fn write_to<W>(&self, writer: &mut W, palette: &Palette) -> anyhow::Result<()>
    where
        W: std::io::Write + std::io::Seek,
    {
        let mut encoder = png::Encoder::new(writer, self.image.width(), self.image.height());
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);

        // create and fill palette array from provided palette
        let mut palette_bytes = [0; PALETTE_SIZE * 3];
        let mut transparency_bytes = [0; PALETTE_SIZE];

        for (i, color) in palette.colors.iter().enumerate() {
            palette_bytes[i * 3] = color[0];
            palette_bytes[i * 3 + 1] = color[1];
            palette_bytes[i * 3 + 2] = color[2];
            transparency_bytes[i] = color[3];
        }
        encoder.set_palette(&palette_bytes);
        encoder.set_trns(&transparency_bytes);

        let mut w = encoder.write_header().expect("Failed to write PNG header");
        w.write_image_data(self.image.as_raw())
            .expect("Failed to write PNG data");
        Ok(())
    }
}

/// Contains the global color palette for rendering.
pub struct Palette {
    colors: [image::Rgba<u8>; PALETTE_SIZE],
}

impl std::ops::Index<PaletteColorEnum> for Palette {
    type Output = image::Rgba<u8>;

    fn index(&self, index: PaletteColorEnum) -> &Self::Output {
        &self.colors[index.to_color().0[0] as usize]
    }
}

impl std::ops::IndexMut<PaletteColorEnum> for Palette {
    fn index_mut(&mut self, index: PaletteColorEnum) -> &mut Self::Output {
        &mut self.colors[index.to_color().0[0] as usize]
    }
}

impl Palette {
    pub fn new(config: &Config) -> Self {
        let colors = [TRANSPARENT_BLACK_RGBA; PALETTE_SIZE];

        let mut palette = Self { colors };

        // initialize the palette with the colors from the config, and any hard-coded colors

        palette[PaletteColorEnum::Transparent] = TRANSPARENT_WHITE_RGBA;

        palette[PaletteColorEnum::Yellow2] = YELLOW2_RGBA;

        {
            let num_greenshades = config.greenshades.len();
            let greentone = config.greentone;

            assert!(
                num_greenshades <= MAX_GREEN_SHADES,
                "Number of green shades must be between 0 and {MAX_GREEN_SHADES}"
            );

            for i in 0..num_greenshades {
                palette[PaletteColorEnum::GreenShade(i as u8)] = Rgba([
                    (greentone - greentone / (num_greenshades - 1) as f64 * i as f64) as u8,
                    (254.0 - (74.0 / (num_greenshades - 1) as f64) * i as f64) as u8,
                    (greentone - greentone / (num_greenshades - 1) as f64 * i as f64) as u8,
                    255,
                ]);
            }
        }

        palette[PaletteColorEnum::BackgroundWhite] = WHITE_RGBA;
        palette[PaletteColorEnum::Black] = BLACK_RGBA;
        palette[PaletteColorEnum::Blue] = BLUE_RGBA;
        palette[PaletteColorEnum::Undergrowth] = UNDERGROWTH_RGBA;

        palette
    }
}

#[repr(u8)]
pub enum PaletteColorEnum {
    /// Transparent background color.
    Transparent,
    BackgroundWhite,
    Black,
    /// Yellow used for open areas.
    Yellow2,
    Undergrowth,
    Blue,
    /// A shade of green used for vegetation. Number of shades are configured by the config. Maximum
    /// 64 shades, so we have to reserve 64 colors for this.
    GreenShade(u8),
}

impl PaletteColorEnum {
    pub const fn to_color(&self) -> PaletteColor {
        match self {
            Self::Transparent => PaletteColor(Luma([0])),
            Self::BackgroundWhite => PaletteColor(Luma([1])),
            Self::Black => PaletteColor(Luma([2])),
            Self::Yellow2 => PaletteColor(Luma([3])),
            Self::Blue => PaletteColor(Luma([4])),
            Self::Undergrowth => PaletteColor(Luma([5])),

            Self::GreenShade(shade) => {
                assert!(
                    (*shade as usize) < MAX_GREEN_SHADES,
                    "Green shade exceeds the maximum number of green shades"
                );
                PaletteColor(Luma([GREEN_SHADE_PALETTE_OFFSET + *shade]))
            }
        }
    }
}

/// The index of a color in the palette. The palette contains up to 256 colors, so this is a single byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct PaletteColor(image::Luma<u8>);

#[allow(unused_variables)]
impl image::Pixel for PaletteColor {
    type Subpixel = u8;

    const CHANNEL_COUNT: u8 = 1;
    const HAS_ALPHA: bool = false;

    fn channels(&self) -> &[Self::Subpixel] {
        self.0.channels()
    }

    fn channels_mut(&mut self) -> &mut [Self::Subpixel] {
        self.0.channels_mut()
    }

    const COLOR_MODEL: &'static str = "Palette";

    fn channels4(
        &self,
    ) -> (
        Self::Subpixel,
        Self::Subpixel,
        Self::Subpixel,
        Self::Subpixel,
    ) {
        #[allow(deprecated)]
        self.0.channels4()
    }

    fn from_channels(
        a: Self::Subpixel,
        b: Self::Subpixel,
        c: Self::Subpixel,
        d: Self::Subpixel,
    ) -> Self {
        #[allow(deprecated)]
        Self(Luma::<u8>::from_channels(a, b, c, d))
    }

    fn from_slice(slice: &[Self::Subpixel]) -> &Self {
        assert_eq!(slice.len(), 1);
        // SAFETY: We have asserted that the slice has the correct length, and we use
        // repr(transparent) on an object that does exactly this internally, so it is safe
        // reinterpret it as a reference to Self.
        unsafe { &*(slice.as_ptr() as *const Self) }
    }

    fn from_slice_mut(slice: &mut [Self::Subpixel]) -> &mut Self {
        assert_eq!(slice.len(), 1);
        // SAFETY: We have asserted that the slice has the correct length, and we use
        // repr(transparent) on an object that does exactly this internally, so it is safe
        // reinterpret it as a reference to Self.
        unsafe { &mut *(slice.as_mut_ptr() as *mut Self) }
    }

    fn to_rgb(&self) -> image::Rgb<Self::Subpixel> {
        self.0.to_rgb()
    }

    fn to_rgba(&self) -> image::Rgba<Self::Subpixel> {
        self.0.to_rgba()
    }

    fn to_luma(&self) -> image::Luma<Self::Subpixel> {
        self.0.to_luma()
    }

    fn to_luma_alpha(&self) -> image::LumaA<Self::Subpixel> {
        self.0.to_luma_alpha()
    }

    fn map<F>(&self, f: F) -> Self
    where
        F: FnMut(Self::Subpixel) -> Self::Subpixel,
    {
        unimplemented!()
    }

    fn apply<F>(&mut self, f: F)
    where
        F: FnMut(Self::Subpixel) -> Self::Subpixel,
    {
        unimplemented!()
    }

    fn map_with_alpha<F, G>(&self, f: F, g: G) -> Self
    where
        F: FnMut(Self::Subpixel) -> Self::Subpixel,
        G: FnMut(Self::Subpixel) -> Self::Subpixel,
    {
        unimplemented!()
    }

    fn apply_with_alpha<F, G>(&mut self, f: F, g: G)
    where
        F: FnMut(Self::Subpixel) -> Self::Subpixel,
        G: FnMut(Self::Subpixel) -> Self::Subpixel,
    {
        unimplemented!()
    }

    fn map2<F>(&self, other: &Self, f: F) -> Self
    where
        F: FnMut(Self::Subpixel, Self::Subpixel) -> Self::Subpixel,
    {
        unimplemented!()
    }

    fn apply2<F>(&mut self, other: &Self, f: F)
    where
        F: FnMut(Self::Subpixel, Self::Subpixel) -> Self::Subpixel,
    {
        unimplemented!()
    }

    fn invert(&mut self) {
        unimplemented!()
    }

    fn blend(&mut self, other: &Self) {
        // if we are blending with a transparent color, we should not change the color at all
        if *other == PaletteColorEnum::Transparent.to_color()
            || *other == PaletteColorEnum::BackgroundWhite.to_color()
        {
            return;
        }

        *self = *other;
    }
}
