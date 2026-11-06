//! A model whose next token depends only on its stored generator state.

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
}
