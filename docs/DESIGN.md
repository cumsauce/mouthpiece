# Mouthpiece design system

The rules, tokens and components for Mouthpiece's interface, and how to build with them. It's written
for anyone changing the UI, human or AI. Read [WORKFLOWS.md](WORKFLOWS.md) first: it says what the
interface is for, and this says how it's built.

**The system lives in code,** in [`crates/mp-app/src/ds/`](../crates/mp-app/src/ds/), and a test holds
panels to it. See it all at once:

```bash
MOUTHPIECE_GALLERY=1 cargo run -p mp-app
```

## Principles

When two options seem equal, these break the tie, in this order.

1. **The core loop comes first.** Say, hear, adjust. Anything that adds a step to it needs a very good
   reason. Count the keystrokes: [WORKFLOWS.md](WORKFLOWS.md) sets a budget for each job.
2. **Sound is the feedback.** Selecting a word or take plays it right away. Don't ask people to read
   what they could hear.
3. **Do it where you're looking.** Put an action next to its subject (the selection bar, a word's chip)
   rather than in a distant panel. The inspector is for precision, not for the first attempt.
4. **The keyboard is never second-class.** Every repeated action has a key, and the interface teaches it
   through tooltips.
5. **Nothing is scary.** All edits are non-destructive and undoable, so don't confirm them. Only losing
   unsaved work gets a dialog.
6. **Quiet until needed.** Controls recede until hovered, colour is rare, and the audio is the brightest
   thing on screen. If everything is emphasised, nothing is.
7. **Never a dead end.** An empty, loading or failed state always says what's happening and offers the
   next step.

## How the code is organised

| Path | What's in it | Rule |
|---|---|---|
| `ds/tokens.rs` | Colours, spacing, sizes, radii, motion, fonts | The only place a colour or font size is defined |
| `ds/controls.rs` | Buttons, fields, tabs, menus, switches | Anything that takes input |
| `ds/audio.rs` | Knob, fader, meter | Audio-specific controls |
| `ds/feedback.rs` | Text styles, tooltips, keycaps, progress, modal | Anything that informs |
| `ds/layout.rs` | Frames, sections, rows, empty states | Structure |
| `ds/icons.rs` | The `Icon` set, drawn from strokes | No image files |
| `ds/gallery.rs` | A screen showing every component | Add new components here |
| `panels/*.rs` | The screens | Built only from `ds` |
| `shell.rs` | Panel arrangement and frame housekeeping | Keep it small |
| `mp-model/src/patches.rs` | Changes: alternatives on a track, which hide what's under them | Only the active version's clips are on the track |
| `mp-model/src/video.rs` | The edit list that clips imply | The picture has no timeline of its own |
| `actions.rs` | The `ACTIONS` table: every command | Name, shortcut and group live here |
| `app.rs` | State and operations | No drawing |

`panels/` reads as a map of the window:

```
+---------------------------- topbar.rs ------------------------------+
| inspector.rs |        arranger.rs + lane.rs        |     say.rs      |
|              |        (selection_bar.rs floats)    |                 |
+--------------+-------------------------------------+-----------------+
|              editor.rs  /  devices.rs  /  mixer.rs                   |
+--------------------------- statusbar.rs -----------------------------+
   Over everything: overlays.rs, palette.rs, render.rs
   say.rs hands over to changes.rs while rewriting, and trial.rs while trying sounds
```

## Tokens

Use tokens by name. If you can't find the one you need, question the design before adding one.

### Colour

| Token | Use |
|---|---|
| `BASE` | The canvas behind clips, and text inputs |
| `PANEL` | Panels |
| `RAISED` | Bars, cards, floating surfaces |
| `CONTROL` | A control at rest |
| `CONTROL_HOVER` | A control when hovered, and the selected segment |
| `BORDER` | Outlines on floating surfaces only |
| `DIVIDER` | The gap between panels |
| `TEXT` | Primary text, and values |
| `TEXT_2` | Body text and resting labels |
| `TEXT_3` | Captions, and anything secondary. The lowest contrast allowed for text |
| `ACCENT` / `ACCENT_HOVER` | See below |
| `GOOD` / `WARN` / `BAD` | Status: done or healthy, attention, failed or impossible |
| `SOLO` / `MUTE` | Only for those two states |
| `track_color(i)` | A track's identity, on its clips, header and knobs |

**Accent is rationed.** It means exactly one of three things: the **playhead**, the **selection**, or the
**one primary action in view**. A switched-on toggle isn't accent: use `active(true)`, which is a neutral
lit state. If two primary buttons are visible at once, one of them is wrong. A test limits `ACCENT` to
the files that need it.

**Depth comes from surfaces, not lines.** Things nearer the user are lighter. Use `BORDER` only where a
surface floats over another, because lines between everything add noise.

**Meaning never rests on colour alone.** Pair it with text or shape: a stitched word has a coloured
underline and a sentence saying so.

**How the palette is built.** Colours are chosen in OKLCH, where equal numbers look equal, and tests in
`tokens.rs` hold them to these rules, so change a colour by its OKLCH value and rerun them:

- **Surfaces** share one cool hue, 262, opposite the accent's 41, at a chroma too low to read as a colour.
  Their lightness rises in even steps.
- **Tracks, status, solo and mute are equally vibrant.** They share one OKLCH chroma, `CHROMA`, the most that
  every hue in use can show, and one *apparent* lightness, `APPARENT_LIGHTNESS`. Equal OKLCH lightness isn't
  enough: a saturated colour looks brighter than a grey of the same lightness, by an amount that depends on its
  hue (the Helmholtz-Kohlrausch effect). So each colour's lightness is solved for, until Fairchild and Pirrotta's
  model of that effect gives the target. Blues come out darker and yellows lighter, and none looks brighter or
  more colourful than another. A test checks every one.
- **Track hues sit on a decagon,** 36° apart from 70°. A song's stems and the Sentence track, which are usually
  open together, take alternate corners, a pentad 72° apart, rotated to stay furthest apart under simulated
  deuteranopia and protanopia. A second or third track of a kind takes a corner between. Of the two corners left,
  one holds the accent, so a track is never mistaken for the selection, and the other is an olive that looks
  muddy at this lightness. At one lightness only hue tells colours apart, so only the tracks open together are
  promised to stay apart for colour-blind people. Track names do the rest.
- **The accent breaks the rule on purpose.** It has far more chroma than anything else, which is what lets it
  mean "selected" or "do this".
- **Phonemes** follow the same rule, dark and quiet: chroma 0.05 at an apparent lightness of 37.5. Sounds that
  ring are cool and sounds that are noise or a stop are warm.
- **The spectrogram** rises evenly in lightness with level, like magma and inferno, and never falls, so that
  brightness can be read as loudness. A rainbow would show edges that aren't in the sound.
- **Blends** go through `mix`, in OKLab, so that tints of a track colour stay clean.

### Type

| Function | Size | Use |
|---|---|---|
| `title()` | 16 medium | The name of what's selected. One per panel |
| `heading()` | 13.5 medium | Dialog titles |
| `strong()` | 12 medium | Names and emphasis at body size |
| `body()` | 12.5 | Default text |
| `caption()` | 11 | Secondary information and section headings |
| `micro()` | 10 | Dense annotations, such as clip badges |
| `num()` | 11 mono | Numbers that change, so digits don't shift |

Those return text for `ui.label(...)`. To paint on a canvas, use the matching `font_*()` token, as well as
`font_display()` for the first-run screen and `font_input()` for the Say box.

### Spacing and size

- Spacing is a 4 px scale: `SP_HALF` 2, `SP_1` 4, `SP_2` 8, `SP_3` 12, `SP_4` 16, `SP_6` 24.
- `CONTROL_H` is 24 and `CONTROL_SM` is 20. Don't invent other control heights. Transport buttons at
  28 are the one exception.
- Radius is `R_S` 4 for controls, `R_M` 6 for cards and floating bars, and `R_L` 10 for dialogs.
- Keep related things `SP_1` apart, separate groups with `SP_3`, and pad panels by `SP_3`.

### Motion

`MOTION_HOVER` (0.08 s) for state changes, and `MOTION_FLASH` (0.7 s) for "this just landed". Nothing
may make anyone wait: motion is feedback, never a transition to sit through.

## Components

Every panel builds with these. Which one do you need?

| You want to… | Use | Not |
|---|---|---|
| Trigger the main action | `Button::primary` | More than one in view |
| Trigger a normal action | `Button::secondary` | |
| Trigger an action in a dense spot | `Button::ghost`, `.small()` | |
| Trigger with an icon | `IconButton::new(icon, "What it does")` | An icon alone with no text |
| Change a number | `ValueField` | A slider or a raw text box |
| Choose from 2–5 visible options | `segmented` | A dropdown |
| Choose from a long list | `dropdown` + `menu_choice` | Segments |
| Turn something on or off | `switch`, or `Button::active` in toolbars | A checkbox |
| Switch views | `tabs`, alone in its row | Segments, or controls right beside the tabs |
| Put controls in a row that holds tabs | `right_aligned`, against the far edge | Directly after the tabs, where they read as more tabs |
| Show a menu | `menu_button` + `menu_item` | |
| Mute or solo | `letter_toggle` | |
| List shortcuts | `shortcut_row`, which lines up the descriptions | `hint_row`, which is for one or two hints |
| Separate a card's title from its contents | `card_rule` | A raw line |
| Make a button a row in a list | `Button::ghost(...).fill_width().left()` | |
| Set a device's parameter | `knob` | `ValueField`, in a device |
| Set a gain | `fader` | |
| Show a level | `meter` | |
| Explain a control | `tip`, or `.tip()` / `IconButton::shortcut` | Unexplained icons |
| Show status | `status_dot` | Coloured text alone |
| Show progress | `progress`, with `None` if unknown | A spinner with no words |
| Show a key | `keycap`, `hint_row` | Typing "⌘K" in plain text |
| Group settings | `section`, with `row` or `info_row` | Boxes around everything |
| Contain something | `card_frame` | |
| Float something | `floating_frame` | |
| Ask a question | `modal` | Only for what's irreversible |
| Tell someone it worked | `App::toast`, `toast_with` for an action | A dialog |
| Show nothing yet | `empty_state` | A blank panel |

### Using `ValueField`

Check `changed()`, begin an undo gesture, then apply. There's no need to end the gesture, which happens
when the pointer lifts.

```rust
let mut gain = clip.gain_db as f64;
let r = ui.add(ValueField::new("Gain", &mut gain, -48.0..=24.0).unit(" dB").default(0.0));
if r.changed() {
    self.begin_gesture("Change clip gain");
    self.set_selected(|c| c.gain_db = gain as f32);
}
```

Add `.bipolar()` for values with a neutral middle, and `.log()` for speeds, times and frequencies.
Drag to change, double-click to type, and Alt-click to reset: this is the same everywhere.

### Using buttons

```rust
if ui.add(Button::secondary("Tune to key").small().icon(Icon::Note).tip("Move to the nearest note", Some("P"))).clicked() {
    self.run(Action::SnapPitchToKey);
}
if ui.add(IconButton::new(Icon::Loop, "Loop").shortcut("L").active(looping)).clicked() {
    self.run(Action::ToggleLoop);
}
```

## Rules for behaviour

- **Every command goes in `ACTIONS`** in `actions.rs`. That gives it the palette, a shortcut and a place in
  the shortcut sheet. Buttons call `self.run(Action::X)` rather than doing the work themselves.
- **Every edit is undoable.** A single change goes through `self.edit("Label", |p| ...)`. A continuous one,
  such as a drag, calls `self.begin_gesture("Label")` before its first change. Labels read as "Undo
  {label}", so write them like "Change pitch". Changes to the project that skip both silently break undo.
- **Set `needs_compile`** when a direct change to the project affects sound, because nothing else reaches the
  engine. `edit` does it for you.
- **Show the shortcut in the tooltip,** written with Mac symbols (`⌘ ⇧ ⌥ ⌫`). `keys()` rewrites them for other
  platforms. Never check the platform in a panel.
- **Keep it responsive.** Decoding, analysis and rendering belong in `jobs.rs` or the render cache. Don't do
  slow work in a frame: if something is needed each frame and costs more than a millisecond, remember the
  result, as `clip_pitch` does.
- **Don't hold the frame rate up.** Nothing should request repaints unless something on screen is moving.
- **Audition on selection.** Choosing a word, take or sound should play it.
- **Experiments are always safe.** Nothing changes a recording, and a saved word doesn't change when a placed
  copy does. Any new clip property must be covered by `restore_original`.
- **Comparing happens in place.** To compare alternatives, swap them into the real project and play them in context,
  rather than making people arrange copies. `render_context` plays any project state through the real engine.
- **Don't add tracks to manage alternatives.** Use a change.
- **Things that belong together stay together.** Use `group_members` when an operation acts on a clip as
  a unit, such as moving or copying. Copies get a new group, through `duplicate_clips`.
- **Follow what you make.** After creating something, make sure it's on screen and noticed, as `after_place` does.

## States

Each panel handles all of these. Most UI bugs are a missing one.

| State | What to show |
|---|---|
| Empty | `empty_state`: what this is for, and how to fill it |
| Waiting | `progress` with words saying what's happening. Near the thing, not only in the status bar |
| Unable | What's stopping it, and a button that fixes it |
| Failed | A toast saying what happened, in plain words |
| Hover | Every interactive thing responds |
| Disabled | Dimmed, and if it isn't obvious why, a tooltip saying so |
| Long text | Truncate with `truncate()`. Nothing wraps or collides at the 1040 × 640 minimum |

## Writing

- Talk about what people are doing, not how it's built: "Find the words", not "Run inference".
- Use sentence case, always. Skip the period on labels, and use it in full sentences.
- Say what happened or what to do: "Nothing to place yet. Type a word that's in the recordings."
- Buttons are verbs or short verb phrases: "Place at 2.1.3", "Don't save".
- Don't apologize, and don't use "please", exclamation marks or jargon a mixer wouldn't use.
- `Sentence`, `take`, `line`, `word`, `sound` and `clip` are the vocabulary. A "take" is one recording of a
  word, and a "line" is what's being said.

## Drawing on canvases

The arranger, editor and transcript paint directly with `egui::Painter`, for speed. This is the one place
a panel may draw by hand, and these rules apply:

- Colours and fonts still come from tokens.
- Cull what's off-screen before doing any work.
- Keep one `ui.interact` for the whole canvas and hit-test by hand, instead of a widget per clip or word.
- Clips must behave the same everywhere, so clip behaviour belongs in `lane.rs`, which both views use.
- Clip paint order: body, waveform, header, badges, fades, selection, flash.

## egui traps we've already hit

Each of these was a real bug. They're easy to repeat.

| Trap | What to do |
|---|---|
| `painter.galley(pos, galley, color)` ignores `color` if the galley was laid out with a real colour | Lay it out with `Color32::PLACEHOLDER` when the colour varies |
| A `Frame` inside `ui.horizontal` inherits the horizontal layout | Wrap the contents in `ui.vertical` |
| `horizontal_wrapped` can't wrap framed widgets, whose size is only known afterwards | Measure the text and work out rows yourself, as `plan_chips` does |
| Sliders change on press, before `drag_started` | Begin a gesture on `changed()`, or use `ValueField` |
| ⌘C and ⌘V arrive as `Event::Copy` and `Event::Paste`, with no key event | See `handle_shortcuts` |
| Shift+/ is `Key::Questionmark`, not `Key::Slash` | Shortcuts match the logical key |
| `Modifiers::NONE` also matches with Shift held | Consume the Shift version first, as `say_input` does |
| Every `send_viewport_cmd` requests a repaint | Only send when the value changes, as `set_title` does |
| `f64::clamp(min, max)` panics if `min > max` | Guard ranges computed from clip lengths with `.max(0.0)` |
| `mac_cmd` in a `Modifiers` pattern never matches on Linux | Use `command` |
| Text fields want ⌘A, ⌘Z and ⌘⌫ | See `TEXT_EDITING` in `handle_shortcuts` |

## Recipes

**Add a command**
1. Add a variant to `Action`.
2. Add a row to `ACTIONS` with its name, shortcut and group. A test rejects duplicate shortcuts.
3. Handle it in `App::run`.
4. If it's part of a workflow, add it to the map in [WORKFLOWS.md](WORKFLOWS.md).

**Add a clip property**
1. Add the field to `Clip` in `mp-model`, with `#[serde(default)]` so old projects still load, and add any
   edit method there, with a test.
2. Add a `ValueField` to `clip_inspector`.
3. If people will reach for it often, add it to the selection bar too.
4. Apply it in `mp-engine/src/compile.rs`.
5. If it changes the sound's content, add it to `RenderKey`.

**Add a panel or tab**
1. Create `panels/name.rs` and declare it in `panels/mod.rs`.
2. Write `impl App { pub fn name_panel(&mut self, ui: &mut Ui) }`, handling every state above.
3. Add it to `shell.rs`, or to the tab list that holds it.

**Add a component**
1. Check that nothing here does the job. Prefer adding an option to an existing one.
2. Add it to the right file in `ds/`, with a doc comment saying when to use it.
3. Add it to `gallery.rs` and to the table above.
4. If it replaces an egui widget, add that widget to the forbidden list in the conformance test.

**Add a device**
1. Add it to `DeviceKind`. Give its parameters stable ids, and never reuse one.
2. Implement it in `mp-engine/src/devices.rs`. Knobs appear on their own, from `params_of`.
3. Add it to `ADDABLE` in `panels/devices.rs` and to `ACTIONS`.

**Add a picture effect**
1. Add it to the end of `PictureKind::ALL` in `mp-model/src/picture.rs`. Its position is its number in the shader.
2. List its parameters in `params_of` in `mp-vfx/src/lib.rs`. Ids are stable, and the order sets `param(i)` in the shader.
3. Add a `case` to `effect` in `mp-vfx/src/shader.wgsl`. Use only the time and parameters, never state from an earlier
   frame: preview scrubs and render jumps, and both have to give the same frame.

**Keep the title bar in mind**
On macOS the top bar is the title bar, and `chrome.rs` centres the traffic lights in it. Anything added to the bar has to leave
`chrome::buttons_inset` free on the left, and empty parts of the bar drag the window. Other systems keep their own title bar.

**Handle a mouse drag**
Use `dragged_by(egui::PointerButton::Primary)`, and the same for `drag_started_by` and `drag_stopped_by`. egui's plain
`dragged()` is true for every button, which is how a middle-button drag once moved clips. Timelines also call
`timeline::middle_drag_nav`.

## Checking your work

Run these in order. Tests alone can't tell you whether something looks right.

```bash
cargo test --workspace
```

This includes the conformance tests, which fail if a panel uses a raw egui widget, hard-coded colour or
font, or uses accent where it doesn't belong.

To check the components, save the gallery and look at it:

```bash
MOUTHPIECE_GALLERY=gallery.png cargo run -p mp-app
```

To check a real screen, script the running app with real input and take screenshots. The steps are listed
at the top of [`script.rs`](../crates/mp-app/src/script.rs).

```bash
MOUTHPIECE_SCRIPT=steps.txt cargo run -p mp-app
```

Look at the screenshot, and ask:

- [ ] Does it keep to the budget in [WORKFLOWS.md](WORKFLOWS.md)? Count the keys.
- [ ] Is there one accent in view, at most, apart from the playhead and selection?
- [ ] Does it hold together at the minimum 1040 × 640?
- [ ] Do the empty, waiting and failed states say what to do next?
- [ ] Does every icon have a tooltip, and every repeated action a key?
- [ ] Does undo restore exactly what was there before?
- [ ] Could any of it be removed?
