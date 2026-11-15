use std::path::Path;

use tokenizers::{
    PaddingDirection, PaddingParams, PaddingStrategy, Tokenizer, TruncationDirection,
    TruncationParams, TruncationStrategy,
};

use crate::error::{ParityError, Result};

/// Maximum text sequence length for EmbeddingGemma 2 (matching `tokenizer_config.json`).
pub const MAX_TEXT_SEQ_LEN: usize = 8192;
pub const PAD_TOKEN_ID: i64 = 0;
pub const EOS_TOKEN_ID: i64 = 1;
pub const BOS_TOKEN_ID: i64 = 2;
pub const UNK_TOKEN_ID: i64 = 3;
pub const IMAGE_BOI_TOKEN_ID: i64 = 255_999;
pub const IMAGE_SOFT_TOKEN_ID: i64 = 258_880;
pub const IMAGE_EOI_TOKEN_ID: i64 = 258_882;

pub const QUERY_PREFIX: &str = "task: search result | query: ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedText {
    pub input_ids: Vec<i64>,
    pub attention_mask: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchEncodedText {
    pub batch_size: usize,
    pub seq_len: usize,
    pub input_ids: Vec<i64>,
    pub attention_mask: Vec<i64>,
}

pub struct TextTokenizer {
    inner: Tokenizer,
}

impl TextTokenizer {
    /// Load `tokenizer.json` and enforce exact EmbeddingGemma 2 truncation and padding settings.
    pub fn from_file(path: &Path) -> Result<Self> {
        let mut inner = Tokenizer::from_file(path).map_err(|e| {
            ParityError::Tokenizer(format!("Failed to load tokenizer {:?}: {e}", path))
        })?;

        // Enable right-truncation at 8192 tokens; TemplateProcessing preserves <bos> and <eos>.
        inner
            .with_truncation(Some(TruncationParams {
                direction: TruncationDirection::Right,
                max_length: MAX_TEXT_SEQ_LEN,
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
            }))
            .map_err(|e| ParityError::Tokenizer(format!("Failed to configure truncation: {e}")))?;

        inner.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            direction: PaddingDirection::Right,
            pad_to_multiple_of: None,
            pad_id: PAD_TOKEN_ID as u32,
            pad_type_id: 0,
            pad_token: "<pad>".to_string(),
        }));

        Ok(Self { inner })
    }

    pub fn format_query(query: &str) -> String {
        format!("{QUERY_PREFIX}{query}")
    }

    pub fn format_document(title: Option<&str>, text: &str) -> String {
        let clean_title = match title {
            Some(t) if !t.is_empty() => t,
            _ => "none",
        };
        format!("title: {clean_title} | text: {text}")
    }

    /// Encode an already-formatted text string with special tokens (<bos>, <eos>) enabled.
    pub fn encode_formatted(&self, text: &str) -> Result<EncodedText> {
        let enc = self
            .inner
            .encode(text, true)
            .map_err(|e| ParityError::Tokenizer(format!("Encode failed: {e}")))?;
        Ok(EncodedText {
            input_ids: enc.get_ids().iter().map(|&id| id as i64).collect(),
            attention_mask: enc.get_attention_mask().iter().map(|&m| m as i64).collect(),
        })
    }

    /// Encode a batch of already-formatted strings, right-padding to the longest item in the batch.
    pub fn encode_batch_formatted(&self, texts: &[&str]) -> Result<BatchEncodedText> {
        if texts.is_empty() {
            return Err(ParityError::InvalidInput(
                "Text batch cannot be empty".into(),
            ));
        }
        let encodings = self
            .inner
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| ParityError::Tokenizer(format!("Batch encode failed: {e}")))?;

        let seq_len = encodings.iter().map(|e| e.len()).max().unwrap_or(0);
        let batch_size = encodings.len();
        let mut input_ids = Vec::with_capacity(batch_size * seq_len);
        let mut attention_mask = Vec::with_capacity(batch_size * seq_len);
        for enc in encodings {
            input_ids.extend(enc.get_ids().iter().map(|&id| id as i64));
            attention_mask.extend(enc.get_attention_mask().iter().map(|&m| m as i64));
        }

        Ok(BatchEncodedText {
            batch_size,
            seq_len,
            input_ids,
            attention_mask,
        })
    }
}

/// Build the backbone `input_ids` and `attention_mask` for an image with `num_soft_tokens`.
/// Layout verified from `processing_embedding_gemma2.py` & reference output:
/// `[2 (<bos>), 255999 (<|image|>), 258880 (<|image_soft|>) x N, 258882 (<|/image|>), 1 (<eos>)]`
pub fn build_image_prompt_ids(num_soft_tokens: usize) -> Vec<i64> {
    let mut ids = Vec::with_capacity(num_soft_tokens + 4);
    ids.push(BOS_TOKEN_ID);
    ids.push(IMAGE_BOI_TOKEN_ID);
    ids.resize(ids.len() + num_soft_tokens, IMAGE_SOFT_TOKEN_ID);
    ids.push(IMAGE_EOI_TOKEN_ID);
    ids.push(EOS_TOKEN_ID);
    ids
}

/// Right-pad a batch of per-image token sequences with `pad_token_id = 0`.
pub fn pad_image_prompt_batch(seqs: &[Vec<i64>]) -> Result<BatchEncodedText> {
    if seqs.is_empty() {
        return Err(ParityError::InvalidInput(
            "Image prompt batch cannot be empty".into(),
        ));
    }
    let batch_size = seqs.len();
    let seq_len = seqs.iter().map(Vec::len).max().unwrap_or(0);
    let mut input_ids = vec![PAD_TOKEN_ID; batch_size * seq_len];
    let mut attention_mask = vec![0i64; batch_size * seq_len];

    for (b, seq) in seqs.iter().enumerate() {
        let row_start = b * seq_len;
        for (t, &tok) in seq.iter().enumerate() {
            input_ids[row_start + t] = tok;
            attention_mask[row_start + t] = 1;
        }
    }

    Ok(BatchEncodedText {
        batch_size,
        seq_len,
        input_ids,
        attention_mask,
    })
}
