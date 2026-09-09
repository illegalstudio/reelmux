# Provenienza e licenze

Subler Linux è un progetto indipendente ispirato a Subler. Non è una versione
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
Non sono stati copiati client API, credenziali, icone o binari macOS.

## Dipendenze

Le dipendenze Rust sono fissate in `Cargo.lock` e conservano le rispettive licenze.
GTK4 viene collegato alle librerie di sistema. FFmpeg e ffprobe vengono eseguiti
come programmi esterni e non sono distribuiti in questo repository.
Le opzioni di build e le licenze dei pacchetti FFmpeg dipendono dalla distribuzione.

L'icona SVG e il foglio di stile di Subler Linux sono nuovi e coperti dalla licenza
del progetto.
