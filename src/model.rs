//! A model whose next token depends only on its stored generator state.

use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};
use candle_transformers::models::quantized_qwen3::ModelWeights;
use std::path::Path;
use std::sync::Arc;
use tokenizers::Tokenizer;

pub trait Model {
    fn start_state(&self, seed: u64, prompt: &str) -> Vec<u8>;
    fn step(&self, state: &[u8]) -> (u32, Vec<u8>);
    fn render(&self, tokens: &[u32]) -> String;
}

pub struct FakeModel;

impl Model for FakeModel {
    fn start_state(&self, seed: u64, prompt: &str) -> Vec<u8> {
        let mut h = blake3::Hasher::new();
        h.update(&seed.to_le_bytes());
        h.update(prompt.as_bytes());
        h.finalize().as_bytes().to_vec()
    }

    fn step(&self, state: &[u8]) -> (u32, Vec<u8>) {
        let mut out = [0u8; 36];
        blake3::Hasher::new()
            .update(state)
            .finalize_xof()
            .fill(&mut out);
        let token = u32::from_le_bytes([out[0], out[1], out[2], out[3]]);
        (token, out[4..].to_vec())
    }

    fn render(&self, tokens: &[u32]) -> String {
        tokens
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Which model a node runs goals on.
pub enum Engine {
    Fake,
    Candle(Arc<Candle>),
}

/// Qwen3 on candle's CPU path, loaded once and cloned cheaply per generation.
pub struct Candle {
    weights: ModelWeights,
    tokenizer: Tokenizer,
    eos: Vec<u32>,
    pool: rayon::ThreadPool,
    pub model_hash: String,
    pub tokenizer_hash: String,
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

impl Candle {
    pub fn load(gguf: &Path, tokenizer: &Path, threads: usize) -> Result<Self, String> {
        let model_bytes = read(gguf)?;
        let tokenizer_bytes = read(tokenizer)?;
        let tok = Tokenizer::from_bytes(&tokenizer_bytes)
            .map_err(|e| format!("{}: {e}", tokenizer.display()))?;
        let mut cursor = std::io::Cursor::new(&model_bytes);
        let content = gguf_file::Content::read(&mut cursor)
            .map_err(|e| format!("{}: {e}", gguf.display()))?;
        let weights = ModelWeights::from_gguf(content, &mut cursor, &Device::Cpu)
            .map_err(|e| format!("{}: {e}", gguf.display()))?;
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|e| format!("thread pool: {e}"))?;
        let eos = ["<|im_end|>", "<|endoftext|>"]
            .iter()
            .filter_map(|t| tok.token_to_id(t))
            .collect();
        Ok(Self {
            weights,
            tokenizer: tok,
            eos,
            pool,
            model_hash: hex(&model_bytes),
            tokenizer_hash: hex(&tokenizer_bytes),
        })
    }

    /// The chat-formatted prompt as token ids.
    pub fn prompt_tokens(&self, prompt: &str) -> Result<Vec<u32>, String> {
        let text = format!("<|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n");
        let encoding = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| format!("tokenize: {e}"))?;
        Ok(encoding.get_ids().to_vec())
    }

    pub fn is_eos(&self, token: u32) -> bool {
        self.eos.contains(&token)
    }

    /// The text of `tokens`. While generation runs, a half-decoded character is held back.
    pub fn render(&self, tokens: &[u32], ended: bool) -> String {
        let ids: Vec<u32> = tokens
            .iter()
            .copied()
            .filter(|t| !self.is_eos(*t))
            .collect();
        let text = self.tokenizer.decode(&ids, false).unwrap_or_default();
        if ended {
            text
        } else {
            text.trim_end_matches('\u{fffd}').to_string()
        }
    }
}

/// One goal's live decoding state: a kv cache holding the prompt and every committed token.
pub struct Generation<'a> {
    candle: &'a Candle,
    weights: ModelWeights,
    seed: u64,
    offset: usize,
    count: u64,
    logits: Vec<f32>,
}

impl<'a> Generation<'a> {
    /// Prefill the prompt, then replay `committed` one token at a time without sampling.
    pub fn new(
        candle: &'a Candle,
        seed: u64,
        prompt: &[u32],
        committed: &[u32],
    ) -> Result<Self, String> {
        let mut g = Self {
            candle,
            weights: candle.weights.clone(),
            seed,
            offset: 0,
            count: 0,
            logits: Vec::new(),
        };
        g.feed(prompt)?;
        for token in committed {
            g.feed(&[*token])?;
            g.count += 1;
        }
        Ok(g)
    }

    fn feed(&mut self, ids: &[u32]) -> Result<(), String> {
        let (weights, offset) = (&mut self.weights, self.offset);
        let run = || -> candle_core::Result<Vec<f32>> {
            let input = Tensor::from_slice(ids, (1, ids.len()), &Device::Cpu)?;
            let out = weights.forward(&input, offset)?;
            out.to_dtype(DType::F32)?.flatten_all()?.to_vec1::<f32>()
        };
        self.logits = self
            .candle
            .pool
            .install(run)
            .map_err(|e| format!("model: {e}"))?;
        self.offset += ids.len();
        Ok(())
    }

    /// Sample the next token, and feed it so the following call can continue.
    pub fn next_token(&mut self) -> Result<u32, String> {
        let token = sample(&self.logits, self.seed, self.count);
        self.feed(&[token])?;
        self.count += 1;
        Ok(token)
    }
}

const TEMPERATURE: f64 = 0.6;
const TOP_P: f64 = 0.95;

/// Temperature plus top-p sampling whose only randomness is a hash of (seed, position).
fn sample(logits: &[f32], seed: u64, position: u64) -> u32 {
    let top = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut probs: Vec<(u32, f64)> = (0u32..)
        .zip(logits)
        .map(|(id, l)| (id, ((f64::from(*l) - f64::from(top)) / TEMPERATURE).exp()))
        .collect();
    let total: f64 = probs.iter().map(|p| p.1).sum();
    probs.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    pick(&probs, total, draw(seed, position))
}

/// The token at `draw` within the top-p nucleus of `probs`, which are sorted by descending weight.
fn pick(probs: &[(u32, f64)], total: f64, draw: f64) -> u32 {
    let mut kept = 0.0;
    let nucleus: Vec<&(u32, f64)> = probs
        .iter()
        .take_while(|p| {
            let more = kept < TOP_P * total;
            kept += p.1;
            more
        })
        .collect();
    let mut target = draw * kept;
    for (id, p) in &nucleus {
        target -= p;
        if target < 0.0 {
            return *id;
        }
    }
    nucleus.last().map_or(0, |p| p.0)
}

/// A uniform value in [0, 1) from blake3 of the seed and position.
fn draw(seed: u64, position: u64) -> f64 {
    let hash = blake3::Hasher::new()
        .update(&seed.to_le_bytes())
        .update(&position.to_le_bytes())
        .finalize();
    let mut first = [0u8; 8];
    first.copy_from_slice(&hash.as_bytes()[..8]);
    (u64::from_le_bytes(first) >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(m: &FakeModel, mut state: Vec<u8>, n: usize) -> (Vec<u32>, Vec<u8>) {
        let mut out = Vec::new();
        for _ in 0..n {
            let (t, s) = m.step(&state);
            out.push(t);
            state = s;
        }
        (out, state)
    }

    fn tokens(seed: u64, prompt: &str, n: usize) -> Vec<u32> {
        run(&FakeModel, FakeModel.start_state(seed, prompt), n).0
    }

    #[test]
    fn same_seed_and_prompt_repeat() {
        assert_eq!(tokens(1, "hi", 20), tokens(1, "hi", 20));
    }

    #[test]
    fn different_seed_or_prompt_differs() {
        assert_ne!(tokens(1, "hi", 20), tokens(2, "hi", 20));
        assert_ne!(tokens(1, "hi", 20), tokens(1, "ho", 20));
    }

    #[test]
    fn chunked_generation_equals_one_run() {
        let whole = tokens(7, "p", 20);
        let mut state = FakeModel.start_state(7, "p");
        let mut joined = Vec::new();
        for n in [1, 3, 16] {
            let (t, s) = run(&FakeModel, state, n);
            joined.extend(t);
            state = s;
        }
        assert_eq!(joined, whole);
    }

    #[test]
    fn render_joins_with_spaces() {
        assert_eq!(FakeModel.render(&[]), "");
        assert_eq!(FakeModel.render(&[1, 22, 333]), "1 22 333");
    }

    #[test]
    fn sampling_is_a_pure_function_of_logits_seed_and_position() {
        let logits = [1.0f32, 1.0, 1.0, 1.0];
        assert_eq!(sample(&logits, 3, 9), sample(&logits, 3, 9));
        let picks: std::collections::HashSet<u32> =
            (0..200).map(|i| sample(&logits, 1, i)).collect();
        assert!(picks.len() > 1, "equal logits should spread over positions");
    }

    #[test]
    fn a_dominant_token_always_wins() {
        let mut logits = vec![0.0f32; 10];
        logits[7] = 30.0;
        assert!((0..50).all(|i| sample(&logits, 2, i) == 7));
    }

    #[test]
    fn top_p_excludes_the_unlikely_tail() {
        // Two tokens hold ~all mass; the tail (ids 2..) is far below the 0.95 cut.
        let mut logits = vec![-20.0f32; 50];
        logits[0] = 5.0;
        logits[1] = 4.9;
        assert!((0..300).all(|i| sample(&logits, 4, i) < 2));
        assert!((0..300).any(|i| sample(&logits, 4, i) == 1));
    }

    #[test]
    fn ties_resolve_by_lowest_id_first() {
        // With draws near 0 the first of the sorted order is picked: ids tie, so id 0.
        let logits = [2.0f32, 2.0, 2.0];
        let first = (0..2000)
            .filter(|i| draw(0, *i) < 0.01)
            .map(|i| sample(&logits, 0, i));
        let picked: Vec<u32> = first.collect();
        assert!(!picked.is_empty() && picked.iter().all(|t| *t == 0));
    }

    #[test]
    fn draws_are_uniform_in_the_unit_interval() {
        let d: Vec<f64> = (0..1000).map(|i| draw(5, i)).collect();
        assert!(d.iter().all(|x| (0.0..1.0).contains(x)));
        let mean = d.iter().sum::<f64>() / 1000.0;
        assert!((0.4..0.6).contains(&mean), "{mean}");
        assert_ne!(draw(1, 0), draw(2, 0));
        assert_ne!(draw(1, 0), draw(1, 1));
    }

    #[test]
    fn huge_logits_still_sample_by_relative_weight() {
        let mut logits = vec![0.0f32; 10];
        logits[3] = 5000.0;
        logits[4] = 4999.0;
        assert!((0..100).all(|i| matches!(sample(&logits, 6, i), 3 | 4)));
    }

    #[test]
    fn pick_boundaries_are_exact() {
        let probs = [(10, 0.5), (11, 0.25), (12, 0.25)];
        // The nucleus stops as soon as it holds 95% of the mass: here 0.5 + 0.25 + 0.25 is needed.
        assert_eq!(pick(&probs, 1.0, 0.99), 12);
        // A draw landing exactly on a boundary belongs to the next token.
        assert_eq!(pick(&probs, 1.0, 0.5), 11);
        assert_eq!(pick(&probs, 1.0, 0.0), 10);
        // Once 95% is covered the rest is excluded, even for a draw at the very top.
        let tail = [(1, 0.96), (2, 0.04)];
        assert_eq!(pick(&tail, 1.0, 0.999), 1);
        let exact = [(1, 0.5), (2, 0.45), (3, 0.05)];
        assert_ne!(pick(&exact, 1.0, 0.999), 3);
    }

    #[test]
    fn temperature_sharpens_the_distribution() {
        // At temperature 0.6 the top token holds over 95% of the mass, so nothing else is sampled.
        assert!((0..300).all(|i| sample(&[0.0, 2.0], 8, i) == 1));
    }
}
