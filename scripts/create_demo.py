#!/usr/bin/env python3
"""Generate local synthetic media for exercising the prototype."""
import argparse
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory", nargs="?", type=Path, default=Path("artifacts/demo"))
args = parser.parse_args()
args.directory.mkdir(parents=True, exist_ok=True)
metadata = args.directory / "chapters.ffmetadata"
metadata.write_text(
    ";FFMETADATA1\ntitle=Viaggio notturno\ndate=2026\ngenre=Documentario\n"
    "description=Un piccolo video sintetico per provare ReelMux.\n"
    "[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=6000\ntitle=Partenza\n"
    "[CHAPTER]\nTIMEBASE=1/1000\nSTART=6000\nEND=12000\ntitle=Arrivo\n",
    encoding="utf-8",
)
subtitle = args.directory / "Italiano.srt"
subtitle.write_text(
    "1\n00:00:00,500 --> 00:00:04,000\nIl viaggio comincia qui.\n\n"
    "2\n00:00:05,000 --> 00:00:09,000\nAudio, video e sottotitoli nello stesso MP4.\n",
    encoding="utf-8",
)
output = args.directory / "Viaggio-notturno.mkv"
subprocess.run([
    "ffmpeg", "-hide_banner", "-loglevel", "error", "-nostdin", "-n",
    "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=24",
    "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000",
    "-f", "lavfi", "-i", "sine=frequency=660:sample_rate=48000",
    "-f", "ffmetadata", "-i", str(metadata),
    "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map_metadata", "3", "-map_chapters", "3",
    "-t", "12", "-c:v", "mpeg4", "-q:v", "4", "-c:a", "aac",
    "-metadata:s:v:0", "title=Video principale",
    "-metadata:s:a:0", "language=ita", "-metadata:s:a:0", "title=Audio italiano",
    "-metadata:s:a:1", "language=eng", "-metadata:s:a:1", "title=English audio",
    str(output),
], check=True)
print(f"Video: {output.resolve()}\nSottotitoli: {subtitle.resolve()}")
