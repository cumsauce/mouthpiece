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
2. **Vocals transcribe themselves.** On first use, the Words panel offers a speech model to download.
   *Base* suits speech. *Large Turbo* is the one to pick for singing.
3. **Find words** with `⌘F`. Press Return to place a word at the playhead and move on to the next one.
4. **Or type a whole sentence** with `⌘B`. The Sentence Builder prefers runs of words as originally
   said, lets you cycle takes, and stitches words nobody said from phonemes.
5. **Shape it:** split, reverse, stretch, repitch, fade, and snap words into the song's key.
6. **Render** with `⌘R`, to MP3 or WAV.

| Search for | To find |
|---|---|
| `hello` | Every take, then sound-alikes, then near matches |
| `hello world` | A phrase, as it was said |
| `/k ae t/` | A sound anywhere, even inside other words |
| `~cat` | Words that rhyme |

`⌘K` lists every command, and `?` shows every shortcut. Double-click a track or clip to open it in the
detail editor. Drag clip edges to trim, Alt-drag edges to stretch, and Alt-drag a clip to copy it.

## Design

The full design is in [PLAN.md](PLAN.md). In short:

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
