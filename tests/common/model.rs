//! The pinned Qwen3-0.6B files the engine tests run on, downloaded once and verified.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

const GGUF: (&str, &str, &str) = (
    "Qwen3-0.6B-Q8_0.gguf",
    "https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/23749fefcc72300e3a2ad315e1317431b06b590a/Qwen3-0.6B-Q8_0.gguf",
    "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031",
);
const TOKENIZER: (&str, &str, &str) = (
    "tokenizer.json",
    "https://huggingface.co/Qwen/Qwen3-0.6B/resolve/c1899de289a04d12100db370d81485cdf75e47ca/tokenizer.json",
    "aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4",
);

fn sha256(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

fn fetch(dir: &Path, (name, url, want): (&str, &str, &str)) -> PathBuf {
    let path = dir.join(name);
    let fail = |why: &str| -> ! {
        panic!("{name}: {why}; expected sha256 {want}; set RS3_MODEL_DIR to a directory holding it")
    };
    if sha256(&path).as_deref() != Some(want) {
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| fail(&format!("cannot create {dir:?}: {e}")));
        let tmp = dir.join(format!("{name}.part"));
        let ok = Command::new("curl")
            .args(["-fL", "-o"])
            .arg(&tmp)
            .arg(url)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            fail("download failed");
        }
        std::fs::rename(&tmp, &path).unwrap_or_else(|e| fail(&format!("cannot rename: {e}")));
        if sha256(&path).as_deref() != Some(want) {
            fail("downloaded file has the wrong hash");
        }
    }
    path
}

/// The (gguf, tokenizer) paths, in RS3_MODEL_DIR or ~/.cache/rs3/models.
pub fn model_files() -> (PathBuf, PathBuf) {
    let dir = std::env::var_os("RS3_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".cache/rs3/models")
        });
    (fetch(&dir, GGUF), fetch(&dir, TOKENIZER))
}
