//! Raster asset decoding (AST-001/003). The format is identified from the
//! bytes, must match the declared mimeHint, and only the matching Skia
//! decoder is offered. Dimensions are checked before pixels are decoded.

use design_model::{Code, Issue, MimeHint};
use serde::Serialize;
use skia_safe::codec::{EncodedOrigin, jpeg_decoder, png_decoder};
use skia_safe::{AlphaType, Codec, ColorSpace, ColorType, Data, Image, ImageInfo};

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
}

pub fn decode(bytes: &[u8], declared: MimeHint) -> Result<(Image, DecodedInfo), Issue> {
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
    let decoder = match sniffed {
        Sniffed::Png => png_decoder::decoder(),
        Sniffed::Jpeg => jpeg_decoder::decoder(),
        Sniffed::Webp => {
            return Err(Issue::error(
                Code::AssetType,
                "WebP decoding is not available in this Phase 0 build",
                "Use PNG or JPEG.",
            ));
        }
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
    if codec.origin() != EncodedOrigin::TopLeft {
        return Err(Issue::error(
            Code::AssetType,
            format!("EXIF orientation {:?} is not applied in Phase 0", codec.origin()),
            "Rotate the pixels and remove the orientation tag before importing.",
        ));
    }

    let source_color = match codec.info().color_space() {
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
    Ok((
        image,
        DecodedInfo {
            width: dims.width,
            height: dims.height,
            source_color,
        },
    ))
}
