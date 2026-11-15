use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

use embedding_parity::bench::{
    BenchMode, run_b70_vs_b140, run_ep_compare, run_single_memory_probe, run_thread_sweep,
};
use embedding_parity::parity::run_full_compare;
use embedding_parity::preprocess::{
    EmbeddingBudget, ResizerBackend, WrongPreprocessMode, preprocess_image_path,
};
use embedding_parity::provider::{
    DEFAULT_STORED_DIM, EmbeddingGemma2Provider, ExecutionProviderKind, NATIVE_EMBEDDING_DIM,
    ProviderConfig, default_ort_lib_path,
};
use embedding_parity::tokenizer::TextTokenizer;

#[derive(Parser)]
#[command(
    name = "embedding-parity",
    about = "EmbeddingGemma 2 Rust parity & benchmark CLI"
)]
struct Cli {
    #[arg(
        long,
        default_value = "spikes/embeddings/models/onnx",
        help = "Directory holding tokenizer.json and onnx/*.onnx"
    )]
    model_dir: PathBuf,

    #[arg(
        long,
        help = "Path to pinned ONNX Runtime dynamic library (.dylib/.dll). Defaults to spikes/embeddings/models/ort-runtime/..."
    )]
    ort_lib: Option<PathBuf>,

    #[arg(long, default_value = ".", help = "Repository root directory")]
    repo_root: PathBuf,

    #[arg(long, value_enum, default_value_t = ResizerBackend::FastCatmullRom)]
    resizer: ResizerBackend,

    #[arg(long, default_value_t = 4)]
    threads: usize,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print token ids for a query or document string.
    Tokens {
        text: String,
        #[arg(long, default_value_t = false)]
        document: bool,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, default_value_t = false)]
        raw: bool,
    },

    /// Preprocess one image and print tensor shape/checksum summary (optionally write f32/i64).
    Preprocess {
        image: PathBuf,
        #[arg(long, value_enum, default_value_t = EmbeddingBudget::B140)]
        budget: EmbeddingBudget,
        #[arg(long)]
        out_prefix: Option<PathBuf>,
    },

    /// Embed a text query or document and print vector norm and first 8 dimensions.
    EmbedText {
        text: String,
        #[arg(long, default_value_t = false)]
        document: bool,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, default_value_t = DEFAULT_STORED_DIM)]
        dim: usize,
    },

    /// Embed an image and print timing split, norm, and first 8 dimensions.
    EmbedImage {
        image: PathBuf,
        #[arg(long, value_enum, default_value_t = EmbeddingBudget::B140)]
        budget: EmbeddingBudget,
        #[arg(long, default_value_t = DEFAULT_STORED_DIM)]
        dim: usize,
    },

    /// Run full token, preprocessed-tensor, and vector parity gates against committed Python references.
    Compare {
        #[arg(long, default_value = "spikes/embeddings/parity")]
        parity_dir: PathBuf,
        #[arg(long, value_enum)]
        wrong_mode: Option<WrongPreprocessMode>,
        #[arg(long, default_value_t = false)]
        preview_test: bool,
        #[arg(long, default_value_t = false)]
        compare_resizers: bool,
        #[arg(long)]
        max_images: Option<usize>,
    },

    /// Run Part D speed, thread-sweep, execution-provider, or clean-process memory benchmarks.
    Bench {
        #[arg(long, default_value = "spikes/embeddings/parity")]
        parity_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = BenchMode::Sweep)]
        mode: BenchMode,
        #[arg(long, value_delimiter = ',', default_value = "1,2,4,6,8,10")]
        sweep_threads: Vec<usize>,
        #[arg(long, value_enum, value_delimiter = ',', default_value = "70,140,280")]
        budgets: Vec<EmbeddingBudget>,
        #[arg(long, value_delimiter = ',', default_value = "1,2,4")]
        batches: Vec<usize>,
        #[arg(long, default_value_t = 12)]
        num_images: usize,
        #[arg(long, value_enum, default_value_t = ExecutionProviderKind::CoreMl)]
        vision_ep: ExecutionProviderKind,
        #[arg(long, value_enum, default_value_t = EmbeddingBudget::B140)]
        budget: EmbeddingBudget,
        #[arg(
            long,
            default_value = "spikes/embeddings/fixtures/images/01_red_circle_on_blue.png"
        )]
        sample_image: PathBuf,
    },
}

fn resolve_ort_lib(cli: &Cli) -> PathBuf {
    cli.ort_lib
        .clone()
        .unwrap_or_else(|| default_ort_lib_path(&cli.repo_root))
}

fn make_provider_config(cli: &Cli, budget: EmbeddingBudget) -> ProviderConfig {
    ProviderConfig {
        model_dir: cli.model_dir.clone(),
        ort_lib_path: resolve_ort_lib(cli),
        intra_threads: cli.threads,
        inter_threads: 1,
        budget,
        resizer: cli.resizer,
        backbone_ep: ExecutionProviderKind::Cpu,
        vision_ep: ExecutionProviderKind::Cpu,
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Tokens {
            text,
            document,
            title,
            raw,
        } => {
            let tokenizer = TextTokenizer::from_file(&cli.model_dir.join("tokenizer.json"))?;
            let formatted = if *raw {
                text.clone()
            } else if *document {
                TextTokenizer::format_document(title.as_deref(), text)
            } else {
                TextTokenizer::format_query(text)
            };
            let enc = tokenizer.encode_formatted(&formatted)?;
            println!("formatted: {:?}", formatted);
            println!("len: {}", enc.input_ids.len());
            println!("input_ids: {:?}", enc.input_ids);
            println!("attention_mask: {:?}", enc.attention_mask);
        }

        Commands::Preprocess {
            image,
            budget,
            out_prefix,
        } => {
            let prep = preprocess_image_path(image, *budget, cli.resizer, None)?;
            println!(
                "image={:?} budget={} target={}x{} patches={}/{} soft_tokens={}",
                image,
                budget.soft_tokens(),
                prep.target_width,
                prep.target_height,
                prep.valid_patches,
                prep.max_patches,
                prep.num_soft_tokens
            );
            if let Some(prefix) = out_prefix {
                write_f32_le(&with_suffix(prefix, "_pv.f32"), &prep.pixel_values)?;
                write_i64_le(&with_suffix(prefix, "_pid.i64"), &prep.pixel_position_ids)?;
                println!("wrote tensor outputs with prefix {:?}", prefix);
            }
        }

        Commands::EmbedText {
            text,
            document,
            title,
            dim,
        } => {
            let cfg = make_provider_config(&cli, EmbeddingBudget::B140);
            let mut provider = EmbeddingGemma2Provider::new(cfg)?;
            let formatted = if *document {
                TextTokenizer::format_document(title.as_deref(), text)
            } else {
                TextTokenizer::format_query(text)
            };
            let (vecs, dur) = provider.embed_formatted_texts(&[formatted.as_str()], *dim)?;
            let v = &vecs[0];
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            println!(
                "dim={} norm={:.6} latency={:.2}ms head={:?}",
                v.len(),
                norm,
                dur.as_secs_f64() * 1000.0,
                &v[..v.len().min(8)]
            );
        }

        Commands::EmbedImage { image, budget, dim } => {
            let cfg = make_provider_config(&cli, *budget);
            let mut provider = EmbeddingGemma2Provider::new(cfg)?;
            let dyn_img = image::open(image)?;
            let (vecs, timing) =
                provider.embed_images_with_timing(&[dyn_img], *budget, *dim, None)?;
            let v = &vecs[0];
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            println!(
                "dim={} budget={} norm={:.6} prep={:.1}ms vis={:.1}ms back={:.1}ms tot={:.1}ms head={:?}",
                v.len(),
                budget.soft_tokens(),
                norm,
                timing.preprocess_ms,
                timing.vision_ms,
                timing.backbone_ms,
                timing.total_ms,
                &v[..v.len().min(8)]
            );
        }

        Commands::Compare {
            parity_dir,
            wrong_mode,
            preview_test,
            compare_resizers,
            max_images,
        } => {
            let cfg = make_provider_config(&cli, EmbeddingBudget::B140);
            let mut provider = EmbeddingGemma2Provider::new(cfg)?;
            run_full_compare(
                &mut provider,
                &cli.repo_root,
                parity_dir,
                *wrong_mode,
                *preview_test,
                *compare_resizers,
                *max_images,
            )?;
        }

        Commands::Bench {
            parity_dir,
            mode,
            sweep_threads,
            budgets,
            batches,
            num_images,
            vision_ep,
            budget,
            sample_image,
        } => {
            let cfg = make_provider_config(&cli, *budget);
            match mode {
                BenchMode::Sweep => {
                    run_thread_sweep(
                        &cfg,
                        &cli.repo_root,
                        parity_dir,
                        sweep_threads,
                        budgets,
                        batches,
                        *num_images,
                    )?;
                }
                BenchMode::EpCompare => {
                    run_ep_compare(
                        &cfg,
                        &cli.repo_root,
                        parity_dir,
                        *vision_ep,
                        *budget,
                        *num_images,
                    )?;
                }
                BenchMode::B70VsB140 => {
                    run_b70_vs_b140(&cfg, &cli.repo_root, parity_dir)?;
                }
                BenchMode::SingleText | BenchMode::SingleImage | BenchMode::SingleDropVision => {
                    run_single_memory_probe(&cfg, *mode, *budget, sample_image)?;
                }
            }
        }
    }

    let _ = NATIVE_EMBEDDING_DIM;
    Ok(())
}

fn with_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn write_f32_le(path: &Path, vals: &[f32]) -> Result<()> {
    let mut bytes = Vec::with_capacity(vals.len() * 4);
    for &v in vals {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_i64_le(path: &Path, vals: &[i64]) -> Result<()> {
    let mut bytes = Vec::with_capacity(vals.len() * 8);
    for &v in vals {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes)?;
    Ok(())
}
