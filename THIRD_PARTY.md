# Attribution and third-party notices

ReelMux is an independent project inspired by Subler. It is not an official
SublerApp project. ReelMux source code is released under the MIT License.

## Subler references

The functional separation between documents, tracks, and metadata import was
informed by Subler. The following Subler and MP42Foundation sources were
consulted while designing ReelMux:

- The language table in `MP42/MP42Languages.m` for expected ISO 639 normalization behavior.
- `Classes/MetadataImporters/MetadataResultMap.swift` and `MP42Metadata.m` for movie and TV metadata fields.
- `AppleTV.swift`, `TheMovieDB.swift`, `TheTVDB.swift`, and `iTunesStore.swift` for provider capabilities and field mappings.

References:

- <https://github.com/SublerApp/MP42Foundation/blob/740fd075b333a03df4ff3d3a141f0c4f4f62b273/MP42/MP42Languages.m>
- <https://github.com/SublerApp/Subler/tree/b0d8ee73a89477a3b50b13e59f1ec2f19f1a35ae/Classes/MetadataImporters>
- <https://github.com/SublerApp/Subler/blob/b0d8ee73a89477a3b50b13e59f1ec2f19f1a35ae/LICENSE>

The language data in `data/languages.json` contains factual English names and
ISO 639 identifiers assembled for ReelMux. The Rust file operations, provider
clients, user interface, icon, and stylesheet are original ReelMux work. ReelMux
does not include credentials, icons, binaries, or source code copied from Subler.

Each metadata provider has its own terms and attribution requirements:

- Apple Search API: <https://developer.apple.com/library/archive/documentation/AudioVideo/Conceptual/iTuneSearchAPI/>
- TheMovieDB API: <https://developer.themoviedb.org/docs/getting-started>
- TheTVDB API and licensing: <https://thetvdb.com/api-information>

`data/tmdb-logo.svg` is the official short blue TMDB logo downloaded from the
[Logos and Attribution](https://www.themoviedb.org/about/logos-attribution)
page. The TMDB trademark belongs to TMDB and is used only for the required
attribution.

ReelMux requires personal credentials for TheMovieDB and TheTVDB and does not
distribute third-party credentials. Images and metadata remain subject to the
terms of the selected provider.

## Dependencies

Rust dependencies, including `ureq`, `url`, and `mp4ameta`, are pinned in
`Cargo.lock` and retain their respective licenses.

Native packages link GTK4 from the system. AppImages include GTK4 and dynamic
libraries collected by linuxdeploy, all subject to their respective licenses.
FFmpeg and FFprobe run as external programs and are not included in ReelMux
packages. Their build options and licenses depend on the Linux distribution.

AppImages are built with pinned, checksum-verified versions of these tools:

- linuxdeploy: <https://github.com/linuxdeploy/linuxdeploy>
- linuxdeploy-plugin-gtk: <https://github.com/linuxdeploy/linuxdeploy-plugin-gtk>
- AppImage type 2 runtime: <https://github.com/AppImage/type2-runtime>
