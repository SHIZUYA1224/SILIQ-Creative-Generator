//! Raster asset decoding (AST-001/003). The format is identified from the
//! bytes, must match the declared mimeHint, and only the matching Skia
//! decoder is offered. Dimensions are checked before pixels are decoded.

use design_model::{Code, Issue, MimeHint};
use serde::Serialize;
use skia_safe::codec::{EncodedOrigin, jpeg_decoder, png_decoder};
use skia_safe::{AlphaType, Codec, ColorSpace, ColorType, Data, Image, ImageInfo, images};
use std::io::Cursor;

pub const MAX_IMAGE_SIDE: i32 = 16_384;
pub const MAX_IMAGE_PIXELS: i64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sniffed {
    Png,
    Jpeg,
    Webp,
    Svg,
    Unknown,
}

pub fn sniff(bytes: &[u8]) -> Sniffed {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Sniffed::Png
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Sniffed::Jpeg
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Sniffed::Webp
    } else {
        let head = &bytes[..bytes.len().min(512)];
        let head = String::from_utf8_lossy(head);
        let t = head.trim_start_matches('\u{feff}').trim_start();
        if t.starts_with("<svg") || t.starts_with("<?xml") {
            Sniffed::Svg
        } else {
            Sniffed::Unknown
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedInfo {
    pub width: i32,
    pub height: i32,
    /// "sRGB" when the file is tagged sRGB, "untagged (assumed sRGB)" when it
    /// carries no colour information, "converted from embedded profile" otherwise.
    pub source_color: String,
    /// Source EXIF orientation; normalized exactly once during import.
    pub orientation_applied: u8,
}

pub fn decode(bytes: &[u8], declared: MimeHint) -> Result<(Image, DecodedInfo), Issue> {
    if bytes.len() > 100 * 1024 * 1024 {
        return Err(Issue::error(
            Code::ResourceLimit,
            "compressed image exceeds 100 MiB",
            "Use a smaller image.",
        ));
    }
    let sniffed = sniff(bytes);
    let expected = match declared {
        MimeHint::Png => Sniffed::Png,
        MimeHint::Jpeg => Sniffed::Jpeg,
        MimeHint::Webp => Sniffed::Webp,
        MimeHint::Svg => Sniffed::Svg,
    };
    if sniffed != expected {
        return Err(Issue::error(
            Code::AssetType,
            format!("file content is {sniffed:?} but the asset is declared as {declared:?}"),
            "Import the file again so that its declared type matches its content.",
        ));
    }
    if sniffed == Sniffed::Webp {
        return decode_webp(bytes);
    }
    let decoder = match sniffed {
        Sniffed::Png => png_decoder::decoder(),
        Sniffed::Jpeg => jpeg_decoder::decoder(),
        _ => return Err(Issue::error(Code::AssetType, "not a raster image", "Use PNG or JPEG.")),
    };
    let mut codec = Codec::from_data_with_decoders(Data::new_copy(bytes), &[decoder]).ok_or_else(|| {
        Issue::error(
            Code::AssetType,
            "image header could not be decoded",
            "Use an intact PNG or JPEG file.",
        )
    })?;

    let dims = codec.dimensions();
    if dims.width <= 0
        || dims.height <= 0
        || dims.width > MAX_IMAGE_SIDE
        || dims.height > MAX_IMAGE_SIDE
        || (dims.width as i64) * (dims.height as i64) > MAX_IMAGE_PIXELS
    {
        return Err(Issue::error(
            Code::ResourceLimit,
            format!(
                "image is {}×{} px; limit is {MAX_IMAGE_SIDE} px per side and 64 MP",
                dims.width, dims.height
            ),
            "Use a smaller image.",
        ));
    }
    if codec.get_frame_count() > 1 {
        return Err(Issue::error(
            Code::AssetType,
            "animated images are not supported",
            "Use a still image.",
        ));
    }
    let orientation = match codec.origin() {
        EncodedOrigin::TopLeft => 1,
        EncodedOrigin::TopRight => 2,
        EncodedOrigin::BottomRight => 3,
        EncodedOrigin::BottomLeft => 4,
        EncodedOrigin::LeftTop => 5,
        EncodedOrigin::RightTop => 6,
        EncodedOrigin::RightBottom => 7,
        EncodedOrigin::LeftBottom => 8,
    };

    let decoded_space = codec.info().color_space();
    if embedded_profile_present(bytes, sniffed) && decoded_space.is_none() {
        return Err(decode_error(
            "embedded ICC profile could not be interpreted; it is not treated as an untagged image",
        ));
    }
    let source_color = match decoded_space {
        None => "untagged (assumed sRGB)".to_owned(),
        Some(cs) if cs.is_srgb() => "sRGB".to_owned(),
        Some(_) => "converted from embedded profile to sRGB".to_owned(),
    };
    let info = ImageInfo::new(dims, ColorType::RGBA8888, AlphaType::Premul, ColorSpace::new_srgb());
    let image = codec.get_image(info, None).map_err(|r| {
        Issue::error(
            Code::AssetType,
            format!("image pixels could not be decoded ({r:?})"),
            "Use an intact PNG or JPEG file.",
        )
    })?;
    let image = orient_image(image, orientation)?;
    let decoded = DecodedInfo {
        width: image.width(),
        height: image.height(),
        source_color,
        orientation_applied: orientation,
    };
    Ok((image, decoded))
}

fn decode_error(message: impl Into<String>) -> Issue {
    Issue::error(Code::AssetType, message, "Use an intact supported static image and valid metadata.")
}
fn decode_webp(bytes: &[u8]) -> Result<(Image, DecodedInfo), Issue> {
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| decode_error(format!("WebP header: {e}")))?;
    decoder.set_memory_limit(512 * 1024 * 1024);
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || w > MAX_IMAGE_SIDE as u32 || h > MAX_IMAGE_SIDE as u32 || u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS as u64 {
        return Err(Issue::error(
            Code::ResourceLimit,
            "WebP dimensions exceed the raster limit",
            "Use a smaller image.",
        ));
    }
    if decoder.is_animated() {
        return Err(decode_error("animated WebP is not supported"));
    }
    let profile = decoder.icc_profile().map_err(|e| decode_error(format!("WebP ICC metadata: {e}")))?;
    let (color_space, source_color) = match profile {
        Some(bytes) => {
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(Issue::error(
                    Code::ResourceLimit,
                    "ICC profile exceeds 4 MiB",
                    "Use a smaller valid profile.",
                ));
            }
            let cs = ColorSpace::new_icc(&bytes).ok_or_else(|| decode_error("invalid WebP ICC profile"))?;
            let description = if cs.is_srgb() {
                "sRGB"
            } else {
                "converted from embedded profile to sRGB"
            };
            (cs, description.to_owned())
        }
        None => (ColorSpace::new_srgb(), "untagged (assumed sRGB)".into()),
    };
    let orientation = match decoder
        .exif_metadata()
        .map_err(|e| decode_error(format!("WebP EXIF metadata: {e}")))?
    {
        Some(bytes) => exif_orientation(&bytes)?,
        None => 1,
    };
    let alpha = decoder.has_alpha();
    let len = decoder
        .output_buffer_size()
        .ok_or_else(|| decode_error("WebP pixel length overflow"))?;
    if len > 256 * 1024 * 1024 {
        return Err(Issue::error(
            Code::ResourceLimit,
            "WebP pixel buffer exceeds limit",
            "Use a smaller image.",
        ));
    }
    let mut raw = vec![0u8; len];
    decoder
        .read_image(&mut raw)
        .map_err(|e| decode_error(format!("WebP pixels: {e}")))?;
    let rgba = if alpha {
        raw
    } else {
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for rgb in raw.chunks_exact(3) {
            rgba.extend_from_slice(rgb);
            rgba.push(255);
        }
        drop(raw);
        rgba
    };
    let src_info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Unpremul, color_space);
    let image = images::raster_from_data(&src_info, Data::new_copy(&rgba), w as usize * 4)
        .ok_or_else(|| decode_error("WebP image allocation failed"))?;
    drop(rgba);
    let dest_info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Premul, ColorSpace::new_srgb());
    let mut normalized = vec![0u8; w as usize * h as usize * 4];
    if !image.read_pixels(
        &dest_info,
        &mut normalized,
        w as usize * 4,
        (0, 0),
        skia_safe::image::CachingHint::Disallow,
    ) {
        return Err(decode_error("WebP color conversion failed"));
    }
    drop(image);
    let image = images::raster_from_data(&dest_info, Data::new_copy(&normalized), w as usize * 4)
        .ok_or_else(|| decode_error("WebP normalized image allocation failed"))?;
    drop(normalized);
    let image = orient_image(image, orientation)?;
    let info = DecodedInfo {
        width: image.width(),
        height: image.height(),
        source_color,
        orientation_applied: orientation,
    };
    Ok((image, info))
}
fn orient_image(image: Image, orientation: u8) -> Result<Image, Issue> {
    if orientation == 1 {
        return Ok(image);
    }
    let (w, h) = (image.width() as usize, image.height() as usize);
    let (dw, dh) = if orientation >= 5 { (h, w) } else { (w, h) };
    let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Premul, ColorSpace::new_srgb());
    let mut src = vec![0u8; w * h * 4];
    if !image.read_pixels(&info, &mut src, w * 4, (0, 0), skia_safe::image::CachingHint::Disallow) {
        return Err(decode_error("EXIF pixel readback failed"));
    }
    drop(image);
    let mut dst = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                8 => (y, w - 1 - x),
                _ => return Err(decode_error("EXIF orientation must be 1..=8")),
            };
            dst[(dy * dw + dx) * 4..(dy * dw + dx) * 4 + 4].copy_from_slice(&src[(y * w + x) * 4..(y * w + x) * 4 + 4]);
        }
    }
    let info = ImageInfo::new(
        (dw as i32, dh as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        ColorSpace::new_srgb(),
    );
    images::raster_from_data(&info, Data::new_copy(&dst), dw * 4).ok_or_else(|| decode_error("EXIF normalization allocation failed"))
}
fn exif_orientation(bytes: &[u8]) -> Result<u8, Issue> {
    let bytes = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
    let little = match bytes.get(0..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Err(decode_error("invalid EXIF TIFF byte order")),
    };
    let u16_at = |i: usize| -> Option<u16> {
        let a: [u8; 2] = bytes.get(i..i.checked_add(2)?)?.try_into().ok()?;
        Some(if little { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) })
    };
    let u32_at = |i: usize| -> Option<u32> {
        let a: [u8; 4] = bytes.get(i..i.checked_add(4)?)?.try_into().ok()?;
        Some(if little { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    };
    if u16_at(2) != Some(42) {
        return Err(decode_error("invalid EXIF TIFF header"));
    }
    let ifd = u32_at(4).ok_or_else(|| decode_error("truncated EXIF header"))? as usize;
    let count = u16_at(ifd).ok_or_else(|| decode_error("truncated EXIF directory"))? as usize;
    if count > 512 {
        return Err(Issue::error(
            Code::ResourceLimit,
            "too many EXIF directory fields",
            "Use smaller valid metadata.",
        ));
    }
    for i in 0..count {
        let base = ifd.checked_add(2 + i * 12).ok_or_else(|| decode_error("EXIF offset overflow"))?;
        let tag = u16_at(base).ok_or_else(|| decode_error("truncated EXIF field"))?;
        if bytes.get(base..base + 12).is_none() {
            return Err(decode_error("truncated EXIF field"));
        }
        if tag == 0x0112 {
            if u16_at(base + 2) != Some(3) || u32_at(base + 4) != Some(1) {
                return Err(decode_error("invalid EXIF orientation type/count"));
            }
            let value = u16_at(base + 8).ok_or_else(|| decode_error("truncated EXIF orientation"))?;
            if !(1..=8).contains(&value) {
                return Err(decode_error("EXIF orientation must be 1..=8"));
            }
            return Ok(value as u8);
        }
    }
    Ok(1)
}

/// Bounded header inspection for aggregate reservation, before pixel decode.
pub fn probe_dimensions(bytes: &[u8], declared: MimeHint) -> Result<(u32, u32), Issue> {
    if bytes.len() > 100 * 1024 * 1024 {
        return Err(Issue::error(
            Code::ResourceLimit,
            "compressed image exceeds 100 MiB",
            "Use smaller input.",
        ));
    }
    let sniffed = sniff(bytes);
    let expected = match declared {
        MimeHint::Png => Sniffed::Png,
        MimeHint::Jpeg => Sniffed::Jpeg,
        MimeHint::Webp => Sniffed::Webp,
        MimeHint::Svg => Sniffed::Svg,
    };
    if sniffed != expected {
        return Err(decode_error("image type does not match its declared MIME"));
    }
    let (w, h) = match sniffed {
        Sniffed::Webp => image_webp::WebPDecoder::new(Cursor::new(bytes))
            .map_err(|e| decode_error(format!("WebP header: {e}")))?
            .dimensions(),
        Sniffed::Png | Sniffed::Jpeg => {
            let decoder = if sniffed == Sniffed::Png {
                png_decoder::decoder()
            } else {
                jpeg_decoder::decoder()
            };
            let codec = Codec::from_data_with_decoders(Data::new_copy(bytes), &[decoder])
                .ok_or_else(|| decode_error("image header cannot be decoded"))?;
            let dims = codec.dimensions();
            if dims.width <= 0 || dims.height <= 0 {
                return Err(decode_error("image dimensions are invalid"));
            }
            (dims.width as u32, dims.height as u32)
        }
        _ => return Err(decode_error("raster header is unsupported")),
    };
    if w == 0 || h == 0 || w > MAX_IMAGE_SIDE as u32 || h > MAX_IMAGE_SIDE as u32 || u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS as u64 {
        return Err(Issue::error(
            Code::ResourceLimit,
            "image dimensions exceed raster limits",
            "Use smaller images.",
        ));
    }
    Ok((w, h))
}

fn embedded_profile_present(bytes: &[u8], format: Sniffed) -> bool {
    match format {
        Sniffed::Png => {
            let mut offset = 8usize;
            while let Some(header) = bytes.get(offset..offset.saturating_add(8)) {
                let length = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
                if &header[4..8] == b"iCCP" {
                    return true;
                }
                let Some(end) = offset.checked_add(length).and_then(|n| n.checked_add(12)) else {
                    break;
                };
                if end > bytes.len() {
                    break;
                }
                offset = end;
            }
            false
        }
        Sniffed::Jpeg => {
            let mut offset = 2usize;
            while offset + 1 < bytes.len() {
                if bytes[offset] != 0xff {
                    break;
                }
                while bytes.get(offset) == Some(&0xff) {
                    offset += 1;
                }
                let Some(&marker) = bytes.get(offset) else {
                    break;
                };
                offset += 1;
                if marker == 0xda || marker == 0xd9 {
                    break;
                }
                if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
                    continue;
                }
                let Some(length) = bytes
                    .get(offset..offset + 2)
                    .map(|s| u16::from_be_bytes(s.try_into().unwrap()) as usize)
                else {
                    break;
                };
                if length < 2 || offset + length > bytes.len() {
                    break;
                }
                if marker == 0xe2 && bytes[offset + 2..offset + length].starts_with(b"ICC_PROFILE\0") {
                    return true;
                }
                offset += length;
            }
            false
        }
        _ => false,
    }
}
