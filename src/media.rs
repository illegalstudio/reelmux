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
use serde::{Deserialize, Serialize};

use crate::language;

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
}

impl Track {
    pub fn operation(&self) -> &str {
        if self.unsupported.is_some() {
            "Non supportata"
        } else if self.kind == "subtitle" {
            "Testo MP4"
        } else if self.kind == "audio" && self.audio_mode == AudioMode::Aac {
            "Converti AAC"
        } else {
            "Copia"
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

fn local_file(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .with_context(|| format!("File non accessibile: {}", path.display()))?;
    ensure!(
        path.is_file(),
        "Seleziona un file regolare: {}",
        path.display()
    );
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
        .context("Impossibile avviare ffprobe. Installa FFmpeg e verifica che sia nel PATH")?;
    let mut stdout = child
        .stdout
        .take()
        .context("ffprobe: stdout non disponibile")?;
    let mut stderr = child
        .stderr
        .take()
        .context("ffprobe: stderr non disponibile")?;
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
            bail!("Analisi interrotta: ffprobe non ha risposto entro 30 secondi");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = output
        .join()
        .map_err(|_| anyhow::anyhow!("Lettura ffprobe interrotta"))??;
    let errors = errors
        .join()
        .map_err(|_| anyhow::anyhow!("Lettura errori ffprobe interrotta"))??;
    ensure!(
        status.success(),
        "Il file non è leggibile come contenuto multimediale: {}",
        errors.trim()
    );
    serde_json::from_slice(&output).context("Risposta ffprobe non valida")
}

impl Document {
    pub fn open(path: &Path) -> Result<Self> {
        let path = local_file(path)?;
        let result = probe(&path)?;
        ensure!(!result.streams.is_empty(), "Il file non contiene tracce");
        let tracks = result.streams.into_iter().map(|stream| {
            let attached = stream.disposition.get("attached_pic") == Some(&1);
            let supported = match stream.codec_type.as_str() {
                "video" if attached => matches!(stream.codec_name.as_str(), "mjpeg" | "png"),
                "video" => matches!(stream.codec_name.as_str(), "h264" | "hevc" | "av1" | "mpeg4" | "vp9"),
                "audio" => true,
                "subtitle" => matches!(stream.codec_name.as_str(), "subrip" | "srt" | "ass" | "ssa" | "mov_text" | "webvtt" | "text"),
                _ => false,
            };
            let unsupported = (!supported).then(|| match stream.codec_type.as_str() {
                "subtitle" => "Sottotitoli bitmap o formato non supportato: serve una conversione OCR esterna.".to_string(),
                "video" => format!("Il codec {} non è previsto per la copia MP4 nel prototipo.", stream.codec_name),
                _ => "Traccia dati o allegato: esclusa dall'esportazione del prototipo.".to_string(),
            });
            let language = tag(&stream.tags, "language");
            let language = language::normalize(&language).unwrap_or("und").to_owned();
            let title = tag(&stream.tags, "title");
            let title = if title.is_empty() { tag(&stream.tags, "handler_name") } else { title };
            let details = match stream.codec_type.as_str() {
                "video" if attached => "Copertina incorporata".into(),
                "video" => format!("{} × {}", stream.width.unwrap_or(0), stream.height.unwrap_or(0)),
                "audio" => format!("{} {} · {} Hz", stream.channels.unwrap_or(0), if stream.channels == Some(1) { "canale" } else { "canali" }, stream.sample_rate.as_deref().unwrap_or("?")),
                "subtitle" => if supported { "Sottotitoli testuali".into() } else { "Sottotitoli non testuali".into() },
                _ => "Dati / allegato".into(),
            };
            let audio_mode = if stream.codec_type == "audio"
                && !matches!(stream.codec_name.as_str(), "aac" | "ac3" | "eac3" | "alac" | "mp3") {
                AudioMode::Aac
            } else { AudioMode::Copy };
            Track {
                source: path.clone(), index: stream.index, kind: stream.codec_type, codec: stream.codec_name,
                details, language, title, enabled: supported, unsupported, audio_mode,
                default: stream.disposition.get("default") == Some(&1),
                forced: stream.disposition.get("forced") == Some(&1),
                dispositions: stream.disposition.into_iter().filter(|(_, value)| *value == 1).map(|(key, _)| key).collect(),
            }
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
            metadata: result
                .format
                .tags
                .into_iter()
                .map(|(key, value)| (key.to_ascii_lowercase(), value))
                .collect(),
            chapters: result.chapters,
        })
    }

    pub fn add_subtitle(&mut self, path: &Path, language: &str) -> Result<()> {
        let code = language::normalize(language).context("Codice lingua non valido")?;
        let path = local_file(path)?;
        ensure!(
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("srt")),
            "Il prototipo importa sottotitoli esterni in formato SRT"
        );
        ensure!(
            !self.tracks.iter().any(|track| track.source == path),
            "Questo sottotitolo è già presente"
        );
        let subtitle = Self::open(&path)?;
        let mut track = subtitle
            .tracks
            .into_iter()
            .find(|track| track.kind == "subtitle" && track.unsupported.is_none())
            .context("Il file non contiene sottotitoli testuali validi")?;
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
}

#[derive(Debug)]
pub enum ExportEvent {
    Stage(String),
    Progress(Option<f64>),
}

/// Export to a temporary file on the destination filesystem, validate, then publish
/// without replacing any existing destination. All process arguments bypass a shell.
pub fn export(
    doc: &Document,
    destination: &Path,
    cancel: &AtomicBool,
    mut notify: impl FnMut(ExportEvent),
) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Esportazione annullata");
    ensure!(
        destination
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4")),
        "Il file di destinazione deve avere estensione .mp4"
    );
    ensure!(
        destination.symlink_metadata().is_err(),
        "Il file di destinazione esiste già. Scegli un nuovo nome"
    );
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .context("Cartella di destinazione non disponibile")?;
    let destination = parent.join(destination.file_name().context("Nome del file mancante")?);
    let selected: Vec<_> = doc.tracks.iter().filter(|track| track.enabled).collect();
    ensure!(
        !selected.is_empty(),
        "Seleziona almeno una traccia da esportare"
    );
    let mut inputs = vec![local_file(&doc.path)?];
    for track in &selected {
        ensure!(
            track.unsupported.is_none(),
            "Una traccia selezionata non è supportata: {}",
            track.codec
        );
        ensure!(
            language::normalize(&track.language).is_some(),
            "Lingua non valida per la traccia {}: {}",
            track.index,
            track.language
        );
        let source = local_file(&track.source)?;
        if !inputs.contains(&source) {
            inputs.push(source);
        }
    }
    let temp = tempfile::Builder::new()
        .prefix(".subler-")
        .suffix(".mp4")
        .tempfile_in(&parent)
        .context("Impossibile creare un file temporaneo nella cartella scelta")?;
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
            .context("Sorgente della traccia non trovata")?;
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
        let code = language::normalize(&track.language).context("Lingua non valida")?;
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
    // Explicitly write editable iTunes-style fields. Other recognized metadata is
    // copied by FFmpeg; preserving arbitrary MP4 atoms is outside this prototype.
    for key in [
        "title",
        "date",
        "genre",
        "description",
        "comment",
        "show",
        "season_number",
        "episode_sort",
    ] {
        if let Some(value) = doc.metadata.get(key) {
            if key == "season_number" || key == "episode_sort" {
                ensure!(
                    value.is_empty() || value.parse::<u32>().is_ok(),
                    "Stagione ed episodio devono essere numeri interi positivi"
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
    notify(ExportEvent::Stage("Esportazione delle tracce…".into()));
    let mut child = command
        .spawn()
        .context("Impossibile avviare FFmpeg. Verifica che sia installato e nel PATH")?;
    let stdout = child
        .stdout
        .take()
        .context("FFmpeg: stdout non disponibile")?;
    let stderr = child
        .stderr
        .take()
        .context("FFmpeg: stderr non disponibile")?;
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
        .unwrap_or_else(|_| "Lettura degli errori interrotta".into());
    ensure!(!cancel.load(Ordering::Relaxed), "Esportazione annullata");
    ensure!(
        status.success(),
        "FFmpeg non ha completato l'esportazione:\n{error_text}"
    );
    notify(ExportEvent::Stage("Verifica del file esportato…".into()));
    let output = probe(temp.path()).context("Verifica del file esportato fallita")?;
    for kind in ["video", "audio", "subtitle"] {
        let expected = selected.iter().filter(|track| track.kind == kind).count();
        let actual = output
            .streams
            .iter()
            .filter(|track| track.codec_type == kind)
            .count();
        ensure!(
            expected == actual,
            "Verifica fallita: attese {expected} tracce {kind}, trovate {actual}"
        );
    }
    ensure!(!cancel.load(Ordering::Relaxed), "Esportazione annullata");
    temp.as_file().sync_all()?;
    temp.persist_noclobber(&destination).map_err(|error| {
        anyhow::anyhow!(
            "Impossibile pubblicare il file senza sovrascrivere la destinazione: {}",
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
