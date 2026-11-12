pub mod bench;
pub mod error;
pub mod parity;
pub mod preprocess;
pub mod provider;
pub mod tokenizer;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use image::{DynamicImage, Rgb, RgbImage};

    use crate::preprocess::{
        EmbeddingBudget, PATCH_FLOATS, ResizerBackend, aspect_ratio_preserving_size,
        preprocess_dynamic_image,
    };
    use crate::provider::{cosine_similarity, truncate_and_l2_normalize};
    use crate::tokenizer::TextTokenizer;

    #[test]
    fn test_truncate_then_l2_normalize_order() {
        // Truncate [3, 4, 100] to 2 dims FIRST -> [3, 4] -> normalized [0.6, 0.8].
        // Normalizing at 3 dims first would give ~[0.03, 0.04], proving truncation order matters.
        let v = vec![3.0f32, 4.0, 100.0];
        let normed = truncate_and_l2_normalize(&v, 2).expect("normalization succeeds");
        assert_eq!(normed.len(), 2);
        assert!((normed[0] - 0.6).abs() < 1e-6);
        assert!((normed[1] - 0.8).abs() < 1e-6);
        let l2 = (normed[0] * normed[0] + normed[1] * normed[1]).sqrt();
        assert!((l2 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_identities() {
        let a = vec![0.25f32, -0.5, 0.75, 0.1];
        let b = vec![0.25f32, -0.5, 0.75, 0.1];
        let c = vec![0.5f32, 0.25, 0.0, 0.0];
        let sim_self = cosine_similarity(&a, &b).expect("self cosine succeeds");
        assert!((sim_self - 1.0).abs() < 1e-6);
        let sim_ortho = cosine_similarity(&a, &c).expect("ortho cosine succeeds");
        assert!(sim_ortho.abs() < 1e-6);
    }

    #[test]
    fn test_preprocess_synthetic_exact_grid_and_resize() {
        // 672x432 matches target grid for 3:2 at budget 140 (42*27 = 1134 valid patches <= 1260),
        // skipping resize while exercising both patch layout and zero/(-1) padding rows.
        let mut img = RgbImage::new(672, 432);
        // Patch (px=1, py=2) starts at x=16, y=32; mark pixel (dx=3, dy=5) at (19, 37)
        img.put_pixel(19, 37, Rgb([51, 102, 204]));
        let dyn_img = DynamicImage::ImageRgb8(img);
        let prep = preprocess_dynamic_image(
            &dyn_img,
            EmbeddingBudget::B140,
            ResizerBackend::FastCatmullRom,
            None,
        )
        .expect("preprocess exact size succeeds");

        assert_eq!(prep.target_width, 672);
        assert_eq!(prep.target_height, 432);
        assert_eq!(prep.valid_patches, 1134);
        assert_eq!(prep.max_patches, 1260);
        assert_eq!(prep.num_soft_tokens, 126);

        // Verify patch (px=1, py=2) -> flat patch index 2 * (672/16) + 1 = 85
        let patch_idx = 2 * 42 + 1;
        assert_eq!(prep.pixel_position_ids[patch_idx * 2], 1);
        assert_eq!(prep.pixel_position_ids[patch_idx * 2 + 1], 2);
        // Padding row after 1134 patches must be [-1, -1]
        assert_eq!(prep.pixel_position_ids[1134 * 2], -1);
        assert_eq!(prep.pixel_position_ids[1134 * 2 + 1], -1);

        // Pixel (dx=3, dy=5) -> inner offset ((5 * 16) + 3) * 3 = 249
        let base = patch_idx * PATCH_FLOATS + ((5 * 16) + 3) * 3;
        let inv_255 = 1.0f32 / 255.0f32;
        assert_eq!(prep.pixel_values[base], 51.0 * inv_255);
        assert_eq!(prep.pixel_values[base + 1], 102.0 * inv_255);
        assert_eq!(prep.pixel_values[base + 2], 204.0 * inv_255);

        // Upscaling check for small 144x96 input -> 672x432 at budget 140
        let (sh, sw) =
            aspect_ratio_preserving_size(96, 144, EmbeddingBudget::B140).expect("144x96 ok");
        assert_eq!((sh, sw), (432, 672));

        // Extreme aspect ratio clamping check
        let (ah, aw) =
            aspect_ratio_preserving_size(20, 2000, EmbeddingBudget::B140).expect("ultrawide ok");
        assert_eq!(ah, 48);
        assert!(aw <= 140 * 48);
    }

    #[test]
    fn test_tokenizer_fixed_strings_against_reference_ids() {
        let tok_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../models/onnx/tokenizer.json");
        assert!(
            tok_path.exists(),
            "tokenizer.json missing at {:?}; run `python spikes/embeddings/fetch_models.py` first",
            tok_path
        );
        let tok = TextTokenizer::from_file(&tok_path).expect("load tokenizer.json");

        let cases: &[(&str, &[i64])] = &[
            (
                "task: search result | query: red circle on blue background",
                &[
                    2, 8071, 236787, 3927, 1354, 1109, 7609, 236787, 2604, 10059, 580, 3730, 1695,
                    1,
                ],
            ),
            (
                "task: search result | query: ",
                &[2, 8071, 236787, 3927, 1354, 1109, 7609, 236787, 236743, 1],
            ),
            (
                "title: none | text: हिमालय की बर्फीली चोटियाँ और सूर्योदय",
                &[
                    2, 3250, 236787, 7293, 1109, 1816, 236787, 137573, 1165, 56327, 9973, 2150,
                    82406, 37260, 1402, 38393, 126513, 1,
                ],
            ),
            (
                "task: search result | query: 東京の夜景と桜の花びら",
                &[
                    2, 8071, 236787, 3927, 1354, 1109, 7609, 236787, 86459, 236945, 238978, 238435,
                    237032, 241744, 177595, 238261, 237173, 1,
                ],
            ),
        ];

        for &(text, expected_ids) in cases {
            let enc = tok
                .encode_formatted(text)
                .expect("encode fixed test string");
            assert_eq!(enc.input_ids, expected_ids, "mismatch for {text:?}");
            assert_eq!(enc.attention_mask, vec![1i64; expected_ids.len()]);
        }
    }
}
