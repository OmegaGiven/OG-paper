#!/usr/bin/env bash
# Cut a recorded preview for the App Store: exact size (WxH), at most 29.5 s
# (sped up evenly if longer), 30 fps H.264 at about 8 Mb/s (Apple's
# processing refuses some very low-bitrate files) in limited-range BT.709,
# with a silent stereo AAC track.
#   scripts/ad/store-cut.sh IN.mp4 OUT.mp4 886x1920
set -euo pipefail
IN=$1 OUT=$2 SIZE=$3
W=${SIZE%x*} H=${SIZE#*x}
D=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$IN")
SPEED=$(python3 -c "d=$D; print(max(1.0, d / 29.5))")
ffmpeg -y -loglevel error -i "$IN" -f lavfi -i anullsrc=channel_layout=stereo:sample_rate=44100 \
  -filter_complex "[0:v]setpts=PTS/${SPEED},fps=30,scale=${W}:${H}:force_original_aspect_ratio=decrease,pad=${W}:${H}:(ow-iw)/2:(oh-ih)/2:color=0xf5f3ec,scale=out_range=tv,format=yuv420p[v]" \
  -map "[v]" -map 1:a -shortest -c:v libx264 -profile:v high -level 4.0 -preset slow -b:v 8M -minrate 8M -maxrate 8M -bufsize 8M -x264-params nal-hrd=cbr -color_range tv -colorspace bt709 -color_primaries bt709 -color_trc bt709 -c:a aac -b:a 128k -movflags +faststart "$OUT"
ffprobe -v error -show_entries format=duration:stream=width,height,codec_name -of compact "$OUT"
