#!/usr/bin/env python3
"""Mouthpiece's stem separation worker. Two stages, using audio-separator:

  1. A RoFormer vocal model splits the song into vocals and instrumental.
  2. htdemucs_ft splits that instrumental into drums, bass and other. Its vocal output is discarded,
     because stage 1's is cleaner.

Jobs arrive as JSON lines on stdin, and events leave as JSON lines on stdout. It runs until stdin
closes. Models load once and stay loaded between jobs. See crates/mp-analysis/src/stems.rs.
"""
import json
import logging
import os
import shutil
import subprocess
import sys
import tempfile
import traceback

# Libraries print freely, so the real stdout is kept for events and everything else goes to the log.
EVENTS = os.fdopen(os.dup(1), "w", encoding="utf-8")
os.dup2(2, 1)
sys.stdout = sys.stderr

log = logging.getLogger("mouthpiece.stems")

VOCAL_NAMES = {"Vocals": "Vocals", "Instrumental": "Instrumental"}
INSTRUMENT_NAMES = {"Vocals": "Discard", "Drums": "Drums", "Bass": "Bass", "Other": "Other"}


def emit(event, **fields):
    EVENTS.write(json.dumps({"event": event, **fields}) + "\n")
    EVENTS.flush()


def detect_device():
    """CUDA, then Apple's MPS, then the CPU. audio-separator makes the same choice by itself."""
    try:
        import torch
    except ImportError:
        return "cpu"
    if torch.cuda.is_available():
        return "cuda"
    if torch.backends.mps.is_available():
        return "mps"
    return "cpu"


class StageError(Exception):
    def __init__(self, kind, message):
        super().__init__(message)
        self.kind = kind


def classify(error, stage):
    text = f"{type(error).__name__}: {error}"
    lower = text.lower()
    if isinstance(error, ImportError):
        return StageError("broken_install", text)
    if isinstance(error, MemoryError) or "out of memory" in lower or "mps backend" in lower:
        return StageError("out_of_memory", text)
    if stage.startswith("load"):
        return StageError("missing_model", text)
    if "invalidaudiodata" in lower or "decod" in lower or "format not recognised" in lower or "could not open" in lower:
        return StageError("bad_input", text)
    return StageError("other", text)


class Pipeline:
    def __init__(self, model_dir):
        # audio-separator won't create this itself, and fails on the first download without it.
        os.makedirs(model_dir, exist_ok=True)
        self.model_dir = model_dir
        self.separators = {}
        self.work = tempfile.mkdtemp(prefix="mouthpiece-stems-")

    def close(self):
        shutil.rmtree(self.work, ignore_errors=True)

    def separator(self, model, low_memory):
        """Loads a model the first time it's asked for, and reuses it after that."""
        key = (model, low_memory)
        if key in self.separators:
            return self.separators[key]
        from audio_separator.separator import Separator

        # A low-memory retry needs a differently set-up model, so the earlier one goes first.
        self.separators.clear()
        out = tempfile.mkdtemp(dir=self.work)
        mdxc = {"segment_size": 128 if low_memory else 256, "override_model_segment_size": low_memory, "batch_size": 1 if low_memory else None, "overlap": None, "pitch_shift": 0}
        demucs = {"segment_size": 10 if low_memory else "Default", "shifts": 1 if low_memory else 2, "overlap": 0.25, "segments_enabled": True}
        # The default of 0.9 turns each stage and stem down by itself, and the instruments twice. Stems then
        # no longer add up to the song, and the vocal comes out too loud for them. 1.0 only prevents clipping.
        separator = Separator(log_level=logging.INFO, model_file_dir=self.model_dir, output_dir=out, output_format="WAV", normalization_threshold=1.0, mdxc_params=mdxc, demucs_params=demucs)
        separator.load_model(model_filename=model)
        self.separators[key] = (separator, out)
        return self.separators[key]

    def split(self, model, low_memory, path, names, load_stage, run_stage):
        emit("stage", stage=load_stage)
        try:
            separator, out = self.separator(model, low_memory)
        except Exception as error:
            traceback.print_exc()
            raise classify(error, "load") from error
        emit("stage", stage=run_stage)
        try:
            produced = separator.separate(path, names)
        except Exception as error:
            traceback.print_exc()
            raise classify(error, run_stage) from error
        # Separator may return paths relative to its output directory, which join leaves alone if absolute.
        return [os.path.join(out, p) for p in produced]

    def run(self, job):
        source = job["input"]
        if not os.path.isfile(source) or os.path.getsize(source) == 0:
            raise StageError("bad_input", f"{source} is missing or empty")
        low_memory = bool(job.get("low_memory"))
        stems = {}
        with tempfile.TemporaryDirectory(dir=self.work) as scratch:
            # Each file gets a plain name, and moves out of the separator's folder right away so the next job can't see it.
            def collect(paths, wanted):
                for path in paths:
                    name = find_stem(path)
                    if name in wanted:
                        target = os.path.join(scratch, f"{name}.wav")
                        shutil.move(path, target)
                        stems[name] = target
                    elif os.path.exists(path):
                        os.remove(path)

            collect(self.split(job["vocal_model"], low_memory, source, VOCAL_NAMES, "load_vocals", "vocals"), ("Vocals", "Instrumental"))
            if "Instrumental" not in stems or "Vocals" not in stems:
                raise StageError("other", f"The vocal model produced {sorted(stems)}, not Vocals and Instrumental")
            instrumental_copy = os.path.join(scratch, "input_instrumental.wav")
            shutil.copyfile(stems["Instrumental"], instrumental_copy)
            collect(self.split(job["instrumental_model"], low_memory, instrumental_copy, INSTRUMENT_NAMES, "load_instruments", "instruments"), ("Drums", "Bass", "Other"))
            missing = [n for n in ("Drums", "Bass", "Other") if n not in stems]
            if missing:
                raise StageError("other", f"The instrument model didn't produce {missing}")

            emit("stage", stage="saving")
            restore_levels(stems, source, job.get("format", "wav"))
            os.makedirs(job["output_dir"], exist_ok=True)
            return {name: save(path, job["output_dir"], name, job.get("format", "wav"), job.get("ffmpeg")) for name, path in stems.items()}


def restore_levels(stems, source, fmt):
    """Puts stems back at the song's levels. Taking the vocal out of a loud master pushes the instrumental
    over full scale, and the separator turns it down to fit, so the stems wouldn't add up to the song and the
    vocal would be too loud for them. Comparing with the song shows how much each side was turned down."""
    # Measured by comparing with the song, so the worst this can do is leave things as they were.
    try:
        import librosa
        import numpy as np
        import soundfile as sf

        audio = {name: sf.read(path, always_2d=True) for name, path in stems.items()}
        rate = audio["Vocals"][1]
        song, _ = librosa.load(source, sr=rate, mono=True)
        vocals, rest = audio["Vocals"][0].mean(axis=1), audio["Instrumental"][0].mean(axis=1)
        n = min(len(song), len(vocals), len(rest))
        gains, *_ = np.linalg.lstsq(np.stack([vocals[:n], rest[:n]], axis=1), song[:n], rcond=None)
        vocal_gain, rest_gain = [float(np.clip(g, 0.25, 4.0)) for g in gains]
        log.info("Restoring levels: vocals x%.3f, instruments x%.3f", vocal_gain, rest_gain)
        scaled = {name: data * (vocal_gain if name == "Vocals" else rest_gain) for name, (data, _) in audio.items()}
        # Float WAV can go over full scale, and FLAC and MP3 can't, so those come down together, keeping the balance.
        peak = max(float(np.abs(data).max()) for data in scaled.values())
        trim = 1.0 if fmt == "wav" or peak <= 1.0 else 1.0 / peak
        for name, data in scaled.items():
            sf.write(stems[name], data * trim, rate, subtype="FLOAT")
    except Exception:
        log.warning("Couldn't restore levels, so the stems stay as separated", exc_info=True)


def find_stem(path):
    """Finds which stem a file is, from a custom name or from audio-separator's "(Stem)" naming."""
    base = os.path.basename(path).lower()
    for name in ("Discard", "Vocals", "Instrumental", "Drums", "Bass", "Other"):
        if os.path.splitext(base)[0] == name.lower() or f"({name.lower()})" in base:
            return name
    return None


def save(path, folder, name, fmt, ffmpeg):
    """Puts a finished stem in place in one step, so a cancelled job can't leave half a file."""
    target = os.path.join(folder, f"{name}.{fmt}")
    if fmt == "wav":
        os.replace(path, target) if same_volume(path, folder) else shutil.move(path, target)
        return target
    partial = os.path.join(folder, f".{name}.partial.{fmt}")
    subprocess.run([ffmpeg or "ffmpeg", "-v", "error", "-y", "-i", path, partial], check=True)
    os.replace(partial, target)
    return target


def same_volume(a, folder):
    return os.stat(a).st_dev == os.stat(folder).st_dev


def main():
    logging.basicConfig(level=logging.INFO, stream=sys.stderr)
    device = detect_device()
    log.info("Separating on: %s", device)
    emit("ready", device=device)
    pipeline = None
    try:
        for line in sys.stdin:
            if not line.strip():
                continue
            job = json.loads(line)
            if pipeline is None:
                pipeline = Pipeline(job["model_dir"])
            try:
                emit("done", files=pipeline.run(job))
            except StageError as error:
                emit("error", kind=error.kind, message=str(error))
            except Exception as error:
                traceback.print_exc()
                emit("error", kind=classify(error, "run").kind, message=f"{type(error).__name__}: {error}")
    finally:
        if pipeline:
            pipeline.close()


if __name__ == "__main__":
    main()
