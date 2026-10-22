# Audio fixtures

Tiny test files for the sound importer (`sound_to_wav` in `src/editor/sound.rs`). Each one is a generated tone: 0.3 seconds, mono, 22 050 Hz, a 440 Hz sine wave. There is no recorded or third-party audio in them, so they carry no licence terms of their own.

| File | Format | Encoder |
|---|---|---|
| `tone.mp3` | MP3 (MPEG-2 layer 3) | libmp3lame, 32 kbit/s |
| `tone.ogg` | Ogg Vorbis | libvorbis, quality 0 |
| `tone.flac` | FLAC | FFmpeg's flac encoder |
| `tone.m4a` | AAC-LC in an MP4 container | FFmpeg's native aac encoder, 32 kbit/s |
| `tone.aac` | AAC-LC in ADTS frames | FFmpeg's native aac encoder, 32 kbit/s |

They were made with FFmpeg 9.0.2 as an offline tool (FFmpeg is never shipped or linked; see `docs/legal/video-export.md`), with `SRC` set to `sine=frequency=440:sample_rate=22050:duration=0.3`:

```sh
ffmpeg -f lavfi -i "$SRC" -ac 1 -c:a libmp3lame -b:a 32k tone.mp3
ffmpeg -f lavfi -i "$SRC" -ac 1 -c:a libvorbis -q:a 0 tone.ogg
ffmpeg -f lavfi -i "$SRC" -ac 1 -c:a flac tone.flac
ffmpeg -f lavfi -i "$SRC" -ac 1 -c:a aac -b:a 32k tone.m4a
ffmpeg -f lavfi -i "$SRC" -ac 1 -c:a aac -b:a 32k -f adts tone.aac
```

The tests check the decoded sample rate, channel count, length (lossy encoders add padding, so within 0.12 s; FLAC exactly 6 615 samples), that the sound is not silent, and that it crosses zero at about 880 times a second. Keep the files small; do not replace them with longer or recorded audio.
