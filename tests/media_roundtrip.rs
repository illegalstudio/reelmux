use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};
use subler_linux::media::{AudioMode, Document, ExportEvent, export};
use subler_linux::metadata::{Artwork, MetadataResult, Provider};

fn ffmpeg(args: &[&str]) {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .args(args)
        .output()
        .expect("FFmpeg must be installed for integration tests");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture(dir: &Path) -> Document {
    let chapters = dir.join("chapters.txt");
    fs::write(&chapters, ";FFMETADATA1\ntitle=Original title\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=Opening\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=2000\ntitle=Ending\n").unwrap();
    let input = dir.join("source with spaces è.mkv");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x180:rate=24",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000",
        "-f",
        "ffmetadata",
        "-i",
        chapters.to_str().unwrap(),
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-map_metadata",
        "2",
        "-map_chapters",
        "2",
        "-t",
        "2",
        "-c:v",
        "mpeg4",
        "-q:v",
        "4",
        "-c:a",
        "aac",
        "-metadata:s:a:0",
        "language=ita",
        input.to_str().unwrap(),
    ]);
    Document::open(&input).unwrap()
}

fn packet_hashes(path: &Path, stream: &str) -> Vec<String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            stream,
            "-show_packets",
            "-show_data_hash",
            "sha256",
            "-show_entries",
            "packet=data_hash",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    value["packets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["data_hash"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn remux_keeps_packets_chapters_and_writes_subtitle_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = fixture(dir.path());
    let original = fs::read(&doc.path).unwrap();
    assert_eq!(doc.chapters.len(), 2);
    let srt = dir.path().join("captions ; $(literal).srt");
    fs::write(&srt, "1\n00:00:00,200 --> 00:00:01,500\nCiao, città!\n").unwrap();
    doc.add_subtitle(&srt, "it").unwrap();
    doc.tracks.last_mut().unwrap().forced = true;
    doc.metadata
        .insert("title".into(), "Un film: città & mare $(literal)".into());
    doc.metadata.insert("show".into(), "Una serie".into());
    doc.metadata.insert("season_number".into(), "2".into());
    doc.metadata.insert("episode_sort".into(), "3".into());
    let destination = dir.path().join("result.mp4");
    let mut completed = false;
    export(&doc, &destination, &AtomicBool::new(false), |event| {
        if matches!(event, ExportEvent::Progress(Some(1.0))) {
            completed = true;
        }
    })
    .unwrap();
    assert!(completed);
    let output = Document::open(&destination).unwrap();
    assert_eq!(output.metadata["title"], doc.metadata["title"]);
    assert_eq!(output.metadata["show"], "Una serie");
    assert_eq!(output.metadata["season_number"], "2");
    assert_eq!(output.metadata["episode_sort"], "3");
    assert_eq!(output.chapters.len(), 2);
    assert_eq!(output.chapters[1].tags["title"], "Ending");
    let subtitle = output.tracks.iter().find(|t| t.kind == "subtitle").unwrap();
    assert_eq!(subtitle.codec, "mov_text");
    assert_eq!(subtitle.language, "ita");
    assert!(subtitle.forced);
    for stream in ["v:0", "a:0"] {
        assert_eq!(
            packet_hashes(&doc.path, stream),
            packet_hashes(&destination, stream)
        );
    }
    assert_eq!(fs::read(&doc.path).unwrap(), original);
    assert!(!fs::read_dir(dir.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".subler-")
    }));
}

#[test]
fn exclusion_and_aac_conversion_are_applied() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = fixture(dir.path());
    doc.tracks
        .iter_mut()
        .find(|t| t.kind == "video")
        .unwrap()
        .enabled = false;
    doc.tracks
        .iter_mut()
        .find(|t| t.kind == "audio")
        .unwrap()
        .audio_mode = AudioMode::Aac;
    let destination = dir.path().join("audio.mp4");
    export(&doc, &destination, &AtomicBool::new(false), |_| {}).unwrap();
    let output = Document::open(&destination).unwrap();
    assert!(!output.tracks.iter().any(|t| t.kind == "video"));
    assert_eq!(
        output
            .tracks
            .iter()
            .find(|t| t.kind == "audio")
            .unwrap()
            .codec,
        "aac"
    );
    assert_ne!(
        packet_hashes(&doc.path, "a:0"),
        packet_hashes(&destination, "a:0")
    );
}

#[test]
fn rejects_overwrite_invalid_selection_language_and_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = fixture(dir.path());
    let cancel = AtomicBool::new(false);
    let output = dir.path().join("output.mp4");
    fs::write(&output, b"keep this file").unwrap();
    assert!(export(&doc, &output, &cancel, |_| {}).is_err());
    assert_eq!(fs::read(&output).unwrap(), b"keep this file");
    let new_output = dir.path().join("new.mp4");
    doc.tracks[0].language = "xxx".into();
    assert!(export(&doc, &new_output, &cancel, |_| {}).is_err());
    assert!(!new_output.exists());
    doc.tracks[0].language = "und".into();
    // Trigger cancellation after preparation, while FFmpeg is starting.
    let result = export(&doc, &new_output, &cancel, |event| {
        if matches!(event, ExportEvent::Stage(_)) {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert!(result.unwrap_err().to_string().contains("annullata"));
    assert!(!new_output.exists());
    cancel.store(false, Ordering::Relaxed);
    for t in &mut doc.tracks {
        t.enabled = false;
    }
    assert!(export(&doc, &new_output, &cancel, |_| {}).is_err());
    assert!(!fs::read_dir(dir.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".subler-")
    }));
}

#[test]
fn invalid_files_and_duplicate_subtitles_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.mp4");
    fs::write(&path, b"not a movie").unwrap();
    assert!(Document::open(&path).is_err());
    assert!(Document::open(dir.path()).is_err());
    let mut doc = fixture(dir.path());
    let srt = dir.path().join("test.srt");
    fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,000\nHello\n").unwrap();
    doc.add_subtitle(&srt, "eng").unwrap();
    assert!(doc.add_subtitle(&srt, "eng").is_err());
}

#[test]
fn imported_metadata_and_poster_are_written_to_mp4() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = fixture(dir.path());
    let poster = dir.path().join("poster.jpg");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "color=c=0x6f4cff:s=600x900",
        "-frames:v",
        "1",
        poster.to_str().unwrap(),
    ]);
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("title".into(), "La città viola".into());
    fields.insert("director".into(), "Giulia Bianchi".into());
    fields.insert("provider".into(), "TheMovieDB".into());
    fields.insert("provider_id".into(), "42".into());
    doc.apply_metadata(
        MetadataResult {
            provider: Provider::Tmdb,
            fields,
            artwork_url: Some("https://image.example/poster.jpg".into()),
            source_url: Some("https://www.themoviedb.org/movie/42".into()),
            attribution: Provider::Tmdb.attribution().into(),
        },
        Some(Artwork {
            bytes: fs::read(&poster).unwrap(),
            media_type: "image/jpeg".into(),
            source_url: "https://image.example/poster.jpg".into(),
            provider: Provider::Tmdb,
        }),
    );
    let destination = dir.path().join("with-poster.mp4");
    export(&doc, &destination, &AtomicBool::new(false), |_| {}).unwrap();
    let output = Document::open(&destination).unwrap();
    assert_eq!(output.metadata["title"], "La città viola");
    assert_eq!(output.metadata["provider_id"], "42");
    assert_eq!(
        output.metadata["webpage_url"],
        "https://www.themoviedb.org/movie/42"
    );
    assert_eq!(
        output.metadata["metadata_attribution"],
        Provider::Tmdb.attribution()
    );
    let tag = mp4ameta::Tag::read_from_path(&destination).unwrap();
    assert_eq!(
        tag.strings_of(&mp4ameta::FreeformIdent::new_static(
            "io.github.sublerlinux.metadata",
            "director"
        ))
        .next(),
        Some("Giulia Bianchi")
    );
    assert_eq!(
        tag.strings_of(&mp4ameta::FreeformIdent::new_static(
            "io.github.sublerlinux.metadata",
            "provider"
        ))
        .next(),
        Some("TheMovieDB")
    );
    assert_eq!(tag.artwork().unwrap().data, fs::read(&poster).unwrap());
    assert_eq!(
        output.artwork.as_ref().unwrap().bytes,
        fs::read(&poster).unwrap()
    );
    assert!(!output.tracks.iter().any(|track| track.attached_picture));
    assert!(!output.tracks.iter().any(|track| track.kind == "data"));

    let reopened = dir.path().join("reopened.mp4");
    export(&output, &reopened, &AtomicBool::new(false), |_| {}).unwrap();
    let reopened_tag = mp4ameta::Tag::read_from_path(&reopened).unwrap();
    assert_eq!(
        reopened_tag.artwork().unwrap().data,
        fs::read(&poster).unwrap()
    );
}

#[test]
fn ffmpeg_failure_and_destination_race_leave_no_partial_output() {
    let dir = tempfile::tempdir().unwrap();
    let doc = fixture(dir.path());
    let destination = dir.path().join("raced.mp4");
    let result = export(&doc, &destination, &AtomicBool::new(false), |event| {
        if let ExportEvent::Stage(stage) = event
            && stage.starts_with("Verifica")
        {
            fs::write(&destination, b"created by another application").unwrap();
        }
    });
    assert!(result.is_err());
    assert_eq!(
        fs::read(&destination).unwrap(),
        b"created by another application"
    );
    fs::write(&doc.path, b"source changed after probing").unwrap();
    let broken = dir.path().join("broken-output.mp4");
    assert!(export(&doc, &broken, &AtomicBool::new(false), |_| {}).is_err());
    assert!(!broken.exists());
    assert!(!fs::read_dir(dir.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".subler-")
    }));
}
