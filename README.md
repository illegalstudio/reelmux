<p align="center">
  <img src="data/io.github.nahime0.ReelMux.svg" alt="ReelMux logo" width="130">
</p>

<h1 align="center">ReelMux</h1>

<p align="center">
  <em>MP4 done right on Linux.</em>
</p>

<p align="center">
  <a href="https://github.com/nahime0/reelmux/stargazers"><img src="https://img.shields.io/github/stars/nahime0/reelmux?style=flat-square&amp;logo=github&amp;logoColor=white&amp;label=stars&amp;color=7759CE" alt="Stars"></a>
  <a href="https://github.com/nahime0/reelmux/releases"><img src="https://img.shields.io/github/v/release/nahime0/reelmux?style=flat-square&amp;logo=github&amp;logoColor=white&amp;label=release&amp;color=7759CE" alt="Latest release"></a>
  <a href="https://github.com/nahime0/reelmux/releases"><img src="https://img.shields.io/github/downloads/nahime0/reelmux/total?style=flat-square&amp;logo=github&amp;logoColor=white&amp;label=downloads&amp;color=7759CE" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/nahime0/reelmux?style=flat-square&amp;color=7759CE" alt="MIT License"></a>
</p>

<p align="center">
  <strong>Rust &middot; GTK4 &middot; FFmpeg &middot; MP4 and MKV</strong>
</p>

<p align="center">
  ReelMux is a native Linux desktop application for managing MP4 tracks,
  metadata, subtitles, and artwork, inspired by <a href="https://subler.org/">Subler</a>.
</p>

<p align="center">
  <a href="https://opensource.nahi.me"><strong>opensource.nahi.me</strong></a>
</p>

---

## Features

- Open MP4, M4V, MOV, and MKV files, including by drag and drop.
- Inspect tracks with codec, resolution, channel count, name, and language details.
- Include or exclude tracks and set default or forced flags.
- Add external SRT files and convert text subtitles to MP4 text.
- Copy video and audio streams, or convert audio to AAC at 192 kbit/s.
- Edit titles, release dates, genres, descriptions, TV show details, seasons, and episodes.
- Search Apple TV, TheMovieDB, TheTVDB, and iTunes Store for movies and TV shows.
- Import descriptions, cast, crew, studio, ratings, and other available metadata.
- Preview and choose from multiple artwork options, then embed the selected image in the MP4 `covr` atom.
- Replace artwork without changing existing metadata.
- Preserve source chapters.
- Export in the background with progress, cancellation, and output verification.

ReelMux always creates a new output file. It writes to a temporary file, verifies
the result, and publishes it only when the export succeeds. Existing files are
never overwritten.

## Installation

Download the AppImage or the package for your distribution from
[GitHub Releases](https://github.com/nahime0/reelmux/releases). Native packages
are available for Debian, RPM, and Arch Linux based distributions on amd64 and
arm64. FFmpeg and FFprobe must be installed and available in `PATH`.

ReelMux requires GTK 4.10 or newer. The AppImage includes GTK, but still uses the
FFmpeg installation provided by the system.

## Using ReelMux

Open ReelMux and choose a media file, or pass its path when starting the app:

```sh
reelmux /path/to/video.mkv
```

Useful shortcuts:

- `Ctrl+O`: open a media file.
- `Ctrl+I`: add an SRT subtitle file.
- `Ctrl+M`: search for metadata and artwork.
- `Ctrl+Shift+M`: replace artwork only.
- `Ctrl+Shift+S`: export an MP4 file.

Language fields accept two-letter or three-letter codes such as `en` / `eng`,
`de` / `deu`, and `fr` / `fra`. An empty value is stored as `und`. The checkboxes
in the **Use** column control which tracks are exported.

## Metadata providers

Apple TV and iTunes Store work without configuration. TheMovieDB and TheTVDB
require personal credentials supplied through environment variables:

```sh
export TMDB_API_TOKEN="your-v4-read-token"
# Alternatively: export TMDB_API_KEY="your-v3-key"
export TVDB_API_KEY="your-api-key"
# Required only for accounts that use one: export TVDB_PIN="your-subscriber-pin"
reelmux
```

The **Search online** window lets you choose a provider, media type, language,
country, season, and episode. ReelMux downloads the full-size version of the
artwork only after you select it. Available titles and images vary by provider
and country.

This product uses the TMDB API but is not endorsed or certified by TMDB.
Metadata returned by TheTVDB is provided by [TheTVDB](https://thetvdb.com/).
API use remains subject to each provider's terms.

## Command line

ReelMux also provides commands for inspection, export, and metadata lookup:

```sh
reelmux inspect video.mkv
reelmux export video.mkv output.mp4 --title "Title" --subtitle english.srt --language eng
reelmux export video.mkv output.mp4 --exclude 2 --aac
reelmux metadata "Dune" --provider apple-tv
reelmux metadata "Dune" --provider apple-tv --select 1 --artwork dune.jpg
reelmux metadata "Breaking Bad" --provider itunes --tv --season 5 --episode 1
```

`--exclude` uses the original track index shown by `inspect`. `--aac` converts
every included audio track. `--subtitle` can be supplied more than once.

## Current limitations

- Chapter editing and batch queues are not available yet.
- Bitmap subtitles require external OCR. ASS and SSA styling may be lost when converted to MP4 text.
- Proprietary MP4 atoms, track relationships, and all HDR or Dolby Vision details may not be preserved.
- Video stream copy currently supports H.264, HEVC, AV1, MPEG-4, and VP9.
- Audio formats other than AAC, AC3, EAC3, ALAC, and MP3 are offered as AAC conversions.
- Unsaved changes remain in memory until an export completes.

## License

ReelMux is available under the [MIT License](LICENSE). See
[THIRD_PARTY.md](THIRD_PARTY.md) for attribution and third-party notices.
