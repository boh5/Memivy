//! User-wide Memivy model cache, independent of library and HF environment paths.
use super::*;
use std::io::{Read, Seek, SeekFrom};
// A stable product namespace keeps dev, installed apps and libraries on one cache.
pub fn model_cache_root() -> Result<PathBuf> {
    cache_root_at(&dirs::cache_dir().ok_or("无法确定用户模型缓存目录")?)
}
fn cache_root_at(base: &Path) -> Result<PathBuf> {
    if !base.is_absolute() {
        return Err("用户模型缓存目录必须为绝对路径".into());
    }
    Ok(base.join("com.memivy.app/models"))
}
#[derive(Clone)]
pub struct HfModelCache {
    pub root: PathBuf,
}
impl HfModelCache {
    pub fn for_user() -> Result<Self> {
        Ok(Self {
            root: model_cache_root()?,
        })
    }
    fn repo(&self) -> PathBuf {
        self.root
            .join(format!("models--{}", MODEL.repo.replace('/', "--")))
    }
    pub fn blob(&self) -> PathBuf {
        self.repo().join("blobs").join(MODEL.sha256)
    }
    pub fn incomplete(&self) -> PathBuf {
        self.blob().with_extension("incomplete")
    }
    pub fn model_path(&self) -> PathBuf {
        self.repo()
            .join("snapshots")
            .join(MODEL.revision)
            .join(MODEL.file)
    }
    pub fn downloaded(&self) -> u64 {
        fs::metadata(self.blob())
            .or_else(|_| fs::metadata(self.incomplete()))
            .map_or(0, |m| m.len().min(MODEL.bytes))
    }
    pub fn published(&self) -> bool {
        fs::metadata(self.model_path()).is_ok_and(|m| m.is_file() && m.len() == MODEL.bytes)
    }
    pub fn verify(&self) -> Result<PathBuf> {
        verify(&self.blob(), &MODEL)?;
        let p = fs::canonicalize(self.model_path()).map_err(io)?;
        if p != fs::canonicalize(self.blob()).map_err(io)? {
            return Err("模型快照不匹配".into());
        }
        Ok(p)
    }
    pub fn clear(&self) -> Result<()> {
        let _lock = lock(&self.root, "download.lock")?;
        // Only this fixed manifest is owned here; other future snapshots survive.
        let snapshot = self.model_path();
        if snapshot.symlink_metadata().is_ok() {
            fs::remove_file(snapshot).map_err(io)?;
        }
        for p in [self.blob(), self.incomplete()] {
            if p.exists() {
                fs::remove_file(p).map_err(io)?;
            }
        }
        Ok(())
    }
    pub async fn download(&self, keep_going: impl Fn() -> bool) -> Result<()> {
        let _lock = lock(&self.root, "download.lock")?;
        private_dir(self.blob().parent().unwrap())?;
        if self.published() && verify(&self.blob(), &MODEL).is_ok() {
            return Ok(());
        }
        if verify(&self.blob(), &MODEL).is_err() {
            let url = format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                MODEL.repo, MODEL.revision, MODEL.file
            );
            download_file(&url, &self.incomplete(), &MODEL, keep_going).await?;
            fs::rename(self.incomplete(), self.blob()).map_err(io)?;
        }
        let snapshot = self.model_path();
        private_dir(snapshot.parent().unwrap())?;
        let temp = snapshot.with_extension("link.tmp");
        if temp.symlink_metadata().is_ok() {
            fs::remove_file(&temp).map_err(io)?;
        }
        std::os::unix::fs::symlink(Path::new("../../blobs").join(MODEL.sha256), &temp)
            .map_err(io)?;
        fs::rename(temp, snapshot).map_err(io)?;
        File::open(self.blob().parent().unwrap())
            .and_then(|f| f.sync_all())
            .map_err(io)
    }
}
pub fn verify(p: &Path, m: &ModelDescriptor) -> Result<()> {
    let mut f = File::open(p).map_err(io)?;
    if f.metadata().map_err(io)?.len() != m.bytes {
        return Err("模型文件长度不符，请继续下载".into());
    }
    let mut sha = Sha256::new();
    let mut bytes = [0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut bytes).map_err(io)?;
        if n == 0 {
            break;
        }
        sha.update(&bytes[..n]);
    }
    if format!("{:x}", sha.finalize()) != m.sha256 {
        return Err("模型校验失败，请重新下载".into());
    }
    Ok(())
}
pub async fn download_file(
    url: &str,
    p: &Path,
    m: &ModelDescriptor,
    keep_going: impl Fn() -> bool,
) -> Result<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(p)
        .map_err(io)?;
    let mut at = file.metadata().map_err(io)?.len();
    if at > m.bytes {
        file.set_len(0).map_err(io)?;
        at = 0;
    }
    if at == m.bytes {
        if verify(p, m).is_ok() {
            return Ok(());
        }
        file.set_len(0).map_err(io)?;
        at = 0;
    }
    if !keep_going() {
        return Err("下载已暂停".into());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .read_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(io)?;
    let mut response = client
        .get(url)
        .header(reqwest::header::RANGE, format!("bytes={at}-"))
        .send()
        .await
        .map_err(|_| "模型下载连接失败，可继续重试".to_string())?;
    match response.status().as_u16() {
        206 => {
            let expected = format!("bytes {at}-{} /{}", m.bytes - 1, m.bytes).replace(" /", "/");
            if response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                != Some(expected.as_str())
            {
                return Err("下载断点响应不匹配，已保留断点".into());
            }
        }
        200 => {
            file.set_len(0).map_err(io)?;
            at = 0;
        }
        416 => return verify(p, m),
        _ => return Err("模型服务器暂不可用，可继续重试".into()),
    }
    file.seek(SeekFrom::Start(at)).map_err(io)?;
    while let Some(bytes) = response
        .chunk()
        .await
        .map_err(|_| "模型下载中断，可继续重试".to_string())?
    {
        if !keep_going() {
            file.sync_all().map_err(io)?;
            return Err("下载已暂停".into());
        }
        if at + bytes.len() as u64 > m.bytes {
            return Err("模型下载长度超出清单".into());
        }
        file.write_all(&bytes).map_err(io)?;
        at += bytes.len() as u64;
    }
    file.sync_all().map_err(io)?;
    let result = verify(p, m);
    if result.is_err() && at == m.bytes {
        file.set_len(0).map_err(io)?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_cache_is_user_wide_and_requires_an_absolute_home() {
        let home = tempfile::tempdir().unwrap();
        let cache = HfModelCache {
            root: cache_root_at(home.path()).unwrap(),
        };
        assert_eq!(cache.root, home.path().join("com.memivy.app/models"));
        assert!(!cache.root.exists()); // Resolving status must not create directories.
        assert!(cache_root_at(Path::new("relative")).is_err());
    }

    #[test]
    fn clearing_the_fixed_model_preserves_other_cached_models() {
        let home = tempfile::tempdir().unwrap();
        let cache = HfModelCache {
            root: cache_root_at(home.path()).unwrap(),
        };
        fs::create_dir_all(cache.blob().parent().unwrap()).unwrap();
        fs::create_dir_all(cache.model_path().parent().unwrap()).unwrap();
        fs::write(cache.blob(), b"synthetic fixed model").unwrap();
        fs::write(cache.incomplete(), b"partial").unwrap();
        std::os::unix::fs::symlink(
            Path::new("../../blobs").join(MODEL.sha256),
            cache.model_path(),
        )
        .unwrap();
        let other = cache.root.join("other-model");
        fs::write(&other, b"keep").unwrap();
        cache.clear().unwrap();
        assert!(!cache.blob().exists());
        assert!(!cache.incomplete().exists());
        assert!(cache.model_path().symlink_metadata().is_err());
        assert_eq!(fs::read(other).unwrap(), b"keep");
    }

    const SMALL: ModelDescriptor = ModelDescriptor {
        repo: "test/repo",
        revision: "fixed",
        file: "model",
        bytes: 5,
        sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    };
    fn server(response: &str) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response = response.to_owned();
        let t = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = vec![];
            let mut b = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                s.read_exact(&mut b).unwrap();
                request.push(b[0]);
            }
            s.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (format!("http://{address}/model"), t)
    }
    #[tokio::test]
    async fn valid_range_resumes_and_200_restarts_safely() {
        for (response, expected_range) in [
            (
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-4/5\r\nContent-Length: 3\r\nConnection: close\r\n\r\nllo",
                "bytes=2-",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                "bytes=2-",
            ),
        ] {
            let d = tempfile::tempdir().unwrap();
            let p = d.path().join("partial");
            fs::write(&p, "he").unwrap();
            let (url, t) = server(response);
            download_file(&url, &p, &SMALL, || true).await.unwrap();
            assert_eq!(fs::read(&p).unwrap(), b"hello");
            assert!(t.join().unwrap().contains(expected_range));
        }
    }
    #[tokio::test]
    async fn corrupted_complete_partial_is_redownloaded_and_wrong_range_keeps_checkpoint() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("partial");
        fs::write(&p, "xxxxx").unwrap();
        let (url, t) =
            server("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
        download_file(&url, &p, &SMALL, || true).await.unwrap();
        assert!(t.join().unwrap().contains("bytes=0-"));
        fs::write(&p, "he").unwrap();
        let (url, t) = server(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 1-4/5\r\nContent-Length: 4\r\nConnection: close\r\n\r\nello",
        );
        assert!(download_file(&url, &p, &SMALL, || true).await.is_err());
        assert_eq!(fs::read(&p).unwrap(), b"he");
        t.join().unwrap();
    }
    #[tokio::test]
    async fn cancel_and_bad_hash_never_publish_success() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("partial");
        assert!(
            download_file("http://127.0.0.1:1", &p, &SMALL, || false)
                .await
                .is_err()
        );
        let (url, t) =
            server("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nxxxxx");
        assert!(download_file(&url, &p, &SMALL, || true).await.is_err());
        assert_eq!(fs::metadata(&p).unwrap().len(), 0);
        t.join().unwrap();
    }
    #[tokio::test]
    async fn range_416_does_not_accept_an_incomplete_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("partial");
        fs::write(&p, "he").unwrap();
        let (url, t) = server(
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        assert!(download_file(&url, &p, &SMALL, || true).await.is_err());
        assert_eq!(fs::read(&p).unwrap(), b"he");
        t.join().unwrap();
    }
}
