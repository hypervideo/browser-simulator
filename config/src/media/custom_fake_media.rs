//! Converts audio and video files into inputs suitable for "fake" video and audio inputs for Chrome/Chromium.

use eyre::{
    bail,
    Context as _,
    Result,
};
use sha1::{
    Digest,
    Sha1,
};
use std::{
    path::{
        Path,
        PathBuf,
    },
    process::Command,
};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeMediaFileOrUrl {
    /// A file path to a video or audio file.
    File(PathBuf),
    /// A URL to a video or audio file.
    Url(Url),
}

impl std::str::FromStr for FakeMediaFileOrUrl {
    type Err = eyre::Report;

    fn from_str(input: &str) -> Result<Self> {
        if let Ok(url) = Url::parse(input) {
            return Ok(Self::Url(url));
        }
        let path = Path::new(input);
        if path.exists() {
            return Ok(Self::File(path.to_path_buf()));
        }
        Err(eyre::eyre!("Invalid input: {input}"))
    }
}

#[derive(Debug, Clone)]
pub struct FakeMediaFiles {
    pub audio: Option<PathBuf>,
    pub audio_error: Option<String>,
    pub video: Option<PathBuf>,
    pub video_error: Option<String>,
}

impl FakeMediaFiles {
    /// Reads a media file and tries to split its audio and video streams into a wav and y4m file, suitable for serving
    /// as "fake" media inputs for Chrome/Chromium.
    pub fn from_file_or_url(input: FakeMediaFileOrUrl, cache_dir: impl AsRef<Path>) -> Result<Self> {
        let cache_dir = cache_dir.as_ref();

        let input = match input {
            FakeMediaFileOrUrl::File(path) => path,
            FakeMediaFileOrUrl::Url(url) => {
                let name =
                    infer_filename_from_url(&url).unwrap_or_else(|| PathBuf::from("input.mp4" /* wild guess */));
                let url_hash = string_hash(&url)?;
                let cache_dir = cache_dir.join("download-cache").join(&url_hash);
                let input = cache_dir.join(&name);
                if !input.exists() {
                    std::fs::create_dir_all(&cache_dir)?;
                    download_file(&url, &input)?;
                }
                input
            }
        };

        Self::from_file(&input, cache_dir)
    }

    pub fn from_file(input: impl AsRef<Path>, cache_dir: impl AsRef<Path>) -> Result<Self> {
        let cache_dir = cache_dir.as_ref().join("media-cache");
        let input = input.as_ref();
        let hash = file_hash(input)?;

        let (video, video_error) = match ffmpeg_extract(Kind::Video, input, &hash, &cache_dir) {
            Ok(video) => (Some(video), None),
            Err(err) => {
                warn!("Video conversion failed: {err}");
                (None, Some(err.to_string()))
            }
        };

        let (audio, audio_error) = match ffmpeg_extract(Kind::Audio, input, &hash, &cache_dir) {
            Ok(audio) => (Some(audio), None),
            Err(err) => {
                warn!("Audio conversion failed: {err}");
                (None, Some(err.to_string()))
            }
        };

        Ok(Self {
            audio,
            audio_error,
            video,
            video_error,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Video,
    Audio,
}

fn ffmpeg_extract(kind: Kind, input: &Path, input_hash: &str, cache_dir: &Path) -> Result<PathBuf> {
    const AUDIO_FILE: &str = "audio.wav";
    const AUDIO_EXT: &str = "wav";
    const VIDEO_FILE: &str = "video.y4m";
    const VIDEO_EXT: &str = "y4m";

    // Check if the input file is already in the correct format
    let filename = match (input.extension().and_then(|ext| ext.to_str()), kind) {
        (Some(AUDIO_EXT), Kind::Audio) => return Ok(input.to_path_buf()),
        (Some(VIDEO_EXT), Kind::Video) => return Ok(input.to_path_buf()),
        (_, Kind::Audio) => AUDIO_FILE,
        (_, Kind::Video) => VIDEO_FILE,
    };

    // Did we already convert this file?
    let cache_dir = cache_dir.join(input_hash);
    let cached = cache_dir.join(filename);
    if cached.exists() {
        return Ok(cached);
    }

    std::fs::create_dir_all(&cache_dir)?;

    let args = match kind {
        Kind::Audio => vec![
            "-loglevel",
            "panic",
            "-i",
            input.to_str().expect("invalid input path"),
            "-y",
            "-vn",
            cached.to_str().expect("invalid output path"),
        ],
        Kind::Video => vec![
            "-loglevel",
            "error",
            "-i",
            input.to_str().expect("invalid input path"),
            "-pix_fmt",
            "yuv420p",
            "-an",
            "-y",
            cached.to_str().expect("invalid output path"),
        ],
    };

    let output = Command::new("ffmpeg")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to start ffmpeg process")?
        .wait_with_output()
        .context("Failed to wait for ffmpeg process")?;

    if !cached.exists() {
        // If the conversion failed, remove the potentially empty cache directory.
        if cache_dir.read_dir().is_ok_and(|dir| dir.count() == 0) {
            let _ = std::fs::remove_dir_all(&cache_dir);
        }
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to create {filename} file: {err}");
    }

    debug!(?cached, "created");

    Ok(cached)
}

fn file_hash(path: &Path) -> Result<String> {
    let content = std::fs::read(path)?;
    let mut hasher = Sha1::new();
    hasher.update(content);
    let bytes = hasher.finalize();
    Ok(bytes.iter().fold(String::new(), |mut acc, b| {
        acc.push_str(&format!("{:02x}", b));
        acc
    }))
}

fn string_hash(string: impl AsRef<str>) -> Result<String> {
    let mut hasher = Sha1::new();
    hasher.update(string.as_ref().bytes().collect::<Vec<_>>());
    let bytes = hasher.finalize();
    Ok(bytes.iter().fold(String::new(), |mut acc, b| {
        acc.push_str(&format!("{:02x}", b));
        acc
    }))
}

fn download_file(url: &Url, output: &Path) -> Result<()> {
    let response = reqwest::blocking::get(url.as_str())?;
    if !response.status().is_success() {
        bail!("Failed to download file from {url}, status code: {}", response.status());
    }
    let mut file = std::fs::File::create(output)?;
    std::io::copy(&mut response.bytes()?.as_ref(), &mut file)?;
    Ok(())
}

fn infer_filename_from_url(url: &Url) -> Option<PathBuf> {
    let path = url.path();
    let filename = path
        .split('/')
        .next_back()
        .ok_or_else(|| eyre::eyre!("Failed to infer filename from URL"))
        .ok()?;

    if filename.is_empty() {
        return None;
    }

    let filename = if filename.contains('?') {
        let filename = filename.split('?').next().unwrap();
        filename
    } else {
        filename
    };

    let path = PathBuf::from(filename);
    path.extension().and_then(|ext| ext.to_str())?;

    Some(path)
}
