use std::fs::File;
use std::io::Read;
use std::path::Path;

use flate2::read::ZlibDecoder;
use serde::Deserialize;

use crate::error::{ParityError, Result};
use crate::preprocess::{
    EmbeddingBudget, PATCH_FLOATS, ResizerBackend, WrongPreprocessMode,
    encode_decode_standin_preview, preprocess_image_path,
};
use crate::provider::{
    DEFAULT_STORED_DIM, EmbeddingGemma2Provider, NATIVE_EMBEDDING_DIM, cosine_similarity,
    truncate_and_l2_normalize,
};
use crate::tokenizer::TextTokenizer;

#[derive(Debug, Deserialize)]
pub struct TokensRefFile {
    pub max_length: usize,
    pub padding_side: String,
    pub cases: Vec<TokenRefCase>,
}

#[derive(Debug, Deserialize)]
pub struct TokenRefCase {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub input_ids: Vec<i64>,
    pub attention_mask: Vec<i64>,
    pub include_in_vectors: bool,
}

#[derive(Debug, Deserialize)]
pub struct TensorsIndexFile {
    pub entries: Vec<TensorRefEntry>,
}

#[derive(Debug, Deserialize)]
pub struct TensorRefEntry {
    pub key: String,
    pub image_name: String,
    pub image_rel_path: String,
    pub orig_width: u32,
    pub orig_height: u32,
    pub budget: u32,
    pub max_patches: usize,
    pub valid_patches: usize,
    pub num_soft_tokens: usize,
    pub pixel_values_shape: Vec<usize>,
    #[serde(default)]
    pub pixel_values_encoding: Option<String>,
    pub pixel_values_sha256: String,
    pub pixel_values_file: String,
    pub pixel_position_ids_shape: Vec<usize>,
    pub pixel_position_ids_sha256: String,
    pub pixel_position_ids_file: String,
    pub input_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct VectorsIndexFile {
    pub dim: usize,
    pub dtype: String,
    pub text_ids: Vec<String>,
    pub texts: Vec<String>,
    pub image_names: Vec<String>,
    pub image_rel_paths: Vec<String>,
    pub files: VectorFilesMap,
}

#[derive(Debug, Deserialize)]
pub struct VectorFilesMap {
    pub q8_text: String,
    pub q8_b140: String,
    pub q8_b280: String,
    pub fp32_text: String,
    pub fp32_b140: String,
    pub fp32_b280: String,
}

#[derive(Debug, Clone)]
pub struct StatsSummary {
    pub count: usize,
    pub min: f32,
    pub mean: f32,
    pub max: f32,
    pub worst_idx: usize,
    pub worst_label: String,
}

pub fn summarize_scores(scores: &[f32], labels: &[String]) -> Result<StatsSummary> {
    if scores.is_empty() || scores.len() != labels.len() {
        return Err(ParityError::InvalidInput(
            "Empty or mismatched score/label slices".into(),
        ));
    }
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    let mut sum = 0.0f64;
    let mut worst_idx = 0usize;
    for (idx, &v) in scores.iter().enumerate() {
        if v < min {
            min = v;
            worst_idx = idx;
        }
        if v > max {
            max = v;
        }
        sum += v as f64;
    }
    Ok(StatsSummary {
        count: scores.len(),
        min,
        mean: (sum / scores.len() as f64) as f32,
        max,
        worst_idx,
        worst_label: labels[worst_idx].clone(),
    })
}

pub fn read_f32_vectors(path: &Path, count: usize, dim: usize) -> Result<Vec<Vec<f32>>> {
    let bytes = std::fs::read(path)?;
    let expected = count * dim * 4;
    if bytes.len() != expected {
        return Err(ParityError::InvalidInput(format!(
            "Vector file {:?} has {} bytes, expected {} ({}x{} f32)",
            path,
            bytes.len(),
            expected,
            count,
            dim
        )));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let mut row = Vec::with_capacity(dim);
        let row_start = i * dim * 4;
        for d in 0..dim {
            let off = row_start + d * 4;
            let val =
                f32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            row.push(val);
        }
        out.push(row);
    }
    Ok(out)
}

fn read_zlib_bytes(path: &Path) -> Result<Vec<u8>> {
    let f = File::open(path)?;
    let mut decoder = ZlibDecoder::new(f);
    let mut raw = Vec::new();
    decoder.read_to_end(&mut raw)?;
    Ok(raw)
}

pub fn load_ref_pixel_values(parity_dir: &Path, entry: &TensorRefEntry) -> Result<Vec<f32>> {
    let raw = read_zlib_bytes(&parity_dir.join(&entry.pixel_values_file))?;
    let expected_elems = entry.max_patches * PATCH_FLOATS;
    if entry.pixel_values_encoding.as_deref() == Some("u8_times_inv255_f32_le")
        || entry.pixel_values_file.ends_with(".u8.zlib")
    {
        if raw.len() != expected_elems {
            return Err(ParityError::InvalidInput(format!(
                "Decompressed u8 tensor {:?} has {} bytes, expected {}",
                entry.pixel_values_file,
                raw.len(),
                expected_elems
            )));
        }
        let inv_255 = 1.0f32 / 255.0f32;
        Ok(raw.into_iter().map(|b| (b as f32) * inv_255).collect())
    } else {
        if raw.len() != expected_elems * 4 {
            return Err(ParityError::InvalidInput(format!(
                "Decompressed f32 tensor {:?} has {} bytes, expected {}",
                entry.pixel_values_file,
                raw.len(),
                expected_elems * 4
            )));
        }
        let mut out = Vec::with_capacity(expected_elems);
        for &chunk in raw.as_chunks::<4>().0 {
            out.push(f32::from_le_bytes(chunk));
        }
        Ok(out)
    }
}

pub fn load_ref_position_ids(parity_dir: &Path, entry: &TensorRefEntry) -> Result<Vec<i64>> {
    let raw = read_zlib_bytes(&parity_dir.join(&entry.pixel_position_ids_file))?;
    let expected_elems = entry.max_patches * 2;
    if raw.len() != expected_elems * 8 {
        return Err(ParityError::InvalidInput(format!(
            "Decompressed i64 position_ids {:?} has {} bytes, expected {}",
            entry.pixel_position_ids_file,
            raw.len(),
            expected_elems * 8
        )));
    }
    let mut out = Vec::with_capacity(expected_elems);
    for &chunk in raw.as_chunks::<8>().0 {
        out.push(i64::from_le_bytes(chunk));
    }
    Ok(out)
}

/// Verifies exact integer equality across all committed token test cases.
pub fn run_token_parity(tokenizer: &TextTokenizer, parity_dir: &Path) -> Result<(usize, usize)> {
    let raw = std::fs::read_to_string(parity_dir.join("tokens_ref.json"))?;
    let ref_data: TokensRefFile = serde_json::from_str(&raw)?;

    let mut passed = 0usize;
    let mut failed = 0usize;
    for case in &ref_data.cases {
        let enc = tokenizer.encode_formatted(&case.text)?;
        if enc.input_ids == case.input_ids && enc.attention_mask == case.attention_mask {
            passed += 1;
        } else {
            failed += 1;
            println!(
                "  [FAIL] token case {} ({}): rust len={} ref len={}",
                case.id,
                case.kind,
                enc.input_ids.len(),
                case.input_ids.len()
            );
            for (idx, (r, p)) in enc.input_ids.iter().zip(case.input_ids.iter()).enumerate() {
                if r != p {
                    println!("         first diff at index {idx}: rust={r}, ref={p}");
                    break;
                }
            }
        }
    }
    Ok((passed, failed))
}

#[derive(Debug)]
pub struct TensorParityReport {
    pub resizer: ResizerBackend,
    pub pos_ids_exact: usize,
    pub prompt_ids_exact: usize,
    pub total_entries: usize,
    pub max_abs_diff: f32,
    pub mean_abs_diff: f64,
    pub worst_key: String,
    pub worst_max_diff: f32,
    pub worst_mean_diff: f64,
}

/// Checks preprocessed `pixel_values`, `pixel_position_ids`, and image `input_ids` against Python.
pub fn run_tensor_parity(
    repo_root: &Path,
    parity_dir: &Path,
    resizer: ResizerBackend,
    wrong_mode: Option<WrongPreprocessMode>,
    verbose: bool,
) -> Result<TensorParityReport> {
    let raw = std::fs::read_to_string(parity_dir.join("tensors_index.json"))?;
    let idx: TensorsIndexFile = serde_json::from_str(&raw)?;

    let mut pos_ids_exact = 0usize;
    let mut prompt_ids_exact = 0usize;
    let mut global_max_diff = 0.0f32;
    let mut global_abs_sum = 0.0f64;
    let mut global_elems = 0usize;
    let mut worst_key = String::new();
    let mut worst_max_diff = -1.0f32;
    let mut worst_mean_diff = 0.0f64;

    for entry in &idx.entries {
        let img_path = repo_root.join(&entry.image_rel_path);
        let budget = EmbeddingBudget::from_u32(entry.budget)?;
        let rust_prep = preprocess_image_path(&img_path, budget, resizer, wrong_mode)?;
        let ref_pv = load_ref_pixel_values(parity_dir, entry)?;
        let ref_pid = load_ref_position_ids(parity_dir, entry)?;

        if rust_prep.pixel_position_ids == ref_pid {
            pos_ids_exact += 1;
        }
        if rust_prep.prompt_ids == entry.input_ids {
            prompt_ids_exact += 1;
        }

        let mut img_max = 0.0f32;
        let mut img_sum = 0.0f64;
        let valid_floats = rust_prep.valid_patches * PATCH_FLOATS;
        if rust_prep.pixel_values.len() != ref_pv.len() {
            return Err(ParityError::ParityGateFailed(format!(
                "Shape length mismatch for {}: rust {} vs ref {}",
                entry.key,
                rust_prep.pixel_values.len(),
                ref_pv.len()
            )));
        }
        for (&r, &p) in rust_prep.pixel_values.iter().zip(ref_pv.iter()) {
            let d = (r - p).abs();
            if d > img_max {
                img_max = d;
            }
            img_sum += d as f64;
        }
        let img_mean_valid = if valid_floats > 0 {
            img_sum / (valid_floats as f64)
        } else {
            0.0
        };
        if verbose {
            println!(
                "  {:<36} patches={:>4}/{:<4} max_diff={:.6} mean_diff(valid)={:.6}",
                entry.key, rust_prep.valid_patches, rust_prep.max_patches, img_max, img_mean_valid
            );
        }
        if img_max > global_max_diff {
            global_max_diff = img_max;
        }
        if img_max > worst_max_diff
            || (img_max == worst_max_diff && img_mean_valid > worst_mean_diff)
        {
            worst_max_diff = img_max;
            worst_mean_diff = img_mean_valid;
            worst_key = entry.key.clone();
        }
        global_abs_sum += img_sum;
        global_elems += valid_floats;
    }

    Ok(TensorParityReport {
        resizer,
        pos_ids_exact,
        prompt_ids_exact,
        total_entries: idx.entries.len(),
        max_abs_diff: global_max_diff,
        mean_abs_diff: if global_elems > 0 {
            global_abs_sum / (global_elems as f64)
        } else {
            0.0
        },
        worst_key,
        worst_max_diff,
        worst_mean_diff,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn run_full_compare(
    provider: &mut EmbeddingGemma2Provider,
    repo_root: &Path,
    parity_dir: &Path,
    wrong_mode: Option<WrongPreprocessMode>,
    preview_test: bool,
    compare_resizers: bool,
    max_images: Option<usize>,
) -> Result<()> {
    println!("=== 1. TOKENIZATION PARITY ===");
    let (tok_pass, tok_fail) = run_token_parity(provider.tokenizer(), parity_dir)?;
    println!(
        "Token parity (query + doc + edge cases, max_len=8192): {}/{} exact matches ({} failed)",
        tok_pass,
        tok_pass + tok_fail,
        tok_fail
    );

    println!("\n=== 2. IMAGE PREPROCESSING TENSOR PARITY ===");
    if compare_resizers {
        for backend in [
            ResizerBackend::FastCatmullRom,
            ResizerBackend::FastMitchell,
            ResizerBackend::ImageCatmullRom,
        ] {
            let r = run_tensor_parity(repo_root, parity_dir, backend, wrong_mode, false)?;
            println!(
                "  [{:?}] pos_ids={}/{} prompt_ids={}/{} max_abs={:.6} mean_abs(valid)={:.6} worst={} (max={:.6}, mean={:.6})",
                r.resizer,
                r.pos_ids_exact,
                r.total_entries,
                r.prompt_ids_exact,
                r.total_entries,
                r.max_abs_diff,
                r.mean_abs_diff,
                r.worst_key,
                r.worst_max_diff,
                r.worst_mean_diff
            );
        }
    }
    let tensor_report = run_tensor_parity(
        repo_root,
        parity_dir,
        provider.config().resizer,
        wrong_mode,
        true,
    )?;
    println!(
        "Tensor summary ({:?}): pos_ids={}/{} exact, prompt_ids={}/{} exact, global_max_diff={:.6}, global_mean_diff(valid)={:.6}, worst={} (max={:.6}, mean={:.6})",
        tensor_report.resizer,
        tensor_report.pos_ids_exact,
        tensor_report.total_entries,
        tensor_report.prompt_ids_exact,
        tensor_report.total_entries,
        tensor_report.max_abs_diff,
        tensor_report.mean_abs_diff,
        tensor_report.worst_key,
        tensor_report.worst_max_diff,
        tensor_report.worst_mean_diff
    );

    println!("\n=== 3. VECTOR PARITY (768d & 256d) ===");
    let raw_vi = std::fs::read_to_string(parity_dir.join("vectors_index.json"))?;
    let vi: VectorsIndexFile = serde_json::from_str(&raw_vi)?;

    let text_count = vi.text_ids.len();
    let img_count = max_images
        .unwrap_or(vi.image_names.len())
        .min(vi.image_names.len());

    let q8_text_768 = read_f32_vectors(
        &parity_dir.join(&vi.files.q8_text),
        text_count,
        NATIVE_EMBEDDING_DIM,
    )?;
    let fp32_text_768 = read_f32_vectors(
        &parity_dir.join(&vi.files.fp32_text),
        text_count,
        NATIVE_EMBEDDING_DIM,
    )?;

    // Run Rust text inference one item at a time cleanly to measure strict parity (no batch padding effects vs single-item Python).
    let mut rust_text_768 = Vec::with_capacity(text_count);
    for t in &vi.texts {
        let (mut vecs, _) = provider.embed_formatted_texts(&[t.as_str()], NATIVE_EMBEDDING_DIM)?;
        rust_text_768.push(vecs.remove(0));
    }

    let text_gate_ok = evaluate_vector_pair(
        "text",
        &vi.text_ids,
        &rust_text_768,
        &q8_text_768,
        &fp32_text_768,
        0.9999,
        0.999,
    )?;

    let mut img_gate_ok = true;
    for (budget, q8_rel, fp32_rel) in [
        (
            EmbeddingBudget::B140,
            &vi.files.q8_b140,
            &vi.files.fp32_b140,
        ),
        (
            EmbeddingBudget::B280,
            &vi.files.q8_b280,
            &vi.files.fp32_b280,
        ),
    ] {
        let all_q8 = read_f32_vectors(
            &parity_dir.join(q8_rel),
            vi.image_names.len(),
            NATIVE_EMBEDDING_DIM,
        )?;
        let all_fp32 = read_f32_vectors(
            &parity_dir.join(fp32_rel),
            vi.image_names.len(),
            NATIVE_EMBEDDING_DIM,
        )?;
        let ref_q8 = &all_q8[..img_count];
        let ref_fp32 = &all_fp32[..img_count];

        let mut rust_img_768 = Vec::with_capacity(img_count);
        for rel_path in &vi.image_rel_paths[..img_count] {
            let dyn_img = image::open(repo_root.join(rel_path))
                .map_err(|e| ParityError::Image(format!("Failed to open {rel_path}: {e}")))?;
            let (mut vecs, _) = provider.embed_images_with_timing(
                &[dyn_img],
                budget,
                NATIVE_EMBEDDING_DIM,
                wrong_mode,
            )?;
            rust_img_768.push(vecs.remove(0));
        }

        let label = format!("image_b{}", budget.soft_tokens());
        let ok = evaluate_vector_pair(
            &label,
            &vi.image_names[..img_count],
            &rust_img_768,
            ref_q8,
            ref_fp32,
            0.999,
            0.995,
        )?;
        img_gate_ok = img_gate_ok && ok;
    }

    if preview_test {
        println!(
            "\n=== 4. APP-SHAPED STAND-IN PREVIEW TEST (JPEG q80, max_edge=1024, Rust `image` crate) ==="
        );
        let idx_raw = std::fs::read_to_string(parity_dir.join("tensors_index.json"))?;
        let tidx: TensorsIndexFile = serde_json::from_str(&idx_raw)?;
        let mut seen = std::collections::HashSet::new();
        let mut subset_indices = Vec::new();
        for entry in &tidx.entries {
            if seen.insert(entry.image_name.clone())
                && let Some(pos) = vi.image_names.iter().position(|n| n == &entry.image_name)
            {
                subset_indices.push((pos, entry.image_name.clone(), entry.image_rel_path.clone()));
            }
        }
        for (budget, q8_rel, fp32_rel) in [
            (
                EmbeddingBudget::B140,
                &vi.files.q8_b140,
                &vi.files.fp32_b140,
            ),
            (
                EmbeddingBudget::B280,
                &vi.files.q8_b280,
                &vi.files.fp32_b280,
            ),
        ] {
            let all_q8 = read_f32_vectors(
                &parity_dir.join(q8_rel),
                vi.image_names.len(),
                NATIVE_EMBEDDING_DIM,
            )?;
            let all_fp32 = read_f32_vectors(
                &parity_dir.join(fp32_rel),
                vi.image_names.len(),
                NATIVE_EMBEDDING_DIM,
            )?;
            let mut rust_prev_vecs = Vec::with_capacity(subset_indices.len());
            let mut sub_q8 = Vec::with_capacity(subset_indices.len());
            let mut sub_fp32 = Vec::with_capacity(subset_indices.len());
            let mut sub_names = Vec::with_capacity(subset_indices.len());

            for (pos, name, rel_path) in &subset_indices {
                let orig = image::open(repo_root.join(rel_path))
                    .map_err(|e| ParityError::Image(format!("Failed to open {rel_path}: {e}")))?;
                let preview = encode_decode_standin_preview(&orig, 1024, 80)?;
                let (mut vecs, _) = provider.embed_images_with_timing(
                    &[preview],
                    budget,
                    NATIVE_EMBEDDING_DIM,
                    None,
                )?;
                rust_prev_vecs.push(vecs.remove(0));
                sub_q8.push(all_q8[*pos].clone());
                sub_fp32.push(all_fp32[*pos].clone());
                sub_names.push(name.clone());
            }

            let label = format!("preview_b{}", budget.soft_tokens());
            let _ = evaluate_vector_pair(
                &label,
                &sub_names,
                &rust_prev_vecs,
                &sub_q8,
                &sub_fp32,
                0.0,
                0.0,
            )?;
        }
    }

    if tok_fail > 0 {
        return Err(ParityError::ParityGateFailed(format!(
            "Token parity failed on {tok_fail} case(s)"
        )));
    }
    if wrong_mode.is_none()
        && (tensor_report.pos_ids_exact != tensor_report.total_entries
            || tensor_report.prompt_ids_exact != tensor_report.total_entries)
    {
        return Err(ParityError::ParityGateFailed(format!(
            "Position/prompt ID parity failed: pos={}/{}, prompt={}/{}",
            tensor_report.pos_ids_exact,
            tensor_report.total_entries,
            tensor_report.prompt_ids_exact,
            tensor_report.total_entries
        )));
    }
    if !text_gate_ok || !img_gate_ok {
        return Err(ParityError::ParityGateFailed(
            "One or more embedding vector cosine gates failed".into(),
        ));
    }

    println!("\nALL PARITY GATES PASSED.");
    Ok(())
}

fn evaluate_vector_pair(
    name: &str,
    labels: &[String],
    rust_768: &[Vec<f32>],
    q8_768: &[Vec<f32>],
    fp32_768: &[Vec<f32>],
    gate_mean_min: f32,
    gate_min_min: f32,
) -> Result<bool> {
    let mut all_ok = true;
    for dim in [NATIVE_EMBEDDING_DIM, DEFAULT_STORED_DIM] {
        let mut r_vs_q8 = Vec::with_capacity(labels.len());
        let mut r_vs_fp32 = Vec::with_capacity(labels.len());
        let mut q8_vs_fp32 = Vec::with_capacity(labels.len());

        for i in 0..labels.len() {
            let rv = truncate_and_l2_normalize(&rust_768[i], dim)?;
            let qv = truncate_and_l2_normalize(&q8_768[i], dim)?;
            let fv = truncate_and_l2_normalize(&fp32_768[i], dim)?;
            r_vs_q8.push(cosine_similarity(&rv, &qv)?);
            r_vs_fp32.push(cosine_similarity(&rv, &fv)?);
            q8_vs_fp32.push(cosine_similarity(&qv, &fv)?);
        }

        let s_rq = summarize_scores(&r_vs_q8, labels)?;
        let s_rf = summarize_scores(&r_vs_fp32, labels)?;
        let s_qf = summarize_scores(&q8_vs_fp32, labels)?;
        let fp32_delta = s_rf.mean - s_qf.mean;

        let is_info_only = gate_mean_min == 0.0 && gate_min_min == 0.0;
        let pass = is_info_only
            || (s_rq.mean >= gate_mean_min && s_rq.min >= gate_min_min && fp32_delta >= -0.001);
        if !pass {
            all_ok = false;
        }
        let status = if is_info_only {
            "INFO"
        } else if pass {
            "PASS"
        } else {
            "FAIL"
        };

        println!(
            "  [{status}] {name:<11} {dim:>3}d (n={}): rust_vs_q8 mean={:.6} min={:.6} (worst={}) | rust_vs_fp32 mean={:.6} min={:.6} | py_q8_vs_fp32 mean={:.6} min={:.6} | delta_mean={:+.6}",
            labels.len(),
            s_rq.mean,
            s_rq.min,
            s_rq.worst_label,
            s_rf.mean,
            s_rf.min,
            s_qf.mean,
            s_qf.min,
            fp32_delta
        );
    }
    Ok(all_ok)
}
