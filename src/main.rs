use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool};

use anyhow::Result;
use clap::{Parser, Subcommand};
use subler_linux::media::{AudioMode, Document, ExportEvent, export};

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
