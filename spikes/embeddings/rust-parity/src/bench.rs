use std::path::Path;
use std::time::Instant;

use clap::ValueEnum;
use image::DynamicImage;

use crate::error::{ParityError, Result};
use crate::parity::{TensorsIndexFile, VectorsIndexFile, summarize_scores};
use crate::preprocess::EmbeddingBudget;
use crate::provider::{
    DEFAULT_STORED_DIM, EmbeddingGemma2Provider, ExecutionProviderKind, NATIVE_EMBEDDING_DIM,
    ProviderConfig, cosine_similarity,
};
use crate::tokenizer::TextTokenizer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BenchMode {
    Sweep,
    EpCompare,
    B70VsB140,
    SingleText,
    SingleImage,
    SingleDropVision,
}

pub fn load_bench_images(
    repo_root: &Path,
    parity_dir: &Path,
    count: usize,
) -> Result<(Vec<String>, Vec<DynamicImage>)> {
    let raw = std::fs::read_to_string(parity_dir.join("vectors_index.json"))?;
    let vi: VectorsIndexFile = serde_json::from_str(&raw)?;
    let n = count.min(vi.image_rel_paths.len());
    let mut names = Vec::with_capacity(n);
    let mut imgs = Vec::with_capacity(n);
    for (name, rel) in vi.image_names[..n]
        .iter()
        .zip(vi.image_rel_paths[..n].iter())
    {
        let img = image::open(repo_root.join(rel))
            .map_err(|e| ParityError::Image(format!("Failed to open {rel}: {e}")))?;
        names.push(name.clone());
        imgs.push(img);
    }
    Ok((names, imgs))
}

pub fn load_bench_queries(parity_dir: &Path, count: usize) -> Result<Vec<String>> {
    let raw = std::fs::read_to_string(parity_dir.join("vectors_index.json"))?;
    let vi: VectorsIndexFile = serde_json::from_str(&raw)?;
    Ok(vi.texts.into_iter().take(count).collect())
}

pub fn run_thread_sweep(
    base_config: &ProviderConfig,
    repo_root: &Path,
    parity_dir: &Path,
    threads: &[usize],
    budgets: &[EmbeddingBudget],
    batches: &[usize],
    num_images: usize,
) -> Result<()> {
    let (_names, imgs) = load_bench_images(repo_root, parity_dir, num_images)?;
    let queries = load_bench_queries(parity_dir, 20)?;

    println!(
        "=== THREAD SWEEP & BATCH BENCHMARK (images={}, queries={}, resizer={:?}) ===",
        imgs.len(),
        queries.len(),
        base_config.resizer
    );

    for &t in threads {
        let mut cfg = base_config.clone();
        cfg.intra_threads = t;
        cfg.inter_threads = 1;
        let mut provider = EmbeddingGemma2Provider::new(cfg)?;
        provider.ensure_vision_loaded()?;
        let back_load_ms = provider.backbone_load_ms;
        let vis_load_ms = provider.vision_load_ms.unwrap_or(0.0);

        // Warmup text + image
        let first_q = queries
            .first()
            .map(String::as_str)
            .unwrap_or("task: search result | query: test");
        let _ = provider.embed_formatted_texts(&[first_q], DEFAULT_STORED_DIM)?;
        let _ = provider.embed_images_with_timing(
            &imgs[..1],
            EmbeddingBudget::B140,
            DEFAULT_STORED_DIM,
            None,
        )?;

        let t0_text = Instant::now();
        for q in &queries {
            let _ = provider.embed_formatted_texts(&[q.as_str()], DEFAULT_STORED_DIM)?;
        }
        let text_avg_ms = (t0_text.elapsed().as_secs_f64() * 1000.0) / (queries.len() as f64);
        println!(
            "\n[threads={t}] load(backbone={:.1}ms, vision={:.1}ms) | text_avg={:.2}ms/query ({:.1} q/s)",
            back_load_ms,
            vis_load_ms,
            text_avg_ms,
            1000.0 / text_avg_ms
        );

        for &budget in budgets {
            for &batch in batches {
                if batch == 0 || imgs.len() < batch {
                    continue;
                }
                let usable = (imgs.len() / batch) * batch;
                let mut sum_prep = 0.0;
                let mut sum_vis = 0.0;
                let mut sum_back = 0.0;
                let mut sum_tot = 0.0;
                let mut n_imgs = 0usize;

                let wall_start = Instant::now();
                for chunk in imgs[..usable].chunks_exact(batch) {
                    let (_vecs, timing) = provider.embed_images_with_timing(
                        chunk,
                        budget,
                        DEFAULT_STORED_DIM,
                        None,
                    )?;
                    sum_prep += timing.preprocess_ms;
                    sum_vis += timing.vision_ms;
                    sum_back += timing.backbone_ms;
                    sum_tot += timing.total_ms;
                    n_imgs += chunk.len();
                }
                let wall_ms = wall_start.elapsed().as_secs_f64() * 1000.0;
                let n = n_imgs as f64;
                println!(
                    "  budget={:>3} batch={}: per_img total={:>7.1}ms (prep={:>4.1}ms vis={:>7.1}ms back={:>6.1}ms) | throughput={:.3} img/s | wall={:.1}ms",
                    budget.soft_tokens(),
                    batch,
                    sum_tot / n,
                    sum_prep / n,
                    sum_vis / n,
                    sum_back / n,
                    n / (wall_ms / 1000.0),
                    wall_ms
                );
            }
        }
    }
    Ok(())
}

pub fn run_ep_compare(
    base_config: &ProviderConfig,
    repo_root: &Path,
    parity_dir: &Path,
    ep_to_test: ExecutionProviderKind,
    budget: EmbeddingBudget,
    num_images: usize,
) -> Result<()> {
    let (names, imgs) = load_bench_images(repo_root, parity_dir, num_images)?;

    println!(
        "=== EXECUTION PROVIDER TEST: CPU vs {:?} (vision_ep={:?}, backbone_ep=Cpu, budget={}, threads={}, n={}) ===",
        ep_to_test,
        ep_to_test,
        budget.soft_tokens(),
        base_config.intra_threads,
        imgs.len()
    );

    let mut cpu_cfg = base_config.clone();
    cpu_cfg.vision_ep = ExecutionProviderKind::Cpu;
    cpu_cfg.backbone_ep = ExecutionProviderKind::Cpu;
    let mut cpu_provider = EmbeddingGemma2Provider::new(cpu_cfg)?;
    cpu_provider.ensure_vision_loaded()?;
    let _ =
        cpu_provider.embed_images_with_timing(&imgs[..1], budget, NATIVE_EMBEDDING_DIM, None)?;

    let mut cpu_vecs = Vec::with_capacity(imgs.len());
    let mut cpu_vis_ms = 0.0;
    let mut cpu_tot_ms = 0.0;
    for img in std::slice::from_ref(&imgs) {
        let _ = img;
    }
    for img in &imgs {
        let (mut vecs, timing) = cpu_provider.embed_images_with_timing(
            std::slice::from_ref(img),
            budget,
            NATIVE_EMBEDDING_DIM,
            None,
        )?;
        cpu_vecs.push(vecs.remove(0));
        cpu_vis_ms += timing.vision_ms;
        cpu_tot_ms += timing.total_ms;
    }
    let n = imgs.len() as f64;
    println!(
        "  [CPU]   vision_load={:.1}ms | avg_vis={:.1}ms avg_tot={:.1}ms ({:.3} img/s)",
        cpu_provider.vision_load_ms.unwrap_or(0.0),
        cpu_vis_ms / n,
        cpu_tot_ms / n,
        1000.0 / (cpu_tot_ms / n)
    );
    drop(cpu_provider);

    let mut ep_cfg = base_config.clone();
    ep_cfg.vision_ep = ep_to_test;
    ep_cfg.backbone_ep = ExecutionProviderKind::Cpu;
    let mut ep_provider = EmbeddingGemma2Provider::new(ep_cfg)?;
    match ep_provider.ensure_vision_loaded() {
        Ok(()) => {}
        Err(err) => {
            println!(
                "  [{:?}] FAILED TO LOAD / REGISTER VISION EP: {err}",
                ep_to_test
            );
            return Err(err);
        }
    }
    // Warmup EP (CoreML/DirectML compiles/specializes graph on first run)
    let t_warm = Instant::now();
    let _ = ep_provider.embed_images_with_timing(&imgs[..1], budget, NATIVE_EMBEDDING_DIM, None)?;
    let warm_ms = t_warm.elapsed().as_secs_f64() * 1000.0;

    let mut ep_vecs = Vec::with_capacity(imgs.len());
    let mut ep_vis_ms = 0.0;
    let mut ep_tot_ms = 0.0;
    for img in &imgs {
        let (mut vecs, timing) = ep_provider.embed_images_with_timing(
            std::slice::from_ref(img),
            budget,
            NATIVE_EMBEDDING_DIM,
            None,
        )?;
        ep_vecs.push(vecs.remove(0));
        ep_vis_ms += timing.vision_ms;
        ep_tot_ms += timing.total_ms;
    }
    let mut cosines = Vec::with_capacity(imgs.len());
    for (c, e) in cpu_vecs.iter().zip(ep_vecs.iter()) {
        cosines.push(cosine_similarity(c, e)?);
    }
    let summary = summarize_scores(&cosines, &names)?;
    println!(
        "  [{:?}] vision_load={:.1}ms warmup={:.1}ms | avg_vis={:.1}ms avg_tot={:.1}ms ({:.3} img/s) | cos_vs_cpu mean={:.6} min={:.6} (worst={})",
        ep_to_test,
        ep_provider.vision_load_ms.unwrap_or(0.0),
        warm_ms,
        ep_vis_ms / n,
        ep_tot_ms / n,
        1000.0 / (ep_tot_ms / n),
        summary.mean,
        summary.min,
        summary.worst_label
    );
    Ok(())
}

pub fn run_b70_vs_b140(
    base_config: &ProviderConfig,
    repo_root: &Path,
    parity_dir: &Path,
) -> Result<()> {
    let raw = std::fs::read_to_string(parity_dir.join("tensors_index.json"))?;
    let tidx: TensorsIndexFile = serde_json::from_str(&raw)?;
    let mut seen = std::collections::HashSet::new();
    let mut names = Vec::new();
    let mut imgs = Vec::new();
    for entry in &tidx.entries {
        if seen.insert(entry.image_name.clone()) {
            names.push(entry.image_name.clone());
            imgs.push(
                image::open(repo_root.join(&entry.image_rel_path)).map_err(|e| {
                    ParityError::Image(format!("Failed to open {}: {e}", entry.image_rel_path))
                })?,
            );
        }
    }

    let mut provider = EmbeddingGemma2Provider::new(base_config.clone())?;
    provider.ensure_vision_loaded()?;
    let _ = provider.embed_images_with_timing(
        &imgs[..1],
        EmbeddingBudget::B70,
        NATIVE_EMBEDDING_DIM,
        None,
    )?;

    let mut v70 = Vec::with_capacity(imgs.len());
    let mut t70_tot = 0.0;
    for img in &imgs {
        let (mut v, t) = provider.embed_images_with_timing(
            std::slice::from_ref(img),
            EmbeddingBudget::B70,
            NATIVE_EMBEDDING_DIM,
            None,
        )?;
        v70.push(v.remove(0));
        t70_tot += t.total_ms;
    }

    let mut v140 = Vec::with_capacity(imgs.len());
    let mut t140_tot = 0.0;
    for img in &imgs {
        let (mut v, t) = provider.embed_images_with_timing(
            std::slice::from_ref(img),
            EmbeddingBudget::B140,
            NATIVE_EMBEDDING_DIM,
            None,
        )?;
        v140.push(v.remove(0));
        t140_tot += t.total_ms;
    }

    let mut cos_768 = Vec::with_capacity(imgs.len());
    for (a, b) in v70.iter().zip(v140.iter()) {
        cos_768.push(cosine_similarity(a, b)?);
    }
    let summary = summarize_scores(&cos_768, &names)?;
    let n = imgs.len() as f64;
    println!(
        "=== BUDGET 70 vs BUDGET 140 (n={}, threads={}) ===",
        imgs.len(),
        base_config.intra_threads
    );
    println!(
        "  b70 avg={:.1}ms ({:.3} img/s) | b140 avg={:.1}ms ({:.3} img/s) | speedup={:.2}x",
        t70_tot / n,
        1000.0 / (t70_tot / n),
        t140_tot / n,
        1000.0 / (t140_tot / n),
        t140_tot / t70_tot
    );
    println!(
        "  cosine(b70, b140) 768d: mean={:.6} min={:.6} max={:.6} (worst={})",
        summary.mean, summary.min, summary.max, summary.worst_label
    );
    Ok(())
}

pub fn run_single_memory_probe(
    base_config: &ProviderConfig,
    mode: BenchMode,
    budget: EmbeddingBudget,
    sample_image: &Path,
) -> Result<()> {
    let mut provider = EmbeddingGemma2Provider::new(base_config.clone())?;
    let q = TextTokenizer::format_query("red circle on blue background");
    let (vecs, dt) = provider.embed_formatted_texts(&[q.as_str()], DEFAULT_STORED_DIM)?;
    println!(
        "single-text done: load_backbone={:.1}ms infer={:.2}ms dim={} l2_first={:.6}",
        provider.backbone_load_ms,
        dt.as_secs_f64() * 1000.0,
        vecs[0].len(),
        vecs[0].iter().map(|v| v * v).sum::<f32>().sqrt()
    );
    if matches!(mode, BenchMode::SingleImage | BenchMode::SingleDropVision) {
        let img = image::open(sample_image)
            .map_err(|e| ParityError::Image(format!("Failed to open {:?}: {e}", sample_image)))?;
        let (ivecs, timing) =
            provider.embed_images_with_timing(&[img], budget, DEFAULT_STORED_DIM, None)?;
        println!(
            "single-image done: load_vision={:.1}ms prep={:.1}ms vis={:.1}ms back={:.1}ms tot={:.1}ms dim={}",
            provider.vision_load_ms.unwrap_or(0.0),
            timing.preprocess_ms,
            timing.vision_ms,
            timing.backbone_ms,
            timing.total_ms,
            ivecs[0].len()
        );
        if mode == BenchMode::SingleDropVision {
            provider.drop_vision_encoder();
            let _ = provider.embed_formatted_texts(&[q.as_str()], DEFAULT_STORED_DIM)?;
            println!(
                "vision encoder dropped; text-only still active={}",
                !provider.has_vision_encoder()
            );
        }
    }
    Ok(())
}
