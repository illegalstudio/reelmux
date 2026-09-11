# ReelMux

ReelMux è un editor MP4 per Linux scritto in Rust e GTK4, ispirato a
[Subler](https://subler.org/). Il prototipo offre un'interfaccia in italiano.

## Funzioni disponibili

- Apertura di MP4, M4V, MOV e MKV, anche trascinando un file nella finestra.
- Elenco delle tracce con codec, risoluzione o canali, nome e lingua modificabili.
- Inclusione ed esclusione delle tracce; flag predefinito e sottotitoli forzati.
- Aggiunta di SRT esterni e conversione dei sottotitoli testuali in `mov_text`.
- Copia del video e scelta tra copia audio e conversione AAC a 192 kbit/s.
- Modifica di titolo, data, genere, descrizione, serie, stagione ed episodio.
- Ricerca di film e serie su Apple TV, TheMovieDB, TheTVDB e iTunes Store.
- Importazione di descrizione, cast, troupe, studio, classificazione e altri campi disponibili.
- Selezione tra più locandine, anteprima e incorporamento nell'atom MP4 `covr`.
- Sostituzione della sola locandina, senza cambiare i metadati già presenti.
- Trasferimento dei capitoli originali.
- Esportazione in background, avanzamento, annullamento e verifica delle tracce.
- Interfaccia a riga di comando per ispezione ed esportazione.

L'esportazione produce un nuovo file. Una destinazione esistente viene rifiutata.
Il risultato viene scritto su un file temporaneo nella cartella di destinazione,
verificato e pubblicato senza sovrascrivere altri file. Se FFmpeg fallisce o
l'operazione viene annullata, il temporaneo viene rimosso.

## Requisiti e avvio

- Rust 1.92 o successivo e Cargo.
- GTK4 4.10 o successivo, con header di sviluppo e `pkg-config`.
- `ffmpeg` e `ffprobe` nel `PATH`.

Su Arch Linux:

```sh
sudo pacman -S --needed rust gtk4 pkgconf ffmpeg
```

Su Ubuntu 24.04 o successivo:

```sh
sudo apt install build-essential pkg-config libgtk-4-dev ffmpeg
```

Su Ubuntu serve inoltre una toolchain Rust recente, ad esempio tramite rustup.

```sh
cargo run --locked
cargo run --locked -- /percorso/al/video.mkv
```

Scorciatoie: `Ctrl+O` apre un file, `Ctrl+I` aggiunge un SRT,
`Ctrl+M` cerca i metadati, `Ctrl+Shift+M` cambia soltanto la locandina,
`Ctrl+Shift+S` esporta un MP4.

Per le lingue sono accettati codici a due o tre lettere: `it` / `ita`,
`en` / `eng`, `de` / `deu` / `ger`. Un valore vuoto equivale a `und`.
Le caselle nella colonna **Usa** determinano le tracce da esportare;
l'evidenziazione delle righe nella tabella non cambia questa scelta.

## Provider dei metadati

Apple TV e iTunes Store non richiedono configurazione. TheMovieDB e TheTVDB
richiedono credenziali personali, che l'applicazione legge dall'ambiente:

```sh
export TMDB_API_TOKEN="token-di-lettura-v4"
# In alternativa al token: export TMDB_API_KEY="chiave-v3"
export TVDB_API_KEY="chiave-api"
# Solo per gli account che lo richiedono: export TVDB_PIN="pin-abbonato"
cargo run --locked
```

Le chiavi comprese nel sorgente di Subler non vengono riutilizzate. La finestra
**Cerca online** consente di scegliere provider, film o serie, lingua, paese,
stagione ed episodio. Dopo la selezione mostra le locandine disponibili e scarica
in alta risoluzione soltanto quella scelta. Il pulsante **Cambia locandina** usa la
stessa ricerca senza modificare gli altri metadati. La disponibilità dei cataloghi
e il numero di immagini variano per paese e provider.

This product uses the TMDB API but is not endorsed or certified by TMDB.
Per i risultati TheTVDB, i metadati sono forniti da
[TheTVDB](https://thetvdb.com/); valuta di contribuire le informazioni mancanti
o di sottoscrivere un abbonamento. L'uso delle API resta soggetto ai termini dei
rispettivi provider.

## Prova con file sintetici

```sh
python scripts/create_demo.py
cargo run --locked -- artifacts/demo/Viaggio-notturno.mkv
```

Il generatore crea un video di prova e un SRT, senza utilizzare file personali.
Il sottotitolo può essere aggiunto dalla GUI o trascinato nella finestra aperta.

## Riga di comando

```sh
cargo run --locked -- inspect video.mkv
cargo run --locked -- export video.mkv risultato.mp4 --title "Titolo" --subtitle italiano.srt --language ita
cargo run --locked -- export video.mkv risultato.mp4 --exclude 2 --aac
cargo run --locked --no-default-features -- metadata "Dune" --provider apple-tv
cargo run --locked --no-default-features -- metadata "Dune" --provider apple-tv --select 1 --artwork dune.jpg
cargo run --locked --no-default-features -- metadata "Breaking Bad" --provider itunes --tv --season 5 --episode 1
```

`--exclude` usa l'indice originale della traccia, visibile nell'output di `inspect`.
`--aac` converte tutte le tracce audio incluse. `--subtitle` è ripetibile.
Per compilare soltanto la CLI e il motore, senza dipendenze GTK:

```sh
cargo build --locked --no-default-features
```

## Verifiche

```sh
make check
```

Il comando esegue formattazione, Clippy e tutti i test automatici. `make build`
crea il binario ottimizzato. Con nFPM installato, `make dist` prepara un archivio
Linux e i pacchetti Debian, RPM e Arch Linux.

`make appimage` scarica le versioni fissate di linuxdeploy e del plugin GTK4,
ne verifica i checksum e genera l'AppImage. `make packages` crea tutti i formati.
FFmpeg resta una dipendenza esterna anche per l'AppImage.

I test d'integrazione richiedono FFmpeg. Generano file locali temporanei e verificano
metadati, capitoli, lingue, sottotitoli forzati, esclusione delle tracce, conversione
AAC, annullamento e rifiuto della sovrascrittura. La copia audio/video viene
confrontata tramite hash SHA-256 dei pacchetti compressi. Il motore può essere
verificato senza un display e con `--no-default-features`.

Per verificare anche i controlli GTK in una sessione grafica:

```sh
cargo test --locked --bin reelmux gui_roundtrip -- --ignored --test-threads=1
```

Questo test apre una finestra con un video sintetico, modifica titolo, lingua,
inclusione e flag dei sottotitoli, quindi esporta e controlla il risultato.
Richiede anche Python 3. Impostando `REELMUX_TEST_SCREENSHOT` a un percorso PNG
si può acquisire la sola finestra del test, senza catturare il resto del desktop.

## Release

Le release partono da un tag semantico `vMAJOR.MINOR.PATCH` raggiungibile da
`main`. Il comando interattivo propone la patch successiva, oppure `v0.1.0`
quando non esistono ancora tag:

```sh
make release
```

La working tree deve essere pulita e `main` deve coincidere con `origin/main`.
Il comando accetta la versione proposta o una versione diversa, aggiorna
`Cargo.toml` e `Cargo.lock`, esegue `make check`, crea il commit di versione e un
tag annotato, quindi pubblica branch e tag con un unico push atomico.

Il workflow GitHub Actions verifica nuovamente tag, branch e versione, compila
su runner Ubuntu 24.04 nativi per Linux amd64 e arm64 e pubblica nella GitHub
Release gli archivi `.tar.gz`, i pacchetti `.deb`, `.rpm`, `.pkg.tar.zst`, le
AppImage e i checksum SHA-256 per entrambe le architetture.

## Limiti del prototipo

- Nessuna modifica dei capitoli o coda batch.
- Apple TV usa l'endpoint pubblico consultato da Subler, che non ha una specifica
  pubblica stabile. iTunes Store può non restituire film in alcuni cataloghi.
- La mappatura copre i principali campi offerti dai quattro provider. I rating
  territoriali complessi richiedono altro lavoro.
- I sottotitoli bitmap richiedono OCR esterno. Gli stili ASS/SSA possono essere persi
  nella conversione in testo MP4.
- Nessuna promessa di conservazione completa degli atom MP4 proprietari, dei legami
  tra tracce o dei dettagli HDR/Dolby Vision. Servono ulteriori verifiche dedicate.
- FFmpeg copia i metadati che riconosce; i campi esposti nella GUI sono scritti
  esplicitamente. Modificare i tag richiede la riscrittura del contenitore.
- I codec video previsti per la copia sono H.264, HEVC, AV1, MPEG-4 e VP9.
  Una traccia non supportata resta visibile ma viene esclusa. L'audio diverso da
  AAC, AC3, EAC3, ALAC e MP3 viene proposto per la conversione AAC.
- Le modifiche non esportate rimangono solo in memoria. La chiusura o l'apertura di
  un altro documento richiede conferma se ci sono modifiche pendenti.
- La verifica dopo l'esportazione controlla la leggibilità e il numero di tracce;
  non sostituisce una validazione completa su ogni lettore o dispositivo.

## Struttura

- `src/media.rs`: modello del documento, operazioni FFmpeg/ffprobe e atom MP4.
- `src/metadata.rs`: client e mappature per i quattro provider di metadati.
- `src/language.rs`: normalizzazione dei codici lingua.
- `src/ui.rs`: interfaccia GTK4 e comunicazione con i worker.
- `src/main.rs`: avvio GUI e CLI.
- `data/`: stile, icona e tabella delle lingue.
- `tests/`: test sui file multimediali.

Licenza GPL-2.0-only. Crediti e provenienza del materiale recuperato da Subler
in [THIRD_PARTY.md](THIRD_PARTY.md).
