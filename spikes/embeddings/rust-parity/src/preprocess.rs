use std::path::Path;

use clap::ValueEnum;
use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{
    FilterType as FirFilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
};
use image::imageops::FilterType as ImageFilterType;
use image::{DynamicImage, GenericImageView, RgbImage};

use crate::error::{ParityError, Result};
use crate::tokenizer::build_image_prompt_ids;

pub const PATCH_SIZE: usize = 16;
pub const POOLING_KERNEL_SIZE: usize = 3;
pub const SIDE_MULT: usize = PATCH_SIZE * POOLING_KERNEL_SIZE; // 48
pub const PATCH_FLOATS: usize = PATCH_SIZE * PATCH_SIZE * 3; // 768

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EmbeddingBudget {
    #[value(name = "70")]
    B70,
    #[value(name = "140")]
    B140,
    #[value(name = "280")]
    B280,
}

impl EmbeddingBudget {
    pub fn soft_tokens(self) -> usize {
        match self {
            Self::B70 => 70,
            Self::B140 => 140,
            Self::B280 => 280,
        }
    }

    /// Nine 16x16 patches (3x3 spatial pooling) collapse into each soft vision token.
    pub fn max_patches(self) -> usize {
        self.soft_tokens() * POOLING_KERNEL_SIZE * POOLING_KERNEL_SIZE
    }

    pub fn from_u32(val: u32) -> Result<Self> {
        match val {
            70 => Ok(Self::B70),
            140 => Ok(Self::B140),
            280 => Ok(Self::B280),
            other => Err(ParityError::InvalidInput(format!(
                "Unsupported vision budget {other}; expected 70, 140, or 280"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ResizerBackend {
    /// `fast_image_resize` with `Convolution(CatmullRom)` (adaptive antialiased bicubic).
    #[value(name = "fast-catmull-rom")]
    FastCatmullRom,
    /// `fast_image_resize` with `Convolution(Mitchell)`.
    #[value(name = "fast-mitchell")]
    FastMitchell,
    /// `image` crate `imageops::resize` with `FilterType::CatmullRom`.
    #[value(name = "image-catmull-rom")]
    ImageCatmullRom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WrongPreprocessMode {
    /// Applies standard ImageNet mean/std normalization instead of Gemma4's [0, 1] scale.
    #[value(name = "imagenet-norm")]
    ImageNetNorm,
    /// Uses nearest-neighbor resizing instead of antialiased bicubic.
    #[value(name = "nearest-filter")]
    NearestFilter,
    /// Emits CHW inner patch order + swapped [y, x] position ids instead of HWC + [x, y].
    #[value(name = "patch-order")]
    PatchOrder,
}

#[derive(Debug, Clone)]
pub struct PreprocessedImage {
    /// Shape: `[max_patches, 768]` stored row-major.
    pub pixel_values: Vec<f32>,
    /// Shape: `[max_patches, 2]` stored row-major `[x, y]` (or `[-1, -1]` for padding).
    pub pixel_position_ids: Vec<i64>,
    pub max_patches: usize,
    pub valid_patches: usize,
    pub num_soft_tokens: usize,
    pub target_width: usize,
    pub target_height: usize,
    pub prompt_ids: Vec<i64>,
}

/// Exact Rust translation of `Gemma4ImageProcessor.get_aspect_ratio_preserving_size`.
pub fn aspect_ratio_preserving_size(
    height: u32,
    width: u32,
    budget: EmbeddingBudget,
) -> Result<(u32, u32)> {
    if height == 0 || width == 0 {
        return Err(ParityError::InvalidInput(format!(
            "Image dimensions must be non-zero, got {width}x{height}"
        )));
    }

    let max_patches = budget.max_patches() as f64;
    let total_px = (height as f64) * (width as f64);
    let target_px = max_patches * ((PATCH_SIZE * PATCH_SIZE) as f64);
    let factor = (target_px / total_px).sqrt();
    let ideal_height = factor * (height as f64);
    let ideal_width = factor * (width as f64);
    let side_mult = SIDE_MULT as u32;

    let mut target_height = ((ideal_height / (SIDE_MULT as f64)).floor() as u32) * side_mult;
    let mut target_width = ((ideal_width / (SIDE_MULT as f64)).floor() as u32) * side_mult;

    if target_height == 0 && target_width == 0 {
        return Err(ParityError::Image(format!(
            "Target size rounded to 0x0 for input {width}x{height} at budget {}",
            budget.soft_tokens()
        )));
    }

    let max_side_length =
        ((budget.max_patches() / (POOLING_KERNEL_SIZE * POOLING_KERNEL_SIZE)) as u32) * side_mult;

    if target_height == 0 {
        target_height = side_mult;
        let scaled = (((width as f64) / (height as f64)).floor() as u32) * side_mult;
        target_width = scaled.min(max_side_length);
    } else if target_width == 0 {
        target_width = side_mult;
        let scaled = (((height as f64) / (width as f64)).floor() as u32) * side_mult;
        target_height = scaled.min(max_side_length);
    }

    if (target_height as usize) * (target_width as usize) > (target_px as usize) {
        return Err(ParityError::Image(format!(
            "Target {target_width}x{target_height} exceeds patch budget {max_patches}"
        )));
    }

    Ok((target_height, target_width))
}

/// Preprocess an image file from disk. EXIF orientation is ignored here only for PNGs;
/// for JPEGs with EXIF orientation tags, previews produced by libvips are already upright.
pub fn preprocess_image_path(
    path: &Path,
    budget: EmbeddingBudget,
    resizer: ResizerBackend,
    wrong_mode: Option<WrongPreprocessMode>,
) -> Result<PreprocessedImage> {
    let dyn_img = image::open(path)
        .map_err(|e| ParityError::Image(format!("Failed to open {:?}: {e}", path)))?;
    preprocess_dynamic_image(&dyn_img, budget, resizer, wrong_mode)
}

pub fn preprocess_dynamic_image(
    img: &DynamicImage,
    budget: EmbeddingBudget,
    resizer_backend: ResizerBackend,
    wrong_mode: Option<WrongPreprocessMode>,
) -> Result<PreprocessedImage> {
    let (orig_w, orig_h) = img.dimensions();
    let (target_h, target_w) = aspect_ratio_preserving_size(orig_h, orig_w, budget)?;

    // Ensure dense RGB8 before resizing so bicubic weights operate strictly on 3 uint8 channels.
    let rgb = img.to_rgb8();
    let resized = if target_w == orig_w && target_h == orig_h {
        rgb
    } else if wrong_mode == Some(WrongPreprocessMode::NearestFilter) {
        image::imageops::resize(&rgb, target_w, target_h, ImageFilterType::Nearest)
    } else {
        resize_rgb8(&rgb, target_w, target_h, resizer_backend)?
    };

    patchify_and_pad(
        &resized,
        target_h as usize,
        target_w as usize,
        budget,
        wrong_mode,
    )
}

fn resize_rgb8(
    rgb: &RgbImage,
    target_w: u32,
    target_h: u32,
    backend: ResizerBackend,
) -> Result<RgbImage> {
    match backend {
        ResizerBackend::ImageCatmullRom => Ok(image::imageops::resize(
            rgb,
            target_w,
            target_h,
            ImageFilterType::CatmullRom,
        )),
        ResizerBackend::FastCatmullRom | ResizerBackend::FastMitchell => {
            let filter = match backend {
                ResizerBackend::FastCatmullRom => FirFilterType::CatmullRom,
                ResizerBackend::FastMitchell => FirFilterType::Mitchell,
                ResizerBackend::ImageCatmullRom => unreachable!(),
            };
            let src = fast_image_resize::images::ImageRef::new(
                rgb.width(),
                rgb.height(),
                rgb.as_raw(),
                PixelType::U8x3,
            )
            .map_err(|e| ParityError::Image(format!("fast_image_resize src view error: {e}")))?;
            let mut dst = FirImage::new(target_w, target_h, PixelType::U8x3);
            let mut resizer = Resizer::new();
            let options = ResizeOptions::new()
                .resize_alg(ResizeAlg::Convolution(filter))
                .use_alpha(false);
            resizer
                .resize(&src, &mut dst, Some(&options))
                .map_err(|e| ParityError::Image(format!("fast_image_resize failed: {e}")))?;
            RgbImage::from_raw(target_w, target_h, dst.into_vec())
                .ok_or_else(|| ParityError::Image("Resized RGB buffer size mismatch".to_string()))
        }
    }
}

fn patchify_and_pad(
    rgb: &RgbImage,
    height: usize,
    width: usize,
    budget: EmbeddingBudget,
    wrong_mode: Option<WrongPreprocessMode>,
) -> Result<PreprocessedImage> {
    if !height.is_multiple_of(PATCH_SIZE) || !width.is_multiple_of(PATCH_SIZE) {
        return Err(ParityError::Image(format!(
            "Resized image {width}x{height} is not divisible by patch size {PATCH_SIZE}"
        )));
    }

    let nph = height / PATCH_SIZE;
    let npw = width / PATCH_SIZE;
    let valid_patches = nph * npw;
    let max_patches = budget.max_patches();
    let num_soft_tokens = valid_patches / (POOLING_KERNEL_SIZE * POOLING_KERNEL_SIZE);

    let mut pixel_values = vec![0.0f32; max_patches * PATCH_FLOATS];
    let mut pixel_position_ids = vec![-1i64; max_patches * 2];

    let raw = rgb.as_raw();
    // Matches Transformers `rescale_factor = 1/255` (float32 multiplication preserves bit-exact parity).
    let inv_255 = 1.0f32 / 255.0f32;
    let imagenet_mean = [0.485f32, 0.456f32, 0.406f32];
    let imagenet_std = [0.229f32, 0.224f32, 0.225f32];
    let wrong_norm = wrong_mode == Some(WrongPreprocessMode::ImageNetNorm);
    let wrong_order = wrong_mode == Some(WrongPreprocessMode::PatchOrder);

    for py in 0..nph {
        for px in 0..npw {
            let patch_idx = py * npw + px;
            let pos_base = patch_idx * 2;
            if wrong_order {
                // Intentionally swapped [y, x] for negative gate verification.
                pixel_position_ids[pos_base] = py as i64;
                pixel_position_ids[pos_base + 1] = px as i64;
            } else {
                // Gemma4 / Siglip2 convention: col (x) first, row (y) second.
                pixel_position_ids[pos_base] = px as i64;
                pixel_position_ids[pos_base + 1] = py as i64;
            }

            let patch_base = patch_idx * PATCH_FLOATS;
            for dy in 0..PATCH_SIZE {
                let y = py * PATCH_SIZE + dy;
                for dx in 0..PATCH_SIZE {
                    let x = px * PATCH_SIZE + dx;
                    let src_px = (y * width + x) * 3;
                    for c in 0..3 {
                        let val = (raw[src_px + c] as f32) * inv_255;
                        let out_val = if wrong_norm {
                            (val - imagenet_mean[c]) / imagenet_std[c]
                        } else {
                            val
                        };
                        let dst_offset = if wrong_order {
                            // Wrong planar CHW layout inside patch.
                            c * (PATCH_SIZE * PATCH_SIZE) + dy * PATCH_SIZE + dx
                        } else {
                            // Correct HWC layout `(dy, dx, c)` inside each 16x16x3 patch.
                            (dy * PATCH_SIZE + dx) * 3 + c
                        };
                        pixel_values[patch_base + dst_offset] = out_val;
                    }
                }
            }
        }
    }

    Ok(PreprocessedImage {
        pixel_values,
        pixel_position_ids,
        max_patches,
        valid_patches,
        num_soft_tokens,
        target_width: width,
        target_height: height,
        prompt_ids: build_image_prompt_ids(num_soft_tokens),
    })
}

/// Stand-in app preview generator: scales long edge down to `max_edge` (1024 px) and
/// re-encodes as JPEG quality 80 in memory to approximate Phase 4B preview derivatives.
pub fn encode_decode_standin_preview(
    img: &DynamicImage,
    max_edge: u32,
    jpeg_quality: u8,
) -> Result<DynamicImage> {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    let scaled = if long > max_edge {
        img.resize(max_edge, max_edge, ImageFilterType::CatmullRom)
    } else {
        img.clone()
    };
    let mut jpeg_bytes = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, jpeg_quality);
    encoder
        .encode_image(&scaled)
        .map_err(|e| ParityError::Image(format!("JPEG preview encode failed: {e}")))?;
    image::load_from_memory_with_format(&jpeg_bytes, image::ImageFormat::Jpeg)
        .map_err(|e| ParityError::Image(format!("JPEG preview decode failed: {e}")))
}
