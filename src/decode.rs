use anyhow::{Context, Result};
use image::DynamicImage;
use std::path::Path;

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8 pixels, row-major.
    pub rgba: Vec<u8>,
}

pub fn decode(path: &Path) -> Result<DecodedImage> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "heic" | "heif" => decode_heif(path),
        _ => decode_with_image_crate(path),
    }
}

fn decode_with_image_crate(path: &Path) -> Result<DecodedImage> {
    let img = image::open(path).with_context(|| format!("failed to decode {}", path.display()))?;
    let img = apply_exif_orientation(img, path);
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(DecodedImage {
        width,
        height,
        rgba: rgba.into_raw(),
    })
}

fn apply_exif_orientation(img: DynamicImage, path: &Path) -> DynamicImage {
    match read_exif_orientation(path) {
        Some(2) => img.fliph(),
        Some(3) => img.rotate180(),
        Some(4) => img.flipv(),
        Some(5) => img.rotate90().fliph(),
        Some(6) => img.rotate90(),
        Some(7) => img.rotate270().fliph(),
        Some(8) => img.rotate270(),
        _ => img,
    }
}

fn read_exif_orientation(path: &Path) -> Option<u32> {
    let file = std::fs::File::open(path).ok()?;
    let mut bufreader = std::io::BufReader::new(&file);
    let exif = exif::Reader::new()
        .read_from_container(&mut bufreader)
        .ok()?;
    let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
    field.value.get_uint(0)
}

fn decode_heif(path: &Path) -> Result<DecodedImage> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

    let lib_heif = LibHeif::new();
    let path_str = path.to_str().context("HEIC path is not valid UTF-8")?;
    let ctx = HeifContext::read_from_file(path_str)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let handle = ctx
        .primary_image_handle()
        .context("HEIC file has no primary image")?;
    let image = lib_heif
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
        .with_context(|| format!("failed to decode {}", path.display()))?;

    let planes = image.planes();
    let plane = planes
        .interleaved
        .context("expected interleaved RGBA plane from libheif")?;

    let width = plane.width;
    let height = plane.height;
    let stride = plane.stride;
    let data = plane.data;

    let row_bytes = width as usize * 4;
    let mut rgba = Vec::with_capacity(row_bytes * height as usize);
    for row in 0..height as usize {
        let start = row * stride;
        rgba.extend_from_slice(&data[start..start + row_bytes]);
    }

    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}
