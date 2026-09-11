use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use mp4ameta::{Data, FreeformIdent, Img, ImgFmt, MediaType, Tag};
use serde::{Deserialize, Serialize};

use crate::{
    language,
    metadata::{Artwork, MetadataResult, Provider},
};

const METADATA_MEAN: &str = "io.github.nahime0.ReelMux.metadata";
const LEGACY_METADATA_MEAN: &str = "io.github.sublerlinux.metadata";
const PRIVATE_METADATA_FIELDS: [(&str, &str); 9] = [
    ("cast", "cast"),
    ("director", "director"),
    ("producers", "producers"),
    ("screenwriters", "screenwriters"),
    ("studio", "studio"),
    ("content_rating", "content-rating"),
    ("provider", "provider"),
    ("provider_id", "provider-id"),
    ("webpage_url", "source-url"),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioMode {
    #[default]
    Copy,
    Aac,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub source: PathBuf,
    pub index: u32,
    pub kind: String,
    pub codec: String,
    pub details: String,
    pub language: String,
    pub title: String,
    pub enabled: bool,
    pub unsupported: Option<String>,
    pub default: bool,
    pub forced: bool,
    pub audio_mode: AudioMode,
    pub dispositions: Vec<String>,
    #[serde(default)]
    pub attached_picture: bool,
}

impl Track {
    pub fn operation(&self) -> &str {
        if self.unsupported.is_some() {
            "Unsupported"
        } else if self.kind == "subtitle" {
            "MP4 text"
        } else if self.kind == "audio" && self.audio_mode == AudioMode::Aac {
            "Convert to AAC"
        } else {
            "Copy"
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chapter {
    pub start_time: String,
    pub end_time: String,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub path: PathBuf,
    pub duration: f64,
    pub size: u64,
    pub tracks: Vec<Track>,
    pub metadata: BTreeMap<String, String>,
    pub chapters: Vec<Chapter>,
    #[serde(skip)]
    pub artwork: Option<Artwork>,
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: ProbeFormat,
    #[serde(default)]
    chapters: Vec<Chapter>,
}

#[derive(Default, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct ProbeStream {
    index: u32,
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    codec_name: String,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u32>,
    sample_rate: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    #[serde(default)]
    disposition: BTreeMap<String, i32>,
}

fn tag(tags: &BTreeMap<String, String>, key: &str) -> String {
    tags.iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

fn metadata_key(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "content-rating" => "content_rating".into(),
        "provider-id" => "provider_id".into(),
        "source-url" => "webpage_url".into(),
        "attribution" => "metadata_attribution".into(),
        key => key.into(),
    }
}

fn local_file(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .with_context(|| format!("File is not accessible: {}", path.display()))?;
    ensure!(path.is_file(), "Select a regular file: {}", path.display());
    Ok(path)
}

fn probe(path: &Path) -> Result<Probe> {
    let mut child = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_streams",
            "-show_format",
            "-show_chapters",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Unable to start FFprobe. Install FFmpeg and make sure it is available in PATH")?;
    let mut stdout = child
        .stdout
        .take()
        .context("FFprobe: stdout is unavailable")?;
    let mut stderr = child
        .stderr
        .take()
        .context("FFprobe: stderr is unavailable")?;
    let output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let errors = thread::spawn(move || {
        let mut bytes = String::new();
        stderr.read_to_string(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = output.join();
            let _ = errors.join();
            bail!("Analysis stopped because FFprobe did not respond within 30 seconds");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = output
        .join()
        .map_err(|_| anyhow::anyhow!("FFprobe output reader stopped unexpectedly"))??;
    let errors = errors
        .join()
        .map_err(|_| anyhow::anyhow!("FFprobe error reader stopped unexpectedly"))??;
    ensure!(
        status.success(),
        "The file could not be read as media: {}",
        errors.trim()
    );
    serde_json::from_slice(&output).context("Invalid FFprobe response")
}

fn read_itunes_artwork(path: &Path, metadata: &BTreeMap<String, String>) -> Option<Artwork> {
    let tag = Tag::read_from_path(path).ok()?;
    let image = tag.artwork()?;
    let media_type = match image.fmt {
        ImgFmt::Bmp => "image/bmp",
        ImgFmt::Jpeg => "image/jpeg",
        ImgFmt::Png => "image/png",
    };
    let provider = metadata
        .get("provider")
        .and_then(|label| {
            Provider::ALL
                .into_iter()
                .find(|provider| provider.label().eq_ignore_ascii_case(label))
        })
        .unwrap_or(Provider::ITunes);
    let source_url = metadata.get("webpage_url").cloned().unwrap_or_default();
    Some(Artwork {
        bytes: image.data.to_vec(),
        media_type: media_type.into(),
        source_url,
        provider,
    })
}

fn read_private_metadata(path: &Path, metadata: &mut BTreeMap<String, String>) {
    let Ok(tag) = Tag::read_from_path(path) else {
        return;
    };
    for (key, name) in PRIVATE_METADATA_FIELDS {
        for mean in [METADATA_MEAN, LEGACY_METADATA_MEAN] {
            let ident = FreeformIdent::new_static(mean, name);
            if let Some(value) = tag.strings_of(&ident).next() {
                metadata.insert(key.into(), value.into());
                break;
            }
        }
    }
    for mean in [METADATA_MEAN, LEGACY_METADATA_MEAN] {
        let ident = FreeformIdent::new_static(mean, "attribution");
        if let Some(value) = tag.strings_of(&ident).next() {
            metadata.insert("metadata_attribution".into(), value.into());
            break;
        }
    }
}

impl Document {
    pub fn open(path: &Path) -> Result<Self> {
        let path = local_file(path)?;
        let result = probe(&path)?;
        ensure!(!result.streams.is_empty(), "The file contains no tracks");
        let mut metadata: BTreeMap<_, _> = result
            .format
            .tags
            .into_iter()
            .map(|(key, value)| (metadata_key(&key), value))
            .collect();
        read_private_metadata(&path, &mut metadata);
        let artwork = read_itunes_artwork(&path, &metadata);
        let has_chapters = !result.chapters.is_empty();
        let tracks = result.streams.into_iter().filter_map(|stream| {
            let attached = stream.disposition.get("attached_pic") == Some(&1);
            if attached && artwork.is_some() {
                return None;
            }
            let handler = tag(&stream.tags, "handler_name");
            let chapter_data = has_chapters
                && stream.codec_type == "data"
                && stream.codec_name == "bin_data"
                && handler.eq_ignore_ascii_case("SubtitleHandler");
            if chapter_data {
                return None;
            }
            let supported = match stream.codec_type.as_str() {
                "video" if attached => matches!(stream.codec_name.as_str(), "mjpeg" | "png"),
                "video" => matches!(stream.codec_name.as_str(), "h264" | "hevc" | "av1" | "mpeg4" | "vp9"),
                "audio" => true,
                "subtitle" => matches!(stream.codec_name.as_str(), "subrip" | "srt" | "ass" | "ssa" | "mov_text" | "webvtt" | "text"),
                _ => false,
            };
            let unsupported = (!supported).then(|| match stream.codec_type.as_str() {
                "subtitle" => "Bitmap subtitles or unsupported format: external OCR conversion is required.".to_string(),
                "video" => format!("The {} codec is not supported for MP4 stream copy.", stream.codec_name),
                _ => "Data track or attachment: excluded from export.".to_string(),
            });
            let language = tag(&stream.tags, "language");
            let language = language::normalize(&language).unwrap_or("und").to_owned();
            let title = tag(&stream.tags, "title");
            let title = if title.is_empty() { handler } else { title };
            let details = match stream.codec_type.as_str() {
                "video" if attached => "Embedded artwork".into(),
                "video" => format!("{} × {}", stream.width.unwrap_or(0), stream.height.unwrap_or(0)),
                "audio" => format!("{} {} · {} Hz", stream.channels.unwrap_or(0), if stream.channels == Some(1) { "channel" } else { "channels" }, stream.sample_rate.as_deref().unwrap_or("?")),
                "subtitle" => if supported { "Text subtitles".into() } else { "Non-text subtitles".into() },
                _ => "Data / attachment".into(),
            };
            let audio_mode = if stream.codec_type == "audio"
                && !matches!(stream.codec_name.as_str(), "aac" | "ac3" | "eac3" | "alac" | "mp3") {
                AudioMode::Aac
            } else { AudioMode::Copy };
            Some(Track {
                source: path.clone(), index: stream.index, kind: stream.codec_type, codec: stream.codec_name,
                details, language, title, enabled: supported, unsupported, audio_mode,
                default: stream.disposition.get("default") == Some(&1),
                forced: stream.disposition.get("forced") == Some(&1),
                dispositions: stream.disposition.into_iter().filter(|(_, value)| *value == 1).map(|(key, _)| key).collect(),
                attached_picture: attached,
            })
        }).collect();
        Ok(Self {
            size: fs::metadata(&path)?.len(),
            path,
            duration: result
                .format
                .duration
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value >= 0.0)
                .unwrap_or(0.0),
            tracks,
            metadata,
            chapters: result.chapters,
            artwork,
        })
    }

    pub fn add_subtitle(&mut self, path: &Path, language: &str) -> Result<()> {
        let code = language::normalize(language).context("Invalid language code")?;
        let path = local_file(path)?;
        ensure!(
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("srt")),
            "External subtitle import supports SRT files"
        );
        ensure!(
            !self.tracks.iter().any(|track| track.source == path),
            "This subtitle has already been added"
        );
        let subtitle = Self::open(&path)?;
        let mut track = subtitle
            .tracks
            .into_iter()
            .find(|track| track.kind == "subtitle" && track.unsupported.is_none())
            .context("The file does not contain valid text subtitles")?;
        track.language = code.into();
        track.title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        track.default = false;
        self.tracks.push(track);
        Ok(())
    }

    pub fn suggested_output(&self) -> PathBuf {
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        self.path.with_file_name(format!("{stem}-edited.mp4"))
    }

    pub fn apply_metadata(&mut self, result: MetadataResult, artwork: Option<Artwork>) {
        self.metadata.extend(result.fields);
        if let Some(source_url) = result.source_url {
            self.metadata.insert("webpage_url".into(), source_url);
        }
        self.metadata
            .insert("metadata_attribution".into(), result.attribution);
        if let Some(artwork) = artwork {
            self.set_artwork(artwork);
        }
    }

    pub fn set_artwork(&mut self, artwork: Artwork) {
        for track in &mut self.tracks {
            if track.attached_picture {
                track.enabled = false;
            }
        }
        self.artwork = Some(artwork);
    }
}

#[derive(Debug)]
pub enum ExportEvent {
    Stage(String),
    Progress(Option<f64>),
}

fn write_itunes_metadata(path: &Path, doc: &Document) -> Result<()> {
    let mut tag = Tag::read_from_path(path).context("Unable to read MP4 atoms")?;
    let value = |key: &str| doc.metadata.get(key).filter(|value| !value.is_empty());
    if let Some(value) = value("title") {
        tag.set_title(value);
    }
    if let Some(value) = value("date") {
        tag.set_year(value);
    }
    if let Some(value) = value("genre") {
        tag.set_custom_genre(value);
    }
    if let Some(value) = value("description") {
        tag.set_description(value);
    }
    if let Some(value) = value("comment") {
        tag.set_comment(value);
    }
    if let Some(value) = value("composer") {
        tag.set_composer(value);
    }
    if let Some(value) = value("copyright") {
        tag.set_copyright(value);
    }
    if let Some(show) = value("show") {
        tag.set_tv_show_name(show);
        tag.set_media_type(MediaType::TvShow);
        if let Some(title) = value("title") {
            tag.set_tv_episode_name(title);
        }
    } else {
        tag.set_media_type(MediaType::Movie);
    }
    if let Some(value) = value("network") {
        tag.set_tv_network_name(value);
    }
    if let Some(value) = value("season_number") {
        tag.set_tv_season(value.parse().context("Invalid season")?);
    }
    if let Some(value) = value("episode_sort") {
        tag.set_tv_episode(value.parse().context("Invalid episode")?);
    }
    for (key, name) in PRIVATE_METADATA_FIELDS
        .into_iter()
        .chain([("metadata_attribution", "attribution")])
    {
        tag.remove_data_of(&FreeformIdent::new_static(METADATA_MEAN, name));
        tag.remove_data_of(&FreeformIdent::new_static(LEGACY_METADATA_MEAN, name));
        if let Some(value) = value(key) {
            tag.set_data(
                FreeformIdent::new_static(METADATA_MEAN, name),
                Data::Utf8(value.clone()),
            );
        }
    }
    if let Some(artwork) = doc.artwork.as_ref() {
        if artwork.media_type == "image/png" {
            tag.set_artwork(Img::png(artwork.bytes.clone()));
        } else {
            tag.set_artwork(Img::jpeg(artwork.bytes.clone()));
        }
    }
    tag.write_to_path(path)
        .context("Unable to write iTunes metadata and artwork")
}

/// Export to a temporary file on the destination filesystem, validate, then publish
/// without replacing any existing destination. All process arguments bypass a shell.
pub fn export(
    doc: &Document,
    destination: &Path,
    cancel: &AtomicBool,
    mut notify: impl FnMut(ExportEvent),
) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Export cancelled");
    ensure!(
        destination
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4")),
        "The destination file must use the .mp4 extension"
    );
    ensure!(
        destination.symlink_metadata().is_err(),
        "The destination file already exists. Choose a new name"
    );
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .context("Destination directory is unavailable")?;
    let destination = parent.join(
        destination
            .file_name()
            .context("Destination file name is missing")?,
    );
    let selected: Vec<_> = doc.tracks.iter().filter(|track| track.enabled).collect();
    ensure!(!selected.is_empty(), "Select at least one track to export");
    let mut inputs = vec![local_file(&doc.path)?];
    for track in &selected {
        ensure!(
            track.unsupported.is_none(),
            "A selected track is unsupported: {}",
            track.codec
        );
        ensure!(
            language::normalize(&track.language).is_some(),
            "Invalid language for track {}: {}",
            track.index,
            track.language
        );
        let source = local_file(&track.source)?;
        if !inputs.contains(&source) {
            inputs.push(source);
        }
    }
    let temp = tempfile::Builder::new()
        .prefix(".reelmux-")
        .suffix(".mp4")
        .tempfile_in(&parent)
        .context("Unable to create a temporary file in the selected directory")?;
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]);
    for path in &inputs {
        command
            .args(["-protocol_whitelist", "file,pipe", "-i"])
            .arg(path);
    }
    for track in &selected {
        let source = local_file(&track.source)?;
        let input_index = inputs
            .iter()
            .position(|path| *path == source)
            .context("Track source was not found")?;
        command.args(["-map", &format!("{input_index}:{}", track.index)]);
    }
    command.args(["-map_metadata", "0", "-map_chapters", "0", "-c", "copy"]);
    for (index, track) in selected.iter().enumerate() {
        if track.kind == "subtitle" {
            command.args([format!("-c:{index}"), "mov_text".into()]);
        } else if track.kind == "audio" && track.audio_mode == AudioMode::Aac {
            command.args([
                format!("-c:{index}"),
                "aac".into(),
                format!("-b:{index}"),
                "192k".into(),
            ]);
        }
        let code = language::normalize(&track.language).context("Invalid language")?;
        for value in [
            format!("language={code}"),
            format!("title={}", track.title),
            format!("handler_name={}", track.title),
        ] {
            command.args([format!("-metadata:s:{index}"), value]);
        }
        let mut flags: Vec<&str> = track
            .dispositions
            .iter()
            .map(String::as_str)
            .filter(|flag| *flag != "default" && *flag != "forced")
            .collect();
        if track.default {
            flags.push("default");
        }
        if track.forced {
            flags.push("forced");
        }
        command.args([
            format!("-disposition:{index}"),
            if flags.is_empty() {
                "0".into()
            } else {
                flags.join("+")
            },
        ]);
    }
    for key in [
        "title",
        "date",
        "genre",
        "description",
        "comment",
        "show",
        "season_number",
        "episode_sort",
        "content_rating",
        "cast",
        "director",
        "producers",
        "screenwriters",
        "composer",
        "studio",
        "network",
        "provider",
        "provider_id",
        "webpage_url",
        "metadata_attribution",
    ] {
        if let Some(value) = doc.metadata.get(key) {
            if key == "season_number" || key == "episode_sort" {
                ensure!(
                    value.is_empty() || value.parse::<u32>().is_ok(),
                    "Season and episode must be positive integers"
                );
            }
            command.args(["-metadata", &format!("{key}={value}")]);
        }
    }
    command
        .args([
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
            "-f",
            "mp4",
        ])
        .arg(temp.path());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    notify(ExportEvent::Stage("Exporting tracks...".into()));
    let mut child = command
        .spawn()
        .context("Unable to start FFmpeg. Make sure it is installed and available in PATH")?;
    let stdout = child
        .stdout
        .take()
        .context("FFmpeg: stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("FFmpeg: stderr is unavailable")?;
    let (tx, rx) = mpsc::channel();
    let progress = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(value) = line
                .strip_prefix("out_time_us=")
                .and_then(|value| value.parse::<f64>().ok())
            {
                let _ = tx.send(value / 1_000_000.0);
            }
        }
    });
    let errors = thread::spawn(move || {
        let mut tail = std::collections::VecDeque::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tail.len() == 40 {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        tail.into_iter().collect::<Vec<_>>().join("\n")
    });
    let status = loop {
        for seconds in rx.try_iter() {
            notify(ExportEvent::Progress(
                (doc.duration > 0.0).then(|| (seconds / doc.duration).clamp(0.0, 0.99)),
            ));
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(40));
    };
    let _ = progress.join();
    let error_text = errors
        .join()
        .unwrap_or_else(|_| "Error output reader stopped unexpectedly".into());
    ensure!(!cancel.load(Ordering::Relaxed), "Export cancelled");
    ensure!(
        status.success(),
        "FFmpeg did not complete the export:\n{error_text}"
    );
    notify(ExportEvent::Stage("Writing metadata and artwork...".into()));
    write_itunes_metadata(temp.path(), doc)?;
    notify(ExportEvent::Stage("Verifying the exported file...".into()));
    let output = probe(temp.path()).context("Exported file verification failed")?;
    for kind in ["video", "audio", "subtitle"] {
        let expected = selected.iter().filter(|track| track.kind == kind).count()
            + usize::from(kind == "video" && doc.artwork.is_some());
        let actual = output
            .streams
            .iter()
            .filter(|track| track.codec_type == kind)
            .count();
        ensure!(
            expected == actual,
            "Verification failed: expected {expected} {kind} tracks, found {actual}"
        );
    }
    ensure!(!cancel.load(Ordering::Relaxed), "Export cancelled");
    temp.as_file().sync_all()?;
    temp.persist_noclobber(&destination).map_err(|error| {
        anyhow::anyhow!(
            "Unable to publish the file without overwriting the destination: {}",
            error.error
        )
    })?;
    notify(ExportEvent::Progress(Some(1.0)));
    Ok(())
}

pub fn duration_label(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
