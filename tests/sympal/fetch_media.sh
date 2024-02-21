#! /usr/bin/env bash

fp="./$0"
media="${fp%/*}/media"
mkdir $media &> /dev/null

set -e

### "Base" mp4
base=${media}/2a1v_48k_aac264_mp4.mp4
yt-dlp 'https://youtu.be/7nQ2oiVqKHw' -f 18 -o - | ffmpeg -f mp4 -i - -c:a aac -af "volume=-12dB" -b:a 256k -ar 48000 -ac 2 -c:v h264 -crf 25 $base -y


### "Normal" Audio
ffmpeg -i $base -vn -c:a mp3 $media/2a_mp3_mp3.mp3 -y
ffmpeg -i $base -vn -c:a flac $media/2a_flac_flac.flac -y
ffmpeg -i $base -vn -c:a aac $media/2a_aac_m4a.m4a -y
ffmpeg -i $base -vn -c:a libopus $media/2a_opus_ogg.ogg -y
ffmpeg -i $base -vn -c:a libvorbis $media/2a_vorbis_ogg.ogg -y
ffmpeg -i $base -vn -c:a pcm_f32le $media/2a_pcmf32_wav.wav -y
ffmpeg -i $base -vn -c:a wavpack $media/2a_wavpack_mkv.mkv -y


### Different Audio
#
# High hertz flac
ffmpeg -i $base -vn -c:a flac -ar 192000 $media/2a_192k_flac_flac.flac -y
#
# Half hertz flac
ffmpeg -i $base -vn -c:a flac -ar 24000 $media/2a_24k_flac_flac.flac -y
#
# Goofy hertz flac
ffmpeg -i $base -vn -c:a flac -ar 45678 $media/2a_45678Hz_flac_flac.flac -y
#
# Mono mp3
ffmpeg -i $base -vn -c:a mp3 -ac 1 -q:a 8 $media/1a_mp3_mp3.mp3 -y
#
# Quad AAC
ffmpeg -i $base -vn -c:a aac -af 'pan=4c|c0=c0|c1=c1|c2=c0|c3=c1' -q:a 8 $media/4a_aac_m4a.m4a -y


### Matroska w/different streams
ffmpeg -i $base -c:v vp9 -c:a mp3 -b:a 320k -b:v 1M $media/2a1v_2a1v_vp9mp3_mkv.mkv -y


set +e
