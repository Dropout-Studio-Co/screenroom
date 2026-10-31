#!/usr/bin/env bash

set -uo pipefail

root="${1:?usage: probe-library.sh <media-root> <output.jsonl>}"
out="${2:?usage: probe-library.sh <media-root> <output.jsonl>}"
jobs="${PROBE_JOBS:-4}"

if [ ! -d "$root" ]; then
  echo "not a directory: $root" >&2
  exit 2
fi
command -v ffprobe >/dev/null || { echo "ffprobe is not installed" >&2; exit 2; }
command -v jq >/dev/null || { echo "jq is not installed" >&2; exit 2; }

mkdir -p "$(dirname "$out")"

work=$(mktemp -d -t screenroom-probe)
trap 'rm -rf "$work"' EXIT

find "$root" -type f \
  \( -iname '*.mkv' -o -iname '*.mp4' -o -iname '*.m4v' -o -iname '*.avi' \
     -o -iname '*.mov' -o -iname '*.webm' -o -iname '*.mpeg' -o -iname '*.mpg' \
     -o -iname '*.ts' -o -iname '*.m2ts' -o -iname '*.wmv' -o -iname '*.flv' \
     -o -iname '*.ogv' -o -iname '*.3gp' \) \
  -not -name '._*' -not -path '*/.*' \
  -not -path '*/Auto Data Backup/*' -not -path '*arkham project media*' \
  -print0 2>/dev/null |
  PROBE_WORK="$work" xargs -0 -n 1 -P "$jobs" bash -c '
    # $0 is the root, so the file xargs appended is $1; the worker directory
    # travels in the environment rather than as another positional, where it
    # would land after the filename.
    file="$1"
    relative="${file#"$0"/}"
    # `-v error` keeps warnings out of the JSON and a non-zero exit is reported
    # rather than swallowed. ffprobe has no `-nostdin` (that is an ffmpeg option);
    # passing it fails every file with "Option not found", which is exactly how
    # this harness first reported the whole library as unreadable.
    stderr=$(mktemp -t screenroom-probe)
    if json=$(ffprobe -v error -show_entries \
        "format=format_name,duration,size,bit_rate:format_tags=title:stream=index,codec_type,codec_name,profile,width,height,pix_fmt,r_frame_rate,sample_aspect_ratio,display_aspect_ratio,channels,channel_layout,sample_rate,bit_rate:stream_tags=language,title:stream_disposition=attached_pic" \
        -of json "$file" 2>"$stderr"); then
      # The document goes through stdin, never into the jq program: interpolating
      # it would paste its quotes straight into the filter and silently print
      # nothing.
      printf "%s" "$json" | jq -c --arg path "$relative" ". + {path: \$path, ok: true}"
    else
      error=$(head -1 "$stderr" 2>/dev/null | tr -d "\r")
      jq -cn --arg path "$relative" --arg error "$error" \
        "{path: \$path, ok: false, error: \$error}"
    fi >>"$PROBE_WORK/$$.jsonl"
    rm -f "$stderr"
  ' "$root"

cat "$work"/*.jsonl 2>/dev/null | jq -c . | sort >"$out"
echo "probed $(wc -l <"$out" | tr -d ' ') file(s) into $out" >&2
