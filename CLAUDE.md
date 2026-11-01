# Mouthpiece

A local, real-time DAW for sentence mixing, in Rust, with an egui interface. See [README.md](README.md).

## Before changing the interface

Read [docs/WORKFLOWS.md](docs/WORKFLOWS.md) (what it's for) and [docs/DESIGN.md](docs/DESIGN.md) (how it's
built). Hard rules, which `cargo test` mostly enforces:

- Panels in `crates/mp-app/src/panels/` use only `crate::ds`. No raw egui widgets, colours or font sizes.
- `ACCENT` means the playhead, the selection, or the one primary action in view. Nothing else.
- Every command is a row in `ACTIONS` in `actions.rs`. Buttons call `self.run(Action::X)`.
- Every edit is undoable: `self.edit(...)`, or `self.begin_gesture(...)` before a continuous change.
- Slow work goes off the UI thread, and the audio thread must never allocate, lock or block.

## Checking your work

```bash
cargo test --workspace
```

Don't call a UI change finished until you've looked at it. Drive the real app with `MOUTHPIECE_SCRIPT`,
take a screenshot, and go through the checklist at the end of DESIGN.md. The steps are at the top of
`crates/mp-app/src/script.rs`.

The test below is the full product loop, and has to keep passing whenever transcription, alignment,
the engine or export change. It needs the Base speech model and macOS `say`.

```bash
cargo test -p mp-export --test sentence_e2e -- --ignored --nocapture
```

## Layout

- `mp-model`: the project and its edits. No audio, no UI.
- `mp-dsp`: signal processing.
- `mp-engine`: the real-time graph and offline render, which share one code path.
- `mp-analysis`: decoding, tempo, key, Whisper, alignment, search.
- `mp-export`: MP3 and WAV.
- `mp-app`: the interface. `ds/` is the design system, `panels/` the screens, `actions.rs` the commands.
