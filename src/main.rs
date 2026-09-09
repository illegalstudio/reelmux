use std::{fs::OpenOptions, io::Write, path::PathBuf, process::ExitCode, sync::atomic::AtomicBool};

use anyhow::Result;
use clap::{Parser, Subcommand};
use subler_linux::media::{AudioMode, Document, ExportEvent, export};
use subler_linux::metadata::{Client as MetadataClient, MediaKind, Provider, SearchQuery};

#[cfg(feature = "gui")]
mod ui;

#[derive(Parser)]
#[command(version, about = "Editor MP4 per Linux, ispirato a Subler")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Apri un file nell'interfaccia grafica.
    file: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Mostra tracce, capitoli e metadati in JSON.
    Inspect { input: PathBuf },
    /// Esporta un nuovo MP4 senza sovrascrivere file esistenti.
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
    /// Cerca metadati e locandine nei provider supportati da Subler.
    Metadata {
        query: String,
        #[arg(long, default_value = "apple-tv")]
        provider: Provider,
        #[arg(long)]
        tv: bool,
        #[arg(long, default_value = "it-IT")]
        language: String,
        #[arg(long, default_value = "IT")]
        country: String,
        #[arg(long)]
        season: Option<u32>,
        #[arg(long)]
        episode: Option<u32>,
        /// Risolvi il risultato indicato, partendo da 1.
        #[arg(long)]
        select: Option<usize>,
        /// Salva la locandina del risultato selezionato senza sovrascrivere.
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
                    .ok_or_else(|| anyhow::anyhow!("Traccia {index} inesistente"))?;
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
            println!("Salvato: {}", output.display());
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
                    .ok_or_else(|| anyhow::anyhow!("Risultato {selected} inesistente"))?;
                let result = client.resolve(hit, &query)?;
                if let Some(path) = artwork
                    && let Some(image) = client.download_artwork(&result)?
                {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)?;
                    file.write_all(&image.bytes)?;
                    eprintln!("Locandina salvata: {}", path.display());
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
                "GUI non inclusa. Ricompila con le funzionalità predefinite o usa inspect/export."
            );
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Errore: {error:#}");
            ExitCode::FAILURE
        }
    }
}
