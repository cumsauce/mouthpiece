# Mouthpiece: a sentence-mixing DAW in Rust

Mouthpiece is a local, real-time DAW built around one job: **take a song's stems, leave the
instrumentals alone, and cut the vocal into a new sentence.** The arrangement and device workflow
is modelled on Bitwig. What a general DAW doesn't have is that every word, and every phoneme, of
the vocal can be searched and placed like a sample.

## The core loop

1. **Drag stems in.** Tracks are created, named, coloured and typed (vocal, drums, bass, other)
   from the filename. BPM, beat grid and key are detected.
2. **Vocals transcribe themselves** on a background worker, with word timestamps refined against
   the audio and phonemes aligned inside each word.
3. **Search.** `hello` finds every take of a word. `/k ae t/` finds a sound anywhere, even inside
   other words. Results audition on click.
4. **Build.** Drag a word to the timeline, or type a whole sentence into the Sentence Builder.
   It picks a take for each word, lets you cycle alternates, and stitches missing words from phonemes.
5. **Shape.** Slice, reverse, stretch, repitch, fade and pitch-snap to key, non-destructively.
6. **Render** to MP3 or WAV through exactly the same engine code used for playback.

## Architecture

```
crates/
  mp-model     Project document, edit operations, undo. No audio or UI dependencies.
  mp-dsp       Pure DSP: filters, dynamics, delay, reverb, resampling, stretch, YIN pitch.
  mp-engine    Real-time graph, Device trait (CLAP-shaped), cpal output, offline render.
  mp-analysis  Decode, peaks, spectrogram, BPM/beat/key, Whisper, G2P, phoneme alignment, search.
  mp-export    MP3 (LAME) and WAV encoders.
  mp-app       egui UI: arranger, editor, devices, word browser, sentence builder.
```

Dependencies only point downward: `mp-app -> {engine, analysis, export} -> {dsp, model}`. The
engine can run without a UI, so playback and rendering can be tested headlessly.

### Real-time rules

The audio callback never allocates, locks, or does I/O.

- The UI owns the `Project`. Any change compiles it into an immutable `EngineGraph` snapshot,
  which is sent through a lock-free `rtrb` queue.
- The audio thread swaps the snapshot in at a block boundary and sends the old one back on a second
  queue, so all deallocation happens off the audio thread.
- Devices keep their state across swaps. The audio thread matches them by `DeviceId` and moves the
  live instance into the new graph, so reverb tails and filter memory survive edits.
- Transport position, meters and CPU load are published through atomics.
- There is one mutation path, the snapshot. It is cheap because audio buffers are shared `Arc`s,
  and it can't go out of sync the way separate per-parameter messages can.

### Non-destructive clips

A `Clip` is metadata: source, source range, position, gain, pan, fades, reverse, stretch, pitch,
mode. Source files are never modified.

| Operation | Where it runs | Why |
|---|---|---|
| Gain, pan, fades, mute, reverse | Real time in the voice | Instant response, no cost |
| Stretch and pitch | Worker thread into a render cache keyed by parameter hash | Best quality, deterministic, identical in playback and export |

While a render is pending, the clip plays its unprocessed audio and is hatched in the UI. Export
waits for all pending renders, so **playback and output use the same code path**.

### CLAP readiness

`Device` deliberately mirrors the CLAP plugin model:

- `activate(sample_rate, max_block)` and `reset()`
- Parameters with stable ids, described by `ParamInfo`
- `process(ctx, buffers)` with transport info
- `latency()`, and state saved as parameter values

`DeviceKind` is an open enum in the project file with a reserved `Clap { path, plugin_id, state }`
variant. Adding hosting means writing a `ClapDevice: Device` wrapper over `clack-host`, plus a
scanner and a window for plugin GUIs. Nothing in the graph, UI or project format has to change.

### Time model

Clip positions are stored in **seconds**, not beats. Stems must never drift when the BPM changes.
The beat grid is a view (`bpm`, `grid_offset`) used for snapping and quantising. Clips can opt in
to musical time with "Fit to tempo", which sets their stretch ratio from their source BPM.

## Analysis pipeline

| Step | Method |
|---|---|
| Decode | `symphonia` (MP3, WAV, FLAC, AAC/M4A, OGG, AIFF), resampled to the engine rate with a built-in windowed-sinc resampler |
| Waveforms | Min/max pyramid with a per-bin brightness value for tinting |
| BPM and beats | Onset-strength envelope, then a tempogram with a log-normal tempo prior, then dynamic-programming beat tracking for the grid phase |
| Key | Chroma against Krumhansl-Schmuckler profiles |
| Words | `whisper.cpp` through `whisper-rs` recognises the text, with DTW attention timestamps as rough anchors. Its default token timestamps proved too inaccurate to cut audio with |
| Word and phoneme timing | Phrase-level forced alignment: CMUdict G2P, then one dynamic-programming search over a phrase's phonemes with broad-class acoustic models, duration priors, optional silence between words, and the DTW anchors as priors |
| Pitch | YIN per word, shown as a note name and used for pitch-snap |

**Known limit:** the aligner's acoustic models are hand-built, so boundaries between similar sounds,
such as a vowel into a nasal, are approximate. In the end-to-end test, words cut this way are
recognised correctly when transcribed again. It has been tested on clear speech but not yet on sung vocals. A later upgrade path is a CTC acoustic model through ONNX
Runtime. Whisper is also less accurate on singing than speech, so the model picker offers larger
models. Whisper models download only when the user clicks to do so.

## UX

Dark, dense, keyboard-first, with Bitwig's panel logic.

```
+-------------------------------------------------------------------------+
| Transport   BPM   Key   Grid   Tools   Meters                  Export   |
+-----------+-------------------------------------------+-----------------+
| Inspector | Ruler / loop                              | Words           |
| (context- | Track headers | Clip lanes with waveforms | Sentence Builder|
| ual)      |                                           | Files           |
+-----------+-------------------------------------------+-----------------+
| Editor panel: [Detail editor] [Devices] [Mixer]                         |
| words lane, phoneme lane, spectrogram, zoomed waveform                  |
+-------------------------------------------------------------------------+
```

- **Double-click** a clip or track to open it in the detail editor, as in Bitwig.
- **Command palette** (`Cmd+K`) lists every action with its shortcut.
- **Direct manipulation:** clip edges trim, `Alt`+edge stretches, corner handles set fades. Snap
  lines appear while dragging, and a ghost shows where a dragged word will land.
- **Audition everywhere:** clicking a word or result plays it through the audition bus immediately.
- **Everything is undoable**, with autosave. The status bar and menu name the next undo.
- **Never blocks:** decoding, analysis and rendering show progress in the status bar while the app
  stays usable.

### Features added beyond the brief

- **Sentence Builder** to go from typed text to a placed sentence.
- **Pitch-snap to key** so spliced words sit in tune with the track.
- **Slice to words** turns a vocal clip into one clip per word in a single action.
- **Close gaps / ripple** to tighten a spliced sentence.
- **Anti-click splice fades** on every cut, plus a minimum edge fade in the engine.
- **Master limiter** for safe, loud renders.

## Milestones

1. **Foundation:** workspace, model and undo, DSP, engine, offline render, tests.
2. **Analysis:** decode, peaks, BPM, key, pitch, G2P, alignment, Whisper.
3. **UI:** shell, theme, arranger, clip editing, transport, inspector, devices, mixer.
4. **Sentence mixing:** word browser, phoneme search, detail editor, Sentence Builder.
5. **Finish:** save/load, autosave, MP3/WAV render, command palette, polish.
6. **Verify:** unit tests, end-to-end test (speech to transcription to sentence to MP3), and visual checks of the running app.

## Later

CLAP hosting through `clack-host`, volume and parameter automation lanes, sends and group tracks,
audio input recording, disk streaming for very long sources, a neural phoneme aligner, and melody
drawing to sing a sentence along a pitch contour.
