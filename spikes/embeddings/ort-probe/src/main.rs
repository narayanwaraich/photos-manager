use anyhow::Result;
use clap::Parser;
use ort::session::builder::AutoDevicePolicy;
use ort::session::Session;
use ort::value::Tensor;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "ort-probe",
    about = "Probe and validate EmbeddingGemma 2 ONNX models in Rust via ort"
)]
struct Args {
    #[arg(
        long,
        default_value = "spikes/embeddings/models/onnx/onnx/model_quantized.onnx",
        help = "Path to text/backbone ONNX model file"
    )]
    model_path: PathBuf,

    #[arg(
        long,
        default_value = "spikes/embeddings/models/onnx/onnx/vision_encoder_quantized.onnx",
        help = "Path to vision encoder ONNX model file"
    )]
    vision_model_path: PathBuf,

    #[arg(long, help = "Run dummy inference test on both models")]
    run_dummy: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("=== ORT-PROBE: Validating ONNX Runtime in Rust ===");

    let env = ort::init().with_name("ort_probe").build()?;

    println!("Loading text model from: {:?}", args.model_path);
    if !args.model_path.exists() {
        eprintln!("Warning: model file does not exist at {:?}", args.model_path);
        return Ok(());
    }

    let t0 = Instant::now();
    let mut text_session = Session::builder(&env)?
        .with_auto_device(AutoDevicePolicy::PreferCPU)
        .map_err(|e| anyhow::anyhow!("Failed to set auto device policy: {:?}", e))?
        .commit_from_file(&args.model_path)?;
    println!(
        "Text model loaded successfully in {:.2}ms",
        t0.elapsed().as_secs_f64() * 1000.0
    );

    println!("\nText Model Inputs:");
    for input in text_session.inputs() {
        println!("  - name: {}, dtype: {:?}", input.name(), input.dtype());
    }
    println!("\nText Model Outputs:");
    for output in text_session.outputs() {
        println!("  - name: {}, dtype: {:?}", output.name(), output.dtype());
    }

    if args.vision_model_path.exists() {
        println!(
            "\nLoading vision encoder from: {:?}",
            args.vision_model_path
        );
        let t1 = Instant::now();
        let vision_session = Session::builder(&env)?
            .with_auto_device(AutoDevicePolicy::PreferCPU)
            .map_err(|e| anyhow::anyhow!("Failed to set auto device policy: {:?}", e))?
            .commit_from_file(&args.vision_model_path)?;
        println!(
            "Vision encoder loaded successfully in {:.2}ms",
            t1.elapsed().as_secs_f64() * 1000.0
        );

        println!("\nVision Encoder Inputs:");
        for input in vision_session.inputs() {
            println!("  - name: {}, dtype: {:?}", input.name(), input.dtype());
        }
        println!("\nVision Encoder Outputs:");
        for output in vision_session.outputs() {
            println!("  - name: {}, dtype: {:?}", output.name(), output.dtype());
        }
    }

    if args.run_dummy {
        println!("\n--- Running Dummy Inference (Text) ---");
        let seq_len = 16;
        let input_ids = Tensor::from_array((vec![1, seq_len], vec![0i64; seq_len]))?;
        let attention_mask = Tensor::from_array((vec![1, seq_len], vec![1i64; seq_len]))?;
        let empty_feats1 = Tensor::from_array((vec![0, 512], Vec::<f32>::new()))?;
        let empty_feats2 = Tensor::from_array((vec![0, 512], Vec::<f32>::new()))?;
        let empty_feats3 = Tensor::from_array((vec![0, 512], Vec::<f32>::new()))?;

        let inputs = ort::inputs![
            "input_ids" => input_ids,
            "attention_mask" => attention_mask,
            "image_features" => empty_feats1,
            "video_features" => empty_feats2,
            "audio_features" => empty_feats3,
        ];

        let t_infer = Instant::now();
        let outputs = text_session.run(inputs)?;
        let elapsed = t_infer.elapsed().as_secs_f64() * 1000.0;
        println!("Inference completed in {:.2}ms", elapsed);

        if let Some(embedding) = outputs.get("sentence_embedding") {
            let shape = embedding.shape();
            println!("Output sentence_embedding shape: {:?}", shape);
            assert_eq!(shape.as_ref(), &[1, 768]);
            println!("Successfully verified 768-d embedding output from Rust ort!");
        }
    }

    println!("\nValidation complete.");
    Ok(())
}
