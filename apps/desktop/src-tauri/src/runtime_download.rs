use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures_util::StreamExt;
use reqwest::{
    header::{CONTENT_RANGE, RANGE},
    Client, StatusCode,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("runtime pack download was cancelled")]
    Cancelled,
    #[error("runtime pack download failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("runtime pack download I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime pack size mismatch: expected {expected}, received {actual}")]
    Size { expected: u64, actual: u64 },
    #[error("runtime pack SHA-256 mismatch")]
    Sha256,
}

pub async fn download_verified_archive<F>(
    client: &Client,
    url: &str,
    target: &Path,
    expected_size: u64,
    expected_sha256: &str,
    cancel: Arc<AtomicBool>,
    mut progress: F,
) -> Result<(), DownloadError>
where
    F: FnMut(u64, u64),
{
    if cancel.load(Ordering::Acquire) {
        let _ = fs::remove_file(target).await;
        return Err(DownloadError::Cancelled);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await?;
    }

    let mut existing = match fs::metadata(target).await {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.into()),
    };
    if existing > expected_size {
        fs::remove_file(target).await?;
        existing = 0;
    }
    if existing == expected_size && existing > 0 {
        if hash_file(target).await? == expected_sha256.to_ascii_lowercase() {
            progress(existing, expected_size);
            return Ok(());
        }
        fs::remove_file(target).await?;
        existing = 0;
    }

    let mut request = client.get(url);
    if existing > 0 {
        request = request.header(RANGE, format!("bytes={existing}-"));
    }
    let response = request.send().await?.error_for_status()?;
    let range_start = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(content_range_start);
    let append = existing > 0
        && response.status() == StatusCode::PARTIAL_CONTENT
        && range_start == Some(existing);
    if !append {
        existing = 0;
    }
    let advertised = response.content_length().unwrap_or(0);
    if advertised > expected_size.saturating_sub(existing) {
        let _ = fs::remove_file(target).await;
        return Err(DownloadError::Size {
            expected: expected_size,
            actual: existing.saturating_add(advertised),
        });
    }

    let mut options = fs::OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let mut output = options.open(target).await?;
    let mut downloaded = existing;
    progress(downloaded, expected_size);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Acquire) {
            drop(output);
            let _ = fs::remove_file(target).await;
            return Err(DownloadError::Cancelled);
        }
        let chunk = chunk?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > expected_size {
            drop(output);
            let _ = fs::remove_file(target).await;
            return Err(DownloadError::Size {
                expected: expected_size,
                actual: downloaded,
            });
        }
        output.write_all(&chunk).await?;
        progress(downloaded, expected_size);
    }
    output.flush().await?;
    drop(output);

    if downloaded != expected_size {
        return Err(DownloadError::Size {
            expected: expected_size,
            actual: downloaded,
        });
    }
    let actual_hash = hash_file(target).await?;
    if actual_hash != expected_sha256.to_ascii_lowercase() {
        let _ = fs::remove_file(target).await;
        return Err(DownloadError::Sha256);
    }
    Ok(())
}

fn content_range_start(value: &str) -> Option<u64> {
    value
        .strip_prefix("bytes ")?
        .split_once('-')?
        .0
        .parse()
        .ok()
}

async fn hash_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Write},
        net::TcpListener,
        sync::{atomic::AtomicBool, Arc},
        thread,
    };

    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    use super::{download_verified_archive, DownloadError};

    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn serve(body: Vec<u8>, honor_range: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).expect("read request");
            let request = String::from_utf8_lossy(&request[..count]);
            let range = request
                .lines()
                .find_map(|line| {
                    line.strip_prefix("range: bytes=")
                        .or_else(|| line.strip_prefix("Range: bytes="))
                })
                .and_then(|value| value.trim_end_matches('-').parse::<usize>().ok());
            let start = if honor_range { range.unwrap_or(0) } else { 0 };
            let status = if honor_range && range.is_some() {
                "206 Partial Content"
            } else {
                "200 OK"
            };
            let payload = &body[start..];
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                payload.len(),
                if status.starts_with("206") {
                    format!(
                        "Content-Range: bytes {start}-{}/{}\r\n",
                        body.len() - 1,
                        body.len()
                    )
                } else {
                    String::new()
                }
            )
            .unwrap();
            stream.write_all(payload).unwrap();
        });
        format!("http://{address}/pack.zip")
    }

    fn serve_wrong_range(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read request");
            write!(stream, "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes 0-{}/{}\r\nConnection: close\r\n\r\n", body.len(), body.len() - 1, body.len()).unwrap();
            stream.write_all(&body).unwrap();
        });
        format!("http://{address}/pack.zip")
    }

    #[tokio::test]
    async fn downloads_and_verifies_complete_archive() {
        let root = TempDir::new().unwrap();
        let target = root.path().join("pack.part");
        let bytes = b"complete runtime archive".to_vec();
        let url = serve(bytes.clone(), true);
        let mut progress = Vec::new();

        download_verified_archive(
            &reqwest::Client::new(),
            &url,
            &target,
            bytes.len() as u64,
            &hash(&bytes),
            Arc::new(AtomicBool::new(false)),
            |downloaded, total| progress.push((downloaded, total)),
        )
        .await
        .expect("download archive");

        assert_eq!(fs::read(target).unwrap(), bytes);
        assert_eq!(progress.last(), Some(&(24, 24)));
    }

    #[tokio::test]
    async fn resumes_partial_download_and_restarts_when_range_is_ignored() {
        for honor_range in [true, false] {
            let root = TempDir::new().unwrap();
            let target = root.path().join("pack.part");
            let bytes = b"0123456789abcdef".to_vec();
            fs::write(&target, &bytes[..6]).unwrap();
            let url = serve(bytes.clone(), honor_range);

            download_verified_archive(
                &reqwest::Client::new(),
                &url,
                &target,
                bytes.len() as u64,
                &hash(&bytes),
                Arc::new(AtomicBool::new(false)),
                |_, _| {},
            )
            .await
            .expect("resume archive");

            assert_eq!(fs::read(target).unwrap(), bytes);
        }
    }

    #[tokio::test]
    async fn reuses_a_complete_verified_partial_without_requesting_an_invalid_range() {
        let root = TempDir::new().unwrap();
        let target = root.path().join("complete.part");
        let bytes = b"already complete".to_vec();
        fs::write(&target, &bytes).unwrap();
        let mut progress = Vec::new();

        download_verified_archive(
            &reqwest::Client::new(),
            "http://127.0.0.1:1/must-not-be-requested",
            &target,
            bytes.len() as u64,
            &hash(&bytes),
            Arc::new(AtomicBool::new(false)),
            |downloaded, total| progress.push((downloaded, total)),
        )
        .await
        .expect("reuse complete verified archive");

        assert_eq!(progress, vec![(bytes.len() as u64, bytes.len() as u64)]);
    }

    #[tokio::test]
    async fn restarts_when_partial_content_begins_at_the_wrong_offset() {
        let root = TempDir::new().unwrap();
        let target = root.path().join("pack.part");
        let bytes = b"0123456789abcdef".to_vec();
        fs::write(&target, &bytes[..6]).unwrap();
        let url = serve_wrong_range(bytes.clone());

        download_verified_archive(
            &reqwest::Client::new(),
            &url,
            &target,
            bytes.len() as u64,
            &hash(&bytes),
            Arc::new(AtomicBool::new(false)),
            |_, _| {},
        )
        .await
        .expect("restart mismatched range from zero");

        assert_eq!(fs::read(target).unwrap(), bytes);
    }

    #[tokio::test]
    async fn rejects_overflow_checksum_mismatch_and_cancellation() {
        let root = TempDir::new().unwrap();
        let target = root.path().join("overflow.part");
        let bytes = b"too many bytes".to_vec();
        let url = serve(bytes.clone(), true);
        assert!(download_verified_archive(
            &reqwest::Client::new(),
            &url,
            &target,
            3,
            &hash(&bytes),
            Arc::new(AtomicBool::new(false)),
            |_, _| {},
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("size"));

        let target = root.path().join("hash.part");
        let url = serve(bytes.clone(), true);
        assert!(download_verified_archive(
            &reqwest::Client::new(),
            &url,
            &target,
            bytes.len() as u64,
            &"0".repeat(64),
            Arc::new(AtomicBool::new(false)),
            |_, _| {},
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("SHA-256"));

        let cancel = Arc::new(AtomicBool::new(true));
        let target = root.path().join("cancel.part");
        let error = download_verified_archive(
            &reqwest::Client::new(),
            "http://127.0.0.1:1/not-used",
            &target,
            1,
            &"0".repeat(64),
            cancel,
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(error, DownloadError::Cancelled));
        assert!(!target.exists());
    }
}
