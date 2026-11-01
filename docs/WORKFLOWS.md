# Core workflows

What people hire Mouthpiece to do, and what that means for the interface. Interface decisions should
trace back to something here. If a change doesn't help one of these jobs, question the change.

## Who it's for

A sentence mixer: someone who cuts a singer's or speaker's words apart to make them say something
new, over music. They know their way around a DAW, or at least a video editor. Their project is one
song and one joke or idea, and the work is playful and iterative: try a line, laugh or wince, change
one word, try again. The instrumental usually stays untouched.

**The core loop is say, hear, adjust.** It runs hundreds of times per project, so its speed defines
the product. Everything else is setup (before the loop) or finishing (after it).

## Jobs

Each job has a budget. A budget is a test: if the interface misses it, the interface is wrong.

### 1. Get a song ready to cut

> When I have stems, I want to get to typing words as soon as possible, so I can start on the idea
> while I still have it.

- **Done when:** stems are on tracks, tempo and key are right, and the words are searchable.
- **Budget:** one drag, plus one click the first time to get a speech model. No configuration.
- **Needs:** visible progress on the vocal itself, not hidden in a status bar. Tracks sorted and coloured
  by type. Typing is possible as soon as there are words, and it's clear why it isn't before then.
- **Failure to avoid:** a dead end. "Can't transcribe" must be followed by the one button that fixes it.

### 2. Make them say a line (the core job)

> When I have a line in my head, I want to type it and hear it, so I know right away whether it's funny.

- **Done when:** the line is on the timeline, playing.
- **Budget:** the line's typing, Return to place it, Space to hear it. No mouse. No choosing between a
  "search" and a "sentence" mode. Afterwards the input is ready for the next line.
- **Needs:** one input for everything. A live plan showing what will happen for each word: found, stitched
  from sounds, or impossible. Audio preview without placing anything.
- **Failure to avoid:** making people place words one at a time. Also, finding out only after placing
  that a word doesn't exist.

### 3. Find the take that works

> When a word sounds wrong, I want to hear other takes of it quickly, so I can find one that fits.

- **Done when:** the right take is chosen, before or after placing.
- **Budget:** one key per take, heard instantly.
- **Needs:** every take available in place, with the words around it for context, and its pitch. Swapping a
  take on a clip **already on the timeline** keeps its position, because that's when you actually notice the
  problem. A good first guess: pick takes that are close in pitch to their neighbours.
- **Failure to avoid:** delete, search, place, and re-align just to try a different take.

### 4. Make it sit in the song

> When a line is placed, I want it to feel like part of the track, so it sounds like they really sang it.

- **Done when:** the words are in time, in key and at the right level.
- **Budget:** one key per operation on any selection: quantize, tune, tighten, nudge.
- **Needs:** these operations right next to the selection, without a trip to a panel. The next note and take
  visible on the clip. Hearing the result against the music, and alone, with a single key.

### 5. Fix a splice

> When a cut clicks or clips a consonant, I want to adjust that one edge, so the word sounds natural.

- **Done when:** the word starts and ends cleanly.
- **Budget:** double-click to open, one drag to fix.
- **Needs:** the waveform, spectrogram, words and phonemes lined up, all at a zoom where edges can be seen.
  Trims and fades that work the same here as in the arrangement.

### 6. Mangle it on purpose

> When I want a joke or an effect, I want to reverse, stretch, stutter and repitch freely, so I can try ideas
> without commitment.

- **Done when:** it sounds ridiculous in the right way.
- **Budget:** one key each. Everything non-destructive, and everything undoable.
- **Needs:** instant audition, a stutter in one key, and no fear of breaking anything.

### 7. Make a word that doesn't exist, and keep it

> When a word isn't in the recordings, I want to build it from pieces of others and blend them, so that it
> sounds like they said it. When I need it again later, I want to find it, not rebuild it.

- **Done when:** the word sits on the beat, sounds right, and can be found by typing its name.
- **Budget:** typing the word gets a first attempt straight away, already grouped and blended. One key to
  crossfade, one drag to adjust a piece, one key and a name to keep it. Typing its name uses it.
- **Needs:**
  - **Groups.** The pieces move and quantize as one word. Clicking again selects one piece to adjust it.
  - **Crossfades you can hear while dragging.** Overlapping two clips fades one into the other for as long as
    they overlap, and pulling them apart undoes it.
  - **A library.** Saved words come first in Say and are listed in My words.
  - **A way back, always.** Recordings are never changed. Saved words are never changed by editing a placed
    copy. "Revert to saved" and "Restore original" go back from any experiment.
- **Failure to avoid:** being afraid to try something, or rebuilding a word for the second chorus.

### 8. Cut the video too

> When my source is a video, I want the picture to follow what I cut, so that I get a video edit without
> doing the work twice.

- **Done when:** an MP4 plays the sentence mix with the matching picture.
- **Budget:** nothing extra. Drop in a video instead of a stem, and sentence mix as usual. Choose MP4 in Render.
- **Needs:** each clip shows its own part of its source, including when reversed or stretched. The topmost
  audible clip wins, and short gaps hold the last frame. A preview that follows the playhead and takes being
  auditioned, so you can choose takes by mouth shape.
- **Failure to avoid:** a separate video timeline to keep in sync. The clips already know their timecodes.
- **Requires:** ffmpeg, which isn't bundled. Sound works without it, and the app says how to get it.

### 9. Finish and share

> When it's good, I want an MP3, so I can post it.

- **Done when:** the file exists and I can find it.
- **Budget:** two clicks with sensible defaults: Render, then choose where.
- **Needs:** a clear statement of what's being rendered. A way to get to the file afterwards.

## What this changed in the interface

| Finding | Decision |
|---|---|
| Search and Sentence Builder are the same job split into two tabs | One **Say** input. Type a word, a line, `/a sound/` or `~a rhyme`. Enter places it, Shift+Enter previews |
| Trying a take after placing meant starting over | **Swap takes on placed clips** with `T`, keeping their position |
| Common operations meant finding a slider in a panel | A **selection bar** by the selection, with take, pitch, speed, tune, repeat and split |
| The transcript was only visible zoomed in, one word at a time | A readable **Transcript** tab: read the lyrics, click to hear, double-click to add to the line |
| Status was split across toasts and a small status bar | **Say** shows what's happening and the next step: no vocal, needs a model, listening, or ready |
| Everything competed for attention, with orange on every toggle | **Accent is reserved** for the playhead, selection and the one primary action in view |
| Sliders and labels varied from panel to panel | A **design system** of tokens and components, with a test that enforces it. See [DESIGN.md](DESIGN.md) |
| Placed words could land off-screen, with no sign anything happened | The view **follows the placement**, and new clips flash |
| Words that don't exist had to be rebuilt each time | Stitched words arrive **grouped and blended**, and ⇧W **saves** them to find by typing |
| Overlapping clips simply summed | Overlaps **crossfade** automatically, marked with an X |
| Experiments felt risky | **Restore original** and **Revert to saved**, on top of undo |
| Hearing the line alone meant finding the track and clicking S | **Solo the selection's track** with one key, and play from the selection with another |
| After placing a line, focus stayed in the input, where Space types a space | In an empty input, **Space plays the line** just placed and stopping returns to where the next one goes |

## Keyboard map for the core loop

| Key | In the Say input | On the timeline |
|---|---|---|
| Return | Place the line at the playhead | Stop and return |
| Shift+Return | Preview the line | |
| Tab / Shift+Tab | Next or previous word | |
| ↑ ↓ | Previous or next take of that word | Pitch up or down |
| Esc | Clear the input | Deselect |
| T / Shift+T | | Next or previous take of the clip |
| P | | Tune to key |
| Q | | Quantize |
| G | | Close gaps |
| Shift+D | | Repeat (stutter) |
| X | | Crossfade the selection together |
| Shift+W | | Save the selection as a word |
| Shift+R | | Restore to how it was first placed |
| ⌘G / Shift+⌘G | | Group or ungroup |
| Alt+← → | | Nudge by 5 ms |
| R | | Reverse |
| Space | With the input empty, hear the line just placed. Again to return | Play or pause |
| Shift+Space | | Play from the selection |
| Shift+S | | Solo the selection's track |
| ⌘F or / | Go to the Say input | |
