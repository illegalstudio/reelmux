use std::{fs::OpenOptions, io::Write, path::PathBuf, process::ExitCode, sync::atomic::AtomicBool};

use anyhow::Result;
use clap::{Parser, Subcommand};
use reelmux::media::{AudioMode, Document, ExportEvent, export};
use reelmux::metadata::{Client as MetadataClient, MediaKind, Provider, SearchQuery};

#[cfg(feature = "gui")]
mod ui;

#[derive(Parser)]
#[command(version, about = "ReelMux, an MP4 editor for Linux")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Open a file in the graphical interface.
    file: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show tracks, chapters, and metadata as JSON.
    Inspect { input: PathBuf },
    /// Export a new MP4 without overwriting existing files.
    Export {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        subtitle: Vec<PathBuf>,
        #[arg(long, default_value = "und")]
        language: String,
        #[arg(long)]
        exclude: Vec<u32>,
        #[arg(long)]
        aac: bool,
    },
    /// Search supported online providers for metadata and artwork.
    Metadata {
        query: String,
        #[arg(long, default_value = "apple-tv")]
        provider: Provider,
        #[arg(long)]
        tv: bool,
        #[arg(long, default_value = "en-US")]
        language: String,
        #[arg(long, default_value = "US")]
        country: String,
        #[arg(long)]
        season: Option<u32>,
        #[arg(long)]
        episode: Option<u32>,
        /// Resolve the selected result, starting from 1.
        #[arg(long)]
        select: Option<usize>,
        /// Save the selected result artwork without overwriting an existing file.
        #[arg(long, requires = "select")]
        artwork: Option<PathBuf>,
    },
}

fn run() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Some(Commands::Inspect { input }) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&Document::open(&input)?)?
            );
        }
        Some(Commands::Export {
            input,
            output,
            title,
            subtitle,
            language,
            exclude,
            aac,
        }) => {
            let mut doc = Document::open(&input)?;
            for index in exclude {
                let track = doc
                    .tracks
                    .iter_mut()
                    .find(|track| track.index == index)
                    .ok_or_else(|| anyhow::anyhow!("Track {index} does not exist"))?;
                track.enabled = false;
            }
            if let Some(title) = title {
                doc.metadata.insert("title".into(), title);
            }
            if aac {
                for track in &mut doc.tracks {
                    if track.kind == "audio" {
                        track.audio_mode = AudioMode::Aac;
                    }
                }
            }
            for path in subtitle {
                doc.add_subtitle(&path, &language)?;
            }
            let cancel = AtomicBool::new(false);
            export(&doc, &output, &cancel, |event| {
                if let ExportEvent::Stage(stage) = event {
                    eprintln!("{stage}");
                }
            })?;
            println!("Saved: {}", output.display());
        }
        Some(Commands::Metadata {
            query,
            provider,
            tv,
            language,
            country,
            season,
            episode,
            select,
            artwork,
        }) => {
            let query = SearchQuery {
                provider,
                kind: if tv {
                    MediaKind::TvShow
                } else {
                    MediaKind::Movie
                },
                term: query,
                language,
                country,
                season,
                episode,
            };
            let client = MetadataClient::default();
            let hits = client.search(&query)?;
            if let Some(selected) = select {
                let hit = selected
                    .checked_sub(1)
                    .and_then(|index| hits.get(index))
                    .ok_or_else(|| anyhow::anyhow!("Result {selected} does not exist"))?;
                let result = client.resolve(hit, &query)?;
                if let Some(path) = artwork
                    && let Some(image) = client.download_artwork(&result)?
                {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)?;
                    file.write_all(&image.bytes)?;
                    eprintln!("Artwork saved: {}", path.display());
                }
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            }
        }
        None => {
            #[cfg(feature = "gui")]
            return ui::run(args.file);
            #[cfg(not(feature = "gui"))]
            anyhow::bail!(
                "GUI support is not included. Rebuild with default features or use inspect/export."
            );
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
