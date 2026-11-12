use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use clap::ValueEnum;
use image::DynamicImage;
use ort::environment::Environment;
use ort::ep;
use ort::session::Session;
use ort::session::builder::AutoDevicePolicy;
use ort::value::Tensor;

use crate::error::{ParityError, Result};
use crate::preprocess::{
    EmbeddingBudget, PATCH_FLOATS, ResizerBackend, WrongPreprocessMode, preprocess_dynamic_image,
};
use crate::tokenizer::{TextTokenizer, pad_image_prompt_batch};

pub const NATIVE_EMBEDDING_DIM: usize = 768;
pub const DEFAULT_STORED_DIM: usize = 256;
pub const MODEL_ID: &str = "onnx-community/embeddinggemma-2-ONNX";
pub const MODEL_WEIGHTS_SHORT_SHA: &str = "daa72c5";
pub const PREPROCESS_VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExecutionProviderKind {
    #[value(name = "cpu")]
    Cpu,
    #[value(name = "coreml")]
    CoreMl,
    #[value(name = "directml")]
    DirectMl,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub model_dir: PathBuf,
    pub ort_lib_path: PathBuf,
    pub intra_threads: usize,
    pub inter_threads: usize,
    pub budget: EmbeddingBudget,
    pub resizer: ResizerBackend,
    pub backbone_ep: ExecutionProviderKind,
    pub vision_ep: ExecutionProviderKind,
}

#[derive(Debug, Clone)]
pub struct EmbeddingModelInfo {
    pub model_id: String,
    pub model_revision: String,
    pub native_dim: usize,
    pub stored_dim: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ImageTiming {
    pub preprocess_ms: f64,
    pub vision_ms: f64,
    pub backbone_ms: f64,
    pub total_ms: f64,
}

/// Future §9.1 product boundary shape for embedding inference.
pub trait EmbeddingProvider {
    fn model_info(&self, stored_dim: usize) -> EmbeddingModelInfo;
    fn embed_queries(&mut self, queries: &[&str], stored_dim: usize) -> Result<Vec<Vec<f32>>>;
    fn embed_documents(
        &mut self,
        docs: &[(Option<&str>, &str)],
        stored_dim: usize,
    ) -> Result<Vec<Vec<f32>>>;
    fn embed_images(&mut self, images: &[DynamicImage], stored_dim: usize)
    -> Result<Vec<Vec<f32>>>;
}

pub struct EmbeddingGemma2Provider {
    config: ProviderConfig,
    env: Environment,
    tokenizer: TextTokenizer,
    backbone: Session,
    vision: Option<Session>,
    pub backbone_load_ms: f64,
    pub vision_load_ms: Option<f64>,
}

/// Default pinned ONNX Runtime dynamic library path inside `spikes/embeddings/models/ort-runtime/`.
pub fn default_ort_lib_path(repo_root: &Path) -> PathBuf {
    let base = repo_root.join("spikes/embeddings/models/ort-runtime");
    #[cfg(target_os = "macos")]
    {
        base.join("onnxruntime-osx-arm64-1.31.1/lib/libonnxruntime.1.31.1.dylib")
    }
    #[cfg(target_os = "windows")]
    {
        let lib_candidate = base.join("onnxruntime-win-x64-1.31.1/lib/onnxruntime.dll");
        if lib_candidate.exists() {
            lib_candidate
        } else {
            base.join("onnxruntime-win-x64-1.31.1/bin/onnxruntime.dll")
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        base.join("libonnxruntime.so")
    }
}

/// Process-global ORT `Environment` cache: ONNX Runtime allows only one `OrtEnv` per process,
/// so benchmark sweeps and sequential `EmbeddingGemma2Provider::new` calls clone the shared handle.
static PROCESS_ORT_ENV: Mutex<Option<Environment>> = Mutex::new(None);

fn shared_ort_environment(ort_lib_path: &Path) -> Result<Environment> {
    let mut guard = PROCESS_ORT_ENV
        .lock()
        .map_err(|_| ParityError::InvalidInput("ORT environment mutex poisoned".into()))?;
    if let Some(env) = guard.as_ref() {
        return Ok(env.clone());
    }
    let env = ort::init_from(ort_lib_path)?
        .with_name("embedding_parity")
        .build()?;
    *guard = Some(env.clone());
    Ok(env)
}

impl EmbeddingGemma2Provider {
    /// Initializes ONNX Runtime from the pinned dynamic library path and loads only the text
    /// tokenizer + text/multimodal backbone. The vision encoder loads lazily on first image call.
    pub fn new(config: ProviderConfig) -> Result<Self> {
        if !config.ort_lib_path.exists() {
            return Err(ParityError::InvalidInput(format!(
                "Pinned ONNX Runtime library not found at {:?}. Run `python spikes/embeddings/fetch_models.py` first.",
                config.ort_lib_path
            )));
        }

        let env = shared_ort_environment(&config.ort_lib_path)?;

        let tok_path = config.model_dir.join("tokenizer.json");
        let tokenizer = TextTokenizer::from_file(&tok_path)?;

        let backbone_path = config.model_dir.join("onnx/model_quantized.onnx");
        let t0 = Instant::now();
        let backbone = build_session(
            &env,
            &backbone_path,
            config.intra_threads,
            config.inter_threads,
            config.backbone_ep,
        )?;
        let backbone_load_ms = t0.elapsed().as_secs_f64() * 1000.0;

        Ok(Self {
            config,
            env,
            tokenizer,
            backbone,
            vision: None,
            backbone_load_ms,
            vision_load_ms: None,
        })
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub fn tokenizer(&self) -> &TextTokenizer {
        &self.tokenizer
    }

    pub fn has_vision_encoder(&self) -> bool {
        self.vision.is_some()
    }

    /// Loads `vision_encoder_quantized.onnx` if it is not yet loaded in memory.
    pub fn ensure_vision_loaded(&mut self) -> Result<()> {
        if self.vision.is_some() {
            return Ok(());
        }
        let vision_path = self
            .config
            .model_dir
            .join("onnx/vision_encoder_quantized.onnx");
        let t0 = Instant::now();
        let session = build_session(
            &self.env,
            &vision_path,
            self.config.intra_threads,
            self.config.inter_threads,
            self.config.vision_ep,
        )?;
        let load_ms = t0.elapsed().as_secs_f64() * 1000.0;
        self.vision_load_ms = Some(load_ms);
        self.vision = Some(session);
        Ok(())
    }

    /// Drops the vision encoder session to free its resident weights memory (~115+ MB RSS).
    pub fn drop_vision_encoder(&mut self) {
        self.vision = None;
    }

    /// Runs backbone inference on pre-tokenized `input_ids` / `attention_mask` (single batch row).
    pub fn embed_pretokenized(
        &mut self,
        input_ids: &[i64],
        attention_mask: &[i64],
        batch_size: usize,
        seq_len: usize,
        stored_dim: usize,
    ) -> Result<(Vec<Vec<f32>>, Duration)> {
        let t0 = Instant::now();
        let ids_t = Tensor::from_array((vec![batch_size, seq_len], input_ids.to_vec()))?;
        let mask_t = Tensor::from_array((vec![batch_size, seq_len], attention_mask.to_vec()))?;
        let empty_img = Tensor::from_array((vec![0usize, 512], Vec::<f32>::new()))?;
        let empty_vid = Tensor::from_array((vec![0usize, 512], Vec::<f32>::new()))?;
        let empty_aud = Tensor::from_array((vec![0usize, 512], Vec::<f32>::new()))?;

        let outputs = self.backbone.run(ort::inputs![
            "input_ids" => ids_t,
            "attention_mask" => mask_t,
            "image_features" => empty_img,
            "video_features" => empty_vid,
            "audio_features" => empty_aud,
        ])?;

        let out_val = outputs.get("sentence_embedding").ok_or_else(|| {
            ParityError::InvalidInput("Missing `sentence_embedding` output from backbone".into())
        })?;
        let (shape, data) = out_val.try_extract_tensor::<f32>()?;
        if shape.len() != 2
            || shape[0] as usize != batch_size
            || shape[1] as usize != NATIVE_EMBEDDING_DIM
        {
            return Err(ParityError::InvalidInput(format!(
                "Unexpected `sentence_embedding` shape {:?}; expected [{batch_size}, {NATIVE_EMBEDDING_DIM}]",
                shape
            )));
        }

        let mut vecs = Vec::with_capacity(batch_size);
        for b in 0..batch_size {
            let start = b * NATIVE_EMBEDDING_DIM;
            let raw_row = &data[start..start + NATIVE_EMBEDDING_DIM];
            vecs.push(truncate_and_l2_normalize(raw_row, stored_dim)?);
        }
        Ok((vecs, t0.elapsed()))
    }

    pub fn embed_formatted_texts(
        &mut self,
        formatted_texts: &[&str],
        stored_dim: usize,
    ) -> Result<(Vec<Vec<f32>>, Duration)> {
        let encoded = self.tokenizer.encode_batch_formatted(formatted_texts)?;
        self.embed_pretokenized(
            &encoded.input_ids,
            &encoded.attention_mask,
            encoded.batch_size,
            encoded.seq_len,
            stored_dim,
        )
    }

    pub fn embed_images_with_timing(
        &mut self,
        images: &[DynamicImage],
        budget: EmbeddingBudget,
        stored_dim: usize,
        wrong_mode: Option<WrongPreprocessMode>,
    ) -> Result<(Vec<Vec<f32>>, ImageTiming)> {
        if images.is_empty() {
            return Err(ParityError::InvalidInput(
                "Image batch cannot be empty".into(),
            ));
        }
        self.ensure_vision_loaded()?;

        let t_total = Instant::now();
        let t_pre = Instant::now();
        let batch_size = images.len();
        let max_patches = budget.max_patches();
        let mut flat_pv = Vec::with_capacity(batch_size * max_patches * PATCH_FLOATS);
        let mut flat_pid = Vec::with_capacity(batch_size * max_patches * 2);
        let mut prompt_seqs = Vec::with_capacity(batch_size);

        for img in images {
            let prep = preprocess_dynamic_image(img, budget, self.config.resizer, wrong_mode)?;
            flat_pv.extend_from_slice(&prep.pixel_values);
            flat_pid.extend_from_slice(&prep.pixel_position_ids);
            prompt_seqs.push(prep.prompt_ids);
        }
        let preprocess_ms = t_pre.elapsed().as_secs_f64() * 1000.0;

        let batched_prompts = pad_image_prompt_batch(&prompt_seqs)?;

        let t_vis = Instant::now();
        let vision_session = self.vision.as_mut().ok_or_else(|| {
            ParityError::InvalidInput("Vision encoder session unexpectedly missing".into())
        })?;
        let pv_tensor = Tensor::from_array((vec![batch_size, max_patches, PATCH_FLOATS], flat_pv))?;
        let pid_tensor = Tensor::from_array((vec![batch_size, max_patches, 2], flat_pid))?;
        let vis_outputs = vision_session.run(ort::inputs![
            "pixel_values" => pv_tensor,
            "pixel_position_ids" => pid_tensor,
        ])?;
        let feat_val = vis_outputs.get("image_features").ok_or_else(|| {
            ParityError::InvalidInput("Missing `image_features` output from vision encoder".into())
        })?;
        let (feat_shape, feat_data) = feat_val.try_extract_tensor::<f32>()?;
        if feat_shape.len() != 2 || feat_shape[1] != 512 {
            return Err(ParityError::InvalidInput(format!(
                "Unexpected `image_features` shape {:?}; expected [sum_soft_tokens, 512]",
                feat_shape
            )));
        }
        let img_feat_tensor = Tensor::from_array((
            vec![feat_shape[0] as usize, feat_shape[1] as usize],
            feat_data.to_vec(),
        ))?;
        drop(vis_outputs);
        let vision_ms = t_vis.elapsed().as_secs_f64() * 1000.0;

        let t_back = Instant::now();
        let ids_t = Tensor::from_array((
            vec![batched_prompts.batch_size, batched_prompts.seq_len],
            batched_prompts.input_ids,
        ))?;
        let mask_t = Tensor::from_array((
            vec![batched_prompts.batch_size, batched_prompts.seq_len],
            batched_prompts.attention_mask,
        ))?;
        let empty_vid = Tensor::from_array((vec![0usize, 512], Vec::<f32>::new()))?;
        let empty_aud = Tensor::from_array((vec![0usize, 512], Vec::<f32>::new()))?;

        let back_outputs = self.backbone.run(ort::inputs![
            "input_ids" => ids_t,
            "attention_mask" => mask_t,
            "image_features" => img_feat_tensor,
            "video_features" => empty_vid,
            "audio_features" => empty_aud,
        ])?;
        let emb_val = back_outputs.get("sentence_embedding").ok_or_else(|| {
            ParityError::InvalidInput("Missing `sentence_embedding` output from backbone".into())
        })?;
        let (emb_shape, emb_data) = emb_val.try_extract_tensor::<f32>()?;
        if emb_shape.len() != 2
            || emb_shape[0] as usize != batch_size
            || emb_shape[1] as usize != NATIVE_EMBEDDING_DIM
        {
            return Err(ParityError::InvalidInput(format!(
                "Unexpected `sentence_embedding` shape {:?}; expected [{batch_size}, {NATIVE_EMBEDDING_DIM}]",
                emb_shape
            )));
        }
        let mut vecs = Vec::with_capacity(batch_size);
        for b in 0..batch_size {
            let start = b * NATIVE_EMBEDDING_DIM;
            let raw_row = &emb_data[start..start + NATIVE_EMBEDDING_DIM];
            vecs.push(truncate_and_l2_normalize(raw_row, stored_dim)?);
        }
        let backbone_ms = t_back.elapsed().as_secs_f64() * 1000.0;
        let total_ms = t_total.elapsed().as_secs_f64() * 1000.0;

        Ok((
            vecs,
            ImageTiming {
                preprocess_ms,
                vision_ms,
                backbone_ms,
                total_ms,
            },
        ))
    }
}

impl EmbeddingProvider for EmbeddingGemma2Provider {
    fn model_info(&self, stored_dim: usize) -> EmbeddingModelInfo {
        EmbeddingModelInfo {
            model_id: MODEL_ID.to_string(),
            model_revision: format!(
                "onnx-q8-{MODEL_WEIGHTS_SHORT_SHA}-b{}-{PREPROCESS_VERSION}",
                self.config.budget.soft_tokens()
            ),
            native_dim: NATIVE_EMBEDDING_DIM,
            stored_dim,
        }
    }

    fn embed_queries(&mut self, queries: &[&str], stored_dim: usize) -> Result<Vec<Vec<f32>>> {
        let formatted: Vec<String> = queries
            .iter()
            .map(|q| TextTokenizer::format_query(q))
            .collect();
        let refs: Vec<&str> = formatted.iter().map(String::as_str).collect();
        let (vecs, _) = self.embed_formatted_texts(&refs, stored_dim)?;
        Ok(vecs)
    }

    fn embed_documents(
        &mut self,
        docs: &[(Option<&str>, &str)],
        stored_dim: usize,
    ) -> Result<Vec<Vec<f32>>> {
        let formatted: Vec<String> = docs
            .iter()
            .map(|(title, body)| TextTokenizer::format_document(*title, body))
            .collect();
        let refs: Vec<&str> = formatted.iter().map(String::as_str).collect();
        let (vecs, _) = self.embed_formatted_texts(&refs, stored_dim)?;
        Ok(vecs)
    }

    fn embed_images(
        &mut self,
        images: &[DynamicImage],
        stored_dim: usize,
    ) -> Result<Vec<Vec<f32>>> {
        let (vecs, _) =
            self.embed_images_with_timing(images, self.config.budget, stored_dim, None)?;
        Ok(vecs)
    }
}

fn build_session(
    env: &Environment,
    model_path: &Path,
    intra_threads: usize,
    inter_threads: usize,
    ep_kind: ExecutionProviderKind,
) -> Result<Session> {
    if !model_path.exists() {
        return Err(ParityError::InvalidInput(format!(
            "ONNX model file not found: {:?}",
            model_path
        )));
    }
    let map_builder_err = |e: ort::Error<ort::session::builder::SessionBuilder>| {
        ParityError::InvalidInput(format!("ONNX Runtime session config error: {e}"))
    };
    let mut builder = Session::builder(env)?
        .with_intra_threads(intra_threads)
        .map_err(map_builder_err)?
        .with_inter_threads(inter_threads)
        .map_err(map_builder_err)?;

    builder = match ep_kind {
        ExecutionProviderKind::Cpu => builder
            .with_auto_device(AutoDevicePolicy::PreferCPU)
            .map_err(map_builder_err)?,
        ExecutionProviderKind::CoreMl => builder
            .with_execution_providers([
                ep::CoreML::default().build().error_on_failure(),
                ep::CPU::default().build(),
            ])
            .map_err(map_builder_err)?,
        ExecutionProviderKind::DirectMl => builder
            .with_execution_providers([
                ep::DirectML::default().build().error_on_failure(),
                ep::CPU::default().build(),
            ])
            .map_err(map_builder_err)?,
    };

    Ok(builder.commit_from_file(model_path)?)
}

/// Truncates a full-precision (768-d) vector to `target_dim` and L2-normalizes the slice.
/// Order invariant (AGENTS.md): always truncate BEFORE L2 normalization.
pub fn truncate_and_l2_normalize(vector: &[f32], target_dim: usize) -> Result<Vec<f32>> {
    if target_dim == 0 || target_dim > vector.len() {
        return Err(ParityError::InvalidInput(format!(
            "Invalid target_dim {target_dim} for vector of length {}",
            vector.len()
        )));
    }
    let sliced = &vector[..target_dim];
    let norm_sq: f64 = sliced.iter().map(|&v| (v as f64) * (v as f64)).sum();
    let norm = norm_sq.sqrt();
    if norm == 0.0 || !norm.is_finite() {
        return Err(ParityError::InvalidInput(
            "Cannot L2-normalize zero or non-finite embedding vector".into(),
        ));
    }
    Ok(sliced.iter().map(|&v| ((v as f64) / norm) as f32).collect())
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Result<f32> {
    if a.len() != b.len() || a.is_empty() {
        return Err(ParityError::InvalidInput(format!(
            "Cosine dimension mismatch or empty: {} vs {}",
            a.len(),
            b.len()
        )));
    }
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let fx = x as f64;
        let fy = y as f64;
        dot += fx * fy;
        na += fx * fx;
        nb += fy * fy;
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 {
        return Err(ParityError::InvalidInput(
            "Zero-norm vector in cosine computation".into(),
        ));
    }
    Ok((dot / denom) as f32)
}
