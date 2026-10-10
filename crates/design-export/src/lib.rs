//! Explicit derivative exports of the canonical CPU raster. File access belongs
//! to the desktop grant broker; this crate never reads or writes a path.
use design_model::{Code, Issue};
use design_render::{RenderOutput, resources::sha256_hex};
use serde::{Deserialize, Serialize};
use skia_safe::{AlphaType, Color, ColorSpace, ColorType, Data, Image, ImageInfo, Paint, Rect, jpeg_encoder, pdf, surfaces};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Png,
    Jpeg,
    Webp,
    Svg,
    Pdf,
}
impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportRecipe {
    pub format: ExportFormat,
    pub quality: Option<u8>,
    pub jpeg_background: Option<String>,
    pub webp_lossless: bool,
    pub allow_rasterization: bool,
    pub pdf_dpi: u16,
}
impl Default for ExportRecipe {
    fn default() -> Self {
        Self {
            format: ExportFormat::Png,
            quality: None,
            jpeg_background: None,
            webp_lossless: true,
            allow_rasterization: false,
            pdf_dpi: 144,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub format: ExportFormat,
    pub width: u32,
    pub height: u32,
    pub color_space: &'static str,
    pub alpha: &'static str,
    pub quality: Option<u8>,
    pub jpeg_background: Option<String>,
    pub rasterized: bool,
    pub rasterization_dpi: Option<u16>,
    pub text_editable: bool,
    pub font_embedding: &'static str,
    pub conversions: Vec<String>,
    pub output_sha256: String,
}
pub struct ExportOutput {
    pub bytes: Vec<u8>,
    pub report: ExportReport,
}
fn error(message: impl Into<String>) -> Issue {
    Issue::error(
        Code::ExportUnsupported,
        message,
        "Choose supported settings; SVG/PDF require explicit whole-artboard rasterization approval.",
    )
}

pub fn export(rendered: &RenderOutput, recipe: &ExportRecipe) -> Result<ExportOutput, Issue> {
    let (w, h) = (rendered.report.width, rendered.report.height);
    if w == 0
        || h == 0
        || w > 16384
        || h > 16384
        || u64::from(w) * u64::from(h) > 64 * 1024 * 1024
        || rendered.rgba.len() != w as usize * h as usize * 4
    {
        return Err(error("invalid raster dimensions or RGBA payload"));
    }
    if recipe.quality.is_some_and(|q| q == 0 || q > 100) {
        return Err(error("JPEG quality must be 1..=100"));
    }
    if recipe.format != ExportFormat::Jpeg && (recipe.quality.is_some() || recipe.jpeg_background.is_some()) {
        return Err(error("quality and jpegBackground apply only to JPEG"));
    }
    if recipe.format == ExportFormat::Webp && !recipe.webp_lossless {
        return Err(error("this codec supports lossless WebP only"));
    }
    let rasterized = matches!(recipe.format, ExportFormat::Svg | ExportFormat::Pdf);
    if rasterized && !recipe.allow_rasterization {
        return Err(error(
            "SVG/PDF would rasterize the complete artboard and remove text/vector editing; explicit approval is required",
        ));
    }
    let rgba = rendered.rgba.len() as u64;
    let png = rendered.png.len() as u64;
    let estimated = match recipe.format {
        ExportFormat::Png => rgba + 2 * png,
        ExportFormat::Webp => 4 * rgba + png,
        ExportFormat::Jpeg => 4 * rgba + 2 * png,
        ExportFormat::Svg => rgba + 4 * png,
        ExportFormat::Pdf => 3 * rgba + 3 * png,
    } + 64 * 1024 * 1024;
    if estimated > 1024 * 1024 * 1024 {
        return Err(Issue::error(
            Code::ResourceLimit,
            "export codec reservation exceeds 1 GiB",
            "Reduce output dimensions before encoding this format.",
        ));
    }
    let mut conversions = Vec::new();
    let mut alpha = "preserved";
    let bytes = match recipe.format {
        ExportFormat::Png => rendered.png.clone(),
        ExportFormat::Webp => {
            let mut bytes = Vec::new();
            let mut encoder = image_webp::WebPEncoder::new(&mut bytes);
            encoder.set_icc_profile(design_render::resources::SRGB_ICC.to_vec());
            encoder
                .encode(&rendered.rgba, w, h, image_webp::ColorType::Rgba8)
                .map_err(|e| error(format!("WebP encode failed: {e}")))?;
            conversions.push("Lossless VP8L; RGB samples use sRGB.".into());
            bytes
        }
        ExportFormat::Jpeg => {
            let bg = recipe
                .jpeg_background
                .as_deref()
                .ok_or_else(|| error("JPEG requires an explicit opaque background color"))?;
            if !design_model::validate::is_hex_color(bg) {
                return Err(error("JPEG background must be #RRGGBB"));
            }
            let rgb = |i| u8::from_str_radix(&bg[i..i + 2], 16).expect("validated color");
            let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Opaque, ColorSpace::new_srgb());
            let mut surface = surfaces::raster(&info, None, None).ok_or_else(|| error("JPEG surface allocation failed"))?;
            surface.canvas().clear(Color::from_rgb(rgb(1), rgb(3), rgb(5)));
            let image = raster_image(rendered)?;
            surface.canvas().draw_image(&image, (0, 0), None);
            let image = surface.image_snapshot();
            let pixels = image.peek_pixels().ok_or_else(|| error("JPEG pixel access failed"))?;
            let mut bytes = Vec::new();
            let options = jpeg_encoder::Options {
                quality: u32::from(recipe.quality.unwrap_or(90)),
                downsample: jpeg_encoder::Downsample::No,
                ..Default::default()
            };
            if !jpeg_encoder::encode(&pixels, &mut bytes, &options) {
                return Err(error("JPEG encode failed"));
            }
            alpha = "flattened";
            conversions.push(format!("Transparency composited over {bg}; JPEG 4:4:4, sRGB."));
            bytes
        }
        ExportFormat::Svg => {
            conversions.push("Complete artboard embedded as an sRGB PNG; no editable text or vector geometry.".into());
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><metadata>SILIQ: approved whole-artboard rasterization; text and vectors are not editable; sRGB.</metadata><image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{}\"/></svg>\n",base64(&rendered.png)).into_bytes()
        }
        ExportFormat::Pdf => {
            if !(72..=1200).contains(&recipe.pdf_dpi) {
                return Err(error("RGB proof PDF DPI must be 72..=1200"));
            }
            let image = raster_image(rendered)?;
            let mut bytes = Vec::new();
            let metadata = pdf::Metadata {
                title: "SILIQ RGB proof — rasterized artboard".into(),
                producer: "SILIQ Creative Generator".into(),
                ..Default::default()
            };
            let document = pdf::new_document(&mut bytes, Some(&metadata));
            let pw = w as f32 * 72.0 / f32::from(recipe.pdf_dpi);
            let ph = h as f32 * 72.0 / f32::from(recipe.pdf_dpi);
            let mut page = document.begin_page((pw, ph), None);
            page.canvas()
                .draw_image_rect(&image, None, Rect::from_wh(pw, ph), &Paint::default());
            page.end_page().close();
            conversions.push(format!(
                "Complete artboard rasterized at {} DPI; RGB proof, no text editing, no font embedding, no PDF/X or CMYK claim.",
                recipe.pdf_dpi
            ));
            bytes
        }
    };
    let report = ExportReport {
        format: recipe.format,
        width: w,
        height: h,
        color_space: "sRGB",
        alpha,
        quality: if recipe.format == ExportFormat::Jpeg {
            Some(recipe.quality.unwrap_or(90))
        } else {
            None
        },
        jpeg_background: recipe.jpeg_background.clone(),
        rasterized,
        rasterization_dpi: if recipe.format == ExportFormat::Pdf {
            Some(recipe.pdf_dpi)
        } else {
            None
        },
        text_editable: false,
        font_embedding: "none",
        conversions,
        output_sha256: sha256_hex(&bytes),
    };
    Ok(ExportOutput { bytes, report })
}
fn raster_image(rendered: &RenderOutput) -> Result<Image, Issue> {
    Image::from_encoded(Data::new_copy(&rendered.png)).ok_or_else(|| error("canonical PNG decode failed"))
}
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(TABLE[(a >> 2) as usize] as char);
        out.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 { TABLE[(c & 63) as usize] as char } else { '=' });
    }
    out
}
