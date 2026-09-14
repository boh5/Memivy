//! Fixed speech model manifest in the user-wide Memivy model cache.
use crate::embedding::{
    ModelDescriptor, Result,
    cache::{download_file, model_cache_root, verify},
    lock, private_dir,
};
use std::{fs, path::PathBuf};
pub const MODELS: [ModelDescriptor; 2] = [
    ModelDescriptor {
        repo: "ggml-org/Qwen3-ASR-0.6B-GGUF",
        revision: "928ab958557df9aa2ef1c93e0e83c7ad0933fae2",
        file: "Qwen3-ASR-0.6B-Q8_0.gguf",
        bytes: 804749248,
        sha256: "bca259818b50ca7c4c05e9bdb35a5dc04fa039653a6d6f3f0f331f96f6aa1971",
    },
    ModelDescriptor {
        repo: "ggml-org/Qwen3-ASR-0.6B-GGUF",
        revision: "928ab958557df9aa2ef1c93e0e83c7ad0933fae2",
        file: "mmproj-Qwen3-ASR-0.6B-Q8_0.gguf",
        bytes: 214392480,
        sha256: "41a342b5e4c514e968cb756de6cd1b7be39eff43c44c57a2ef5fc6522e36603d",
    },
];
#[derive(Clone)]
pub struct SpeechCache {
    pub root: PathBuf,
}
impl SpeechCache {
    pub fn for_user() -> Result<Self> {
        Ok(Self {
            root: model_cache_root()?,
        })
    }
    fn repo(&self) -> PathBuf {
        self.root.join("models--ggml-org--Qwen3-ASR-0.6B-GGUF")
    }
    pub fn path(&self, m: &ModelDescriptor) -> PathBuf {
        self.repo().join("snapshots").join(m.revision).join(m.file)
    }
    fn blob(&self, m: &ModelDescriptor) -> PathBuf {
        self.repo().join("blobs").join(m.sha256)
    }
    pub fn ready(&self) -> bool {
        MODELS
            .iter()
            .all(|m| fs::metadata(self.path(m)).is_ok_and(|f| f.is_file() && f.len() == m.bytes))
    }
    pub fn downloaded(&self) -> u64 {
        MODELS
            .iter()
            .map(|m| {
                fs::metadata(self.path(m))
                    .or_else(|_| fs::metadata(self.blob(m)))
                    .or_else(|_| fs::metadata(self.blob(m).with_extension("incomplete")))
                    .map_or(0, |f| f.len().min(m.bytes))
            })
            .sum()
    }
    pub fn clear(&self) -> Result<()> {
        let locks = self
            .root
            .join(".locks/models--ggml-org--Qwen3-ASR-0.6B-GGUF");
        let _guards = MODELS
            .iter()
            .map(|m| lock(&locks, &format!("{}.lock", m.sha256)))
            .collect::<Result<Vec<_>>>()?;
        for m in &MODELS {
            for p in [
                self.path(m),
                self.blob(m),
                self.blob(m).with_extension("incomplete"),
            ] {
                if p.symlink_metadata().is_ok() {
                    fs::remove_file(p).map_err(|_| "Cannot remove the model file")?;
                }
            }
        }
        Ok(())
    }
    pub fn verify(&self) -> Result<()> {
        for m in &MODELS {
            verify(&self.path(m), m)?;
        }
        Ok(())
    }
    pub async fn download(&self, keep: impl Fn() -> bool) -> Result<()> {
        // Serialize downloads of each immutable blob across Memivy processes.
        for m in &MODELS {
            let locks = self
                .root
                .join(".locks/models--ggml-org--Qwen3-ASR-0.6B-GGUF");
            let _guard = lock(&locks, &format!("{}.lock", m.sha256))?;
            if verify(&self.path(m), m).is_ok() {
                continue;
            }
            private_dir(self.blob(m).parent().unwrap())?;
            if verify(&self.blob(m), m).is_err() {
                let partial = self.blob(m).with_extension("incomplete");
                download_file(
                    &format!(
                        "https://huggingface.co/{}/resolve/{}/{}",
                        m.repo, m.revision, m.file
                    ),
                    &partial,
                    m,
                    &keep,
                )
                .await?;
                fs::rename(partial, self.blob(m)).map_err(|_| "Cannot write the model cache")?;
            }
            let snapshot = self.path(m);
            private_dir(snapshot.parent().unwrap())?;
            let temp = snapshot.with_extension("link.tmp");
            if temp.symlink_metadata().is_ok() {
                fs::remove_file(&temp).map_err(|_| "Cannot write the model snapshot")?;
            }
            std::os::unix::fs::symlink(PathBuf::from("../../blobs").join(m.sha256), &temp)
                .map_err(|_| "Cannot write the model snapshot")?;
            fs::rename(temp, snapshot).map_err(|_| "Cannot write the model snapshot")?;
        }
        Ok(())
    }
}
pub fn transcript(raw: &str) -> Result<String> {
    let (_, text) = raw
        .split_once("<asr_text>")
        .ok_or("The speech model returned no valid transcript; try again")?;
    if text.contains("<|") || text.len() > 16000 {
        return Err("The speech model output is incomplete; try again".into());
    }
    Ok(text.trim().to_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_does_not_leak_or_invent_text() {
        assert_eq!(
            transcript("language Chinese<asr_text>今天试试 Rust。").unwrap(),
            "今天试试 Rust。"
        );
        assert_eq!(transcript("language None<asr_text>").unwrap(), "");
        assert!(transcript("language Chinese").is_err());
        assert!(transcript("<asr_text>hello<|im_end|>").is_err());
    }
    #[test]
    fn speech_and_embedding_share_the_product_cache() {
        assert_eq!(
            SpeechCache::for_user().unwrap().root,
            crate::embedding::cache::HfModelCache::for_user()
                .unwrap()
                .root
        );
    }
    #[test]
    fn status_never_creates_or_deletes_shared_cache() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("hub");
        let cache = SpeechCache { root: root.clone() };
        assert!(!cache.ready());
        assert_eq!(cache.downloaded(), 0);
        assert!(!root.exists());
        assert_eq!(MODELS.iter().map(|m| m.bytes).sum::<u64>(), 1019141728);
    }
}
