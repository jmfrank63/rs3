use std::hash::{BuildHasher, Hasher};

// from https://www.reddit.com/r/rust/comments/j0fm4x/implementing_a_custom_hash_function/
pub struct ShiftXorHasher {
    state: u64,
}

impl ShiftXorHasher {
    pub fn new(seed: u64) -> Self {
        ShiftXorHasher { state: seed }
    }

    pub fn default() -> Self {
        ShiftXorHasher { state: 0 }
    }
}

impl Hasher for ShiftXorHasher {
    fn finish(&self) -> u64 {
        self.state
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.state = self.state.rotate_left(8) ^ u64::from(byte);
        }
    }
}

pub struct BuildShiftXorHasher;

impl BuildHasher for BuildShiftXorHasher {
    type Hasher = ShiftXorHasher;

    fn build_hasher(&self) -> Self::Hasher {
        ShiftXorHasher { state: 0 }
    }
}
