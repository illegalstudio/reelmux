# Provenienza e licenze

ReelMux è un progetto indipendente ispirato a Subler. Non è una versione
ufficiale di SublerApp. Il codice di questo prototipo è distribuito sotto GPL-2.0-only.

## Materiale recuperato da Subler

- `data/languages.json` è un adattamento della tabella `languages[]` in
  `MP42/MP42Languages.m`, file creato da Damiano Galassi in MP42Foundation.
  Sono stati conservati nome inglese e codici ISO 639-1, 639-2/T e 639-2/B.
  Sono stati omessi i nomi nativi e gli identificativi QuickTime; la punteggiatura
  dei nomi è stata normalizzata dove necessario.
- Sorgente: <https://github.com/SublerApp/MP42Foundation/blob/740fd075b333a03df4ff3d3a141f0c4f4f62b273/MP42/MP42Languages.m>.
- Il file `LICENSE` riproduce `COPYING` di Subler al commit
  `b0d8ee73a89477a3b50b13e59f1ec2f19f1a35ae`.
- La dichiarazione di licenza di Subler è disponibile in
  <https://github.com/SublerApp/Subler/blob/b0d8ee73a89477a3b50b13e59f1ec2f19f1a35ae/LICENSE>.

La normalizzazione delle lingue usa la tabella recuperata. La separazione tra
documento, tracce e importazione prende come riferimento funzionale Subler.
Le implementazioni Rust delle operazioni sui file e dell'interfaccia sono nuove.
Le mappature di `Classes/MetadataImporters/MetadataResultMap.swift` e il modello
di `MP42Metadata.m` sono stati consultati per individuare i campi di film e serie.
I file `AppleTV.swift`, `TheMovieDB.swift`, `TheTVDB.swift` e `iTunesStore.swift`
sono stati consultati per identificare i provider, le risorse usate e la
corrispondenza dei campi. I client Rust sono implementazioni nuove. Non sono
state copiate le credenziali presenti nel sorgente originale, né icone o binari
macOS.

I provider hanno termini e requisiti di attribuzione propri:

- Apple Search API: <https://developer.apple.com/library/archive/documentation/AudioVideo/Conceptual/iTuneSearchAPI/>
- TheMovieDB API: <https://developer.themoviedb.org/docs/getting-started>
- TheTVDB API e licenze: <https://thetvdb.com/api-information>
- Sorgente degli importer Subler consultati:
  <https://github.com/SublerApp/Subler/tree/b0d8ee73a89477a3b50b13e59f1ec2f19f1a35ae/Classes/MetadataImporters>

`data/tmdb-logo.svg` è il logo corto blu ufficiale scaricato dalla pagina
[Logos & Attribution](https://www.themoviedb.org/about/logos-attribution). Il
marchio appartiene a TMDB e viene usato unicamente per l'attribuzione richiesta.

ReelMux richiede chiavi personali per TheMovieDB e TheTVDB. Non distribuisce
credenziali di terzi. Le immagini e i dati scaricati restano soggetti ai termini
del provider selezionato.

## Dipendenze

Le dipendenze Rust, inclusi `ureq`, `url` e `mp4ameta`, sono fissate in
`Cargo.lock` e conservano le rispettive licenze.
GTK4 viene collegato alle librerie di sistema. FFmpeg e ffprobe vengono eseguiti
come programmi esterni e non sono distribuiti in questo repository.
Le opzioni di build e le licenze dei pacchetti FFmpeg dipendono dalla distribuzione.

L'icona SVG e il foglio di stile di ReelMux sono nuovi e coperti dalla licenza
del progetto.
