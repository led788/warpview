use anyhow::{Context, Result};
use image::{AnimationDecoder, DynamicImage, ImageDecoder};
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

/// Delays below this are treated as authoring mistakes (many GIF encoders
/// emit 0ms) and bumped up, matching common browser/viewer behavior.
const MIN_FRAME_DELAY: Duration = Duration::from_millis(20);
const DEFAULT_FRAME_DELAY: Duration = Duration::from_millis(100);

pub struct DecodedFrame {
    /// Tightly packed RGBA8 pixels, row-major.
    pub rgba: Vec<u8>,
    pub delay: Duration,
}

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Always has at least one frame. More than one means animated.
    pub frames: Vec<DecodedFrame>,
}

impl DecodedImage {
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }
}

pub fn decode(path: &Path) -> Result<DecodedImage> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "heic" | "heif" => decode_heif(path),
        "gif" => decode_gif(path),
        "png" | "apng" => decode_png(path),
        _ => decode_static_with_image_crate(path),
    }
}

fn decode_static_with_image_crate(path: &Path) -> Result<DecodedImage> {
    let img = image::open(path).with_context(|| format!("failed to decode {}", path.display()))?;
    let img = apply_exif_orientation(img, path);
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(DecodedImage {
        width,
        height,
        frames: vec![DecodedFrame {
            rgba: rgba.into_raw(),
            delay: Duration::ZERO,
        }],
    })
}

fn decode_gif(path: &Path) -> Result<DecodedImage> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(file))
        .with_context(|| format!("failed to decode {}", path.display()))?;
    let (width, height) = decoder.dimensions();
    let frames = collect_frames(decoder.into_frames())
        .with_context(|| format!("failed to decode {}", path.display()))?;
    Ok(DecodedImage {
        width,
        height,
        frames,
    })
}

fn decode_png(path: &Path) -> Result<DecodedImage> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let decoder = image::codecs::png::PngDecoder::new(BufReader::new(file))
        .with_context(|| format!("failed to decode {}", path.display()))?;

    if !decoder.is_apng()? {
        return decode_static_with_image_crate(path);
    }

    let (width, height) = decoder.dimensions();
    let apng = decoder.apng()?;
    let frames = collect_frames(apng.into_frames())
        .with_context(|| format!("failed to decode {}", path.display()))?;
    Ok(DecodedImage {
        width,
        height,
        frames,
    })
}

fn collect_frames(frames: image::Frames) -> Result<Vec<DecodedFrame>> {
    let mut out = Vec::new();
    for frame in frames {
        let frame = frame?;
        let delay = clamp_delay(frame.delay().into());
        let buffer = frame.into_buffer();
        out.push(DecodedFrame {
            rgba: buffer.into_raw(),
            delay,
        });
    }
    anyhow::ensure!(!out.is_empty(), "no frames decoded");
    Ok(out)
}

fn clamp_delay(delay: Duration) -> Duration {
    if delay < MIN_FRAME_DELAY {
        DEFAULT_FRAME_DELAY
    } else {
        delay
    }
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
        frames: vec![DecodedFrame {
            rgba,
            delay: Duration::ZERO,
        }],
    })
}
