# Mouthpiece

A local, real-time DAW for sentence mixing, written in Rust. Drop in a song's stems, then make the
vocal say something new: every word, and every sound inside a word, can be searched and placed like
a sample. The arrangement and device workflow is modelled on Bitwig.

## Run a release

Every push to `main` updates the **latest-main** pre-release on the repository's Releases page, and
pushing a tag such as `v1.0.0` makes a permanent release.

**macOS (Apple Silicon, macOS 13 or newer).** Unzip it and move `Mouthpiece.app` wherever you like.
The app isn't notarized, so macOS blocks a copy downloaded with a browser. Clear that once:

```bash
xattr -dr com.apple.quarantine Mouthpiece.app
```

**Linux (x86_64, glibc 2.35 or newer, a CPU with AVX2).** Unpack it and run it:

```bash
tar xzf Mouthpiece-*-linux-x86_64.tar.gz
```

```bash
./mouthpiece/mouthpiece
```

It needs ALSA (`libasound2`, or PipeWire's or PulseAudio's ALSA layer) and a Vulkan or OpenGL driver.
File dialogs use the desktop portal, or `zenity` if there isn't one. On Wayland, dragging files in
from a file manager may not work, so use **Choose files…** or start it under X11 with `env -u WAYLAND_DISPLAY ./mouthpiece/mouthpiece`.
Where this README says `⌘`, use Ctrl on Linux.

**Windows (x86_64, a CPU with AVX2).** Unzip it and run `mouthpiece.exe`. It isn't signed, so SmartScreen may ask
you to confirm the first time. Where this README says `⌘`, use Ctrl.

## Splitting a song into stems

Drop in a whole song and Mouthpiece offers to split it into vocals, drums, bass and other. It uses
[audio-separator](https://github.com/nomadkaraoke/python-audio-separator): a RoFormer vocal model first, then `htdemucs_ft`
on what's left. This is Python, so the first split sets up a private environment and downloads several gigabytes.
Stems go in a `<song> stems` folder beside the song, along with `Instrumental`. Preferences are in `settings.json` in the app's
data folder, including the model names.

## Tools it fetches for itself

Nothing needs installing by hand. On first need, these go into a `tools` folder in the app's data folder:

| Tool | For | From |
|---|---|---|
| ffmpeg, ffprobe | Video, and downloads | [martin-riedl.de](https://ffmpeg.martin-riedl.de) on macOS and Linux, [yt-dlp/FFmpeg-Builds](https://github.com/yt-dlp/FFmpeg-Builds) on Windows |
| yt-dlp | Find a song online | [yt-dlp releases](https://github.com/yt-dlp/yt-dlp/releases) |
| Deno | yt-dlp needs a JavaScript runtime for YouTube | [Deno releases](https://github.com/denoland/deno/releases) |
| uv | Sets up Python for stem splitting | [uv releases](https://github.com/astral-sh/uv/releases) |

yt-dlp updates separately from the app, to its nightly build: at most daily when it's used, again if a search or download fails,
and on demand with **Update yt-dlp now** in the command palette. An ffmpeg or uv already on your system is used instead of downloading one.

## Build from source

Needs Rust and CMake (`brew install cmake`). The first build compiles whisper.cpp and LAME, which takes a few minutes. On Linux, the packages to install are listed in
[release.yml](.github/workflows/release.yml).

```bash
cargo run --release -p mp-app
```

You can also pass audio files or a `.mouthpiece` project on the command line. `scripts/package.sh`
makes the same packages that CI does, in `dist/`.

## The workflow

1. **Drop your stems in.** Tracks are named, coloured and ordered by type. BPM, beat grid and key are detected.
2. **Vocals transcribe themselves.** The first time, the Say panel offers a speech model to download.
   *Base* suits speech. *Large Turbo* is the one to pick for singing.
3. **Type what they should say,** then press Return. Space plays it back with the music, and you can
   carry on typing the next line.
4. **Try other takes** with `T`, right on the timeline. Each word stays where it is.
5. **Shape it:** split, reverse, stretch, repitch, stutter, and tune words to the song's key. The bar that
   appears over a selection has the common ones.
6. **Add picture effects,** if your source is a video. Devices has a Sound and a Picture chain. Any knob can
   have keyframes, pulse on the beat, or follow a track's level, and effects can be limited to an area.
7. **Render** with `⇧⌘R`, to MP3, WAV or MP4.

| Type | To get |
|---|---|
| `hello` | Every take of a word |
| `hello world` | A whole line, using runs of words as they were said, and stitching ones nobody said from sounds |
| `/k ae t/` | A sound anywhere, even inside other words |
| `~cat` | Words that rhyme |

Press `/` to start typing from anywhere. In the input, Tab moves between words, ↑ and ↓ choose takes,
and Shift+Return previews the line. The Transcript tab shows the lyrics: click a word to hear it, or double-click
to add it to your line.

### Making words that don't exist

Type a word that was never said and it's stitched from sounds, as a group of clips that are already blended. Click
a piece of the group to adjust it, or drag a clip's name strip in the editor. Overlapping clips crossfade for as
long as they overlap, and `X` blends any selection. When it sounds right, press `⇧W` to name it. After that, typing
its name uses it, and it's listed under My words.

Nothing you do changes your recordings, and editing a placed copy doesn't change the saved word. Revert to saved
and Restore original (`⇧R`) go back from any experiment.

### Finding the sound a word needs

Click a sound in the editor's phoneme lane, or select a piece of a word, and press `F`. Every match appears, ranked
by what will join best. Press ↓ to try each one: it's swapped into the word and plays in place, with the music.
`A` flips back to what was there and `P` pins a favourite. Return keeps it, and Esc puts everything back.

### Changing a few words in a song

In Lyrics, click the first word and Shift-click the last, then press `C` and retype them. Only the words that really
changed get new audio, fitted to the original timing. It stays on the vocal's own track, with the original
underneath. `A` compares the two, even during playback. Press `C` again for another version. The Changes tab lists
everything you've changed, and `⇧A` plays the whole original song.

### Video

Drop in a video instead of a stem and work as usual. Each clip shows its own part of the picture, so a sentence mix
cuts the video by itself, and the preview follows the playhead and takes you audition. Render it as MP4. Video needs
[ffmpeg](https://ffmpeg.org) on your system (`brew install ffmpeg`). Sound works without it.

### Everything else

`⌘K` lists every command, and `?` shows every shortcut. Double-click a track or clip to open it in the
editor. Drag clip edges to trim, Alt-drag edges to stretch, and Alt-drag a clip to copy it.

## Design

- [docs/WORKFLOWS.md](docs/WORKFLOWS.md): what the app is for, and the jobs it has to do well.
- [docs/DESIGN.md](docs/DESIGN.md): the design system, its rules, and how to build with it.
- [PLAN.md](PLAN.md): the original architecture.

The code is organised like this:

- `mp-model`: the project document and edits. No audio, no UI.
- `mp-dsp`: filters, dynamics, reverb, resampling, stretching, pitch detection.
- `mp-engine`: the real-time graph and offline render, which share one code path. The audio thread
  never allocates or locks, and a test proves it. `Device` mirrors CLAP's plugin model, and the project format
  already has a `Clap` device kind, so hosting needs a wrapper rather than a redesign.
- `mp-analysis`: decoding, tempo, key, Whisper, forced alignment, search and the sentence planner.
- `mp-export`: MP3 and WAV.
- `mp-app`: the egui interface.

## Tests

```bash
cargo test --workspace
```

The test below is the full loop: it transcribes speech, builds new sentences, renders them to MP3 and
transcribes those MP3s to check what they say. It needs macOS `say` and the Base model installed.

```bash
cargo test -p mp-export --test sentence_e2e -- --ignored --nocapture
```

`MOUTHPIECE_SCRIPT=<file>` drives the running app with real input events and can take screenshots. The steps are listed at the top of
[script.rs](crates/mp-app/src/script.rs).

## Building on a VM shared folder

The linker can't memory-map its output on some shared folders. If you see `memory map must have a
non-zero length`, point `build.target-dir` in `.cargo/config.toml` at a local disk.

## Licences

Mouthpiece's code is MIT licensed. Release builds also include LGPL and other third-party components,
listed in [THIRD_PARTY.md](THIRD_PARTY.md).
