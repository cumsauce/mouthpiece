# Third-party software

Mouthpiece's own code is MIT licensed (see `LICENSE`). Release builds also contain the following.

## LGPL: the MP3 encoder

Release builds statically link [LAME](https://lame.sourceforge.io/) 3.100 (LGPL, see `LICENSE-LAME`)
through the `mp3lame-sys` and `mp3lame-encoder` crates (LGPL-3.0). You may replace them: all of
Mouthpiece's source and its build scripts are in this repository, so you can rebuild the app against a
modified LAME by changing that dependency in `crates/mp-export/Cargo.toml` and running
`scripts/package.sh`. LAME's source is included in the `mp3lame-sys` crate on crates.io.

## MPL-2.0

The audio decoders come from [Symphonia](https://github.com/pdeljoo/Symphonia), unmodified. Its source
is available at that address.

## Permissive licences

- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (MIT) and `whisper-rs` (Unlicense)
- [Signalsmith Stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch) (MIT)
- [CMUdict](http://www.speech.cs.cmu.edu/cgi-bin/cmudict) (BSD, see `LICENSE-CMUDICT`)
- `egui`, `eframe`, `cpal`, `rfd` and other Rust crates (MIT and/or Apache-2.0). `Cargo.lock` lists
  every dependency, and each crate's licence is on crates.io.

Speech models are downloaded separately, and only when you choose to, from
[Hugging Face](https://huggingface.co/ggerganov/whisper.cpp). They aren't part of this software.
