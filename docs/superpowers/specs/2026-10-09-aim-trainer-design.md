# Aim Trainer tab

Date: 2026-10-09
Status: draft, awaiting review

## Why

A small aim warm-up inside Prime, in the style of WAIUA's mini aim trainer (red dots that grow until
they burst, a health bar, a best score), except the crosshair moves at the player's VALORANT
sensitivity, so the hand movement carries over to the game.

## Design

Pencil file `~/.pencil/documents/2517e1ae-923d-4a91-a3cb-f6528b4323a8/pencil-new.pen`, row at y=12580:

- `mqoBa` Aim Trainer — Ready: setup modal over the arena.
- `kw664` Aim Trainer — Playing.
- `eHyAg` Aim Trainer — Game Over: results modal over the arena.

The arena is the same size in every state (user requirement). Setup and results are modals over it.
The HUD row (Score, Accuracy, Best, Health bar) is always shown: 0, "—", the best score and 100%
before a run.

The nav item "Aim Trainer" (Lucide `crosshair`) sits between Loadout and Settings. In the .pen only
the three new frames show it; the `C / Sidebar` component gets it when this ships.

## Goals

- The crosshair turns `0.07° × sens` per raw mouse count, the same as VALORANT, whatever Windows
  pointer speed and "Enhance pointer precision" are set to.
- Dots sit where they would on screen in VALORANT at 103° horizontal FOV on this monitor, so a flick
  to a dot takes the same hand movement as in game.
- Sensitivity is a plain field the player types; the last value is saved.
- One best run for the whole app.
- No new crates.

## Non-goals

- A first-person 3D view, other modes (Gridshot, Tracking), ADS or scoped sensitivity, FOV or
  aspect-ratio options, run history beyond the best run.
- Matching VALORANT exactly away from the horizontal axis: diagonals use the same flat mapping on
  both axes, which is close but not exact.

## Gameplay

Taken from WAIUA (`WAIUA/Views/Home.xaml.cs`), with sizes in degrees instead of pixels so they scale
with the monitor:

- Health starts at 100. A dot that bursts costs 15; a click that misses every dot costs 5. The run
  ends at 0.
- A dot spawns at a random spot inside the arena's view. It starts 0.3° wide and grows to 3.6° in
  about 2.2 s, then bursts.
- The spawn interval starts at 600 ms and shortens as the score rises, to 200 ms at 65 (WAIUA's
  steps: score < 5, < 20, < 35, < 50, < 65).
- A click hits the newest dot under the crosshair. A hit scores 1, removes the dot and records its
  reaction time (click time minus spawn time).
- The constants live together at the top of the game module so they can be tuned.

Results: score (= hits), misses, bursts, accuracy (hits / clicks), average reaction time and how long
the run lasted. A run beats the best run when its score is higher.

## Mouse input

iced 0.14 only reports cursor positions, which Windows has already scaled by pointer speed and
acceleration; `iced_winit` drops winit's raw `DeviceEvent::MouseMotion`, and there is no cursor-grab
command. So:

- `src/raw_mouse.rs` runs a thread with a message-only window (`HWND_MESSAGE`), registers it for
  raw mouse input (`RegisterRawInputDevices`, usage page 1, usage 2, `RIDEV_INPUTSINK`), reads each
  `WM_INPUT` with `GetRawInputData` and sends relative `(dx, dy)` counts to the app. Absolute
  movement (`MOUSE_MOVE_ABSOLUTE`, tablets and remote desktop) is ignored.
- The app takes it as an iced `Subscription::run` stream, present only while a run is playing.
  When the subscription stops, the thread unregisters (`RIDEV_REMOVE`) and closes its window.
- Windows allows one raw mouse target per process, so this takes the registration from winit while
  a run plays. iced ignores winit's device events, so nothing in Prime loses input.
- Starting a run hides the cursor (`mouse::Interaction::Hidden` over the arena) and clips it to a
  1×1 rectangle at its current position (`GetCursorPos`, `ClipCursor`), which is over the Start
  button inside Prime's window, so clicks stay in Prime. Stopping, pausing and dropping the run
  call `ClipCursor(null)`.
- `windows-sys` gains the `Win32_UI_Input` feature. Everything else is in
  `Win32_UI_WindowsAndMessaging`, which is already enabled.

## Angles to pixels

- The crosshair is a (yaw, pitch) angle from the arena's centre. Each raw count adds
  `0.07 × sens` degrees; up on the mouse is up on screen.
- `f = (monitor_width_px / 2) / tan(51.5°)` is VALORANT's focal length in physical pixels. A point
  at angle `a` sits `f × tan(a)` from the centre, on each axis.
- The arena's view is the angles that fit inside it: `atan((arena_px / 2) / f)` each way, using the
  arena's physical size (logical size × `window::scale_factor`). The crosshair is clamped to it and
  dots spawn inside it with a margin.
- The monitor width comes from `window::monitor_size` when the tab opens; it is in physical pixels.
- Resizing the window during a run changes the view; dots outside it are dropped without costing
  health.

## States

- Ready: setup modal. Sensitivity and Mouse DPI fields, eDPI / cm per 360° / FOV tiles, the best run, "Raw input · Esc to stop"
  and Start. Start is disabled while the sensitivity isn't a positive number.
- Playing: HUD, arena, dots, crosshair, "Esc to stop". The game advances on `window::frames()`.
- Paused: losing window focus (`window::Event::Unfocused`) pauses the clock, releases the cursor
  and shows the setup modal with Start reading "Resume" and the sensitivity and DPI fields
  read-only (a run keeps one sensitivity); Esc from there ends the run.
- Game over: results modal with a "New best" badge and the previous best when beaten, the
  sensitivity and DPI the run used, Done (back to Ready) and Play again.
- Esc during a run ends it and shows its results. A run that ends early still counts toward the best.
- Leaving the tab or minimizing ends the run the same way.

## Sensitivity and DPI

- Both are plain text fields. Each valid edit is saved, so the fields open with the last values.
  Nothing is read from the account or from Riot.
- Sensitivity starts empty (placeholder "e.g. 0.35"); Start stays disabled until it is a positive
  number. DPI defaults to 800.
- cm/360 = `360 / (0.07 × sens) / dpi × 2.54`; eDPI = `sens × dpi`. DPI only feeds these and the
  results line.

## Storage

`StoredState` gains one field, written only when it differs from the default:

```rust
#[serde(default, skip_serializing_if = "AimTrainerState::is_default")]
pub aim_trainer: AimTrainerState,

pub struct AimTrainerState {
    pub sensitivity: Option<f64>, // None until the player types one
    pub dpi: Option<u32>,         // None = 800
    pub best: Option<AimRun>,
}

pub struct AimRun {
    pub score: u32,
    pub misses: u32,
    pub bursts: u32,
    pub avg_reaction_ms: u32,
    pub duration_ms: u32,
    pub sensitivity: f64,
    pub dpi: u32,
    pub finished_at_unix: i64,
}
```

Accuracy is worked out from score and misses, not stored.

## Code layout

- `src/aim_trainer.rs` (outside the UI, like `game_settings.rs`): the game state and rules,
  angle-to-pixel mapping, results, and a small xorshift RNG seeded from the clock (no `rand`).
  No iced or Windows types.
- `src/raw_mouse.rs`: the raw input thread, cursor clip and release.
- `src/ui/screens/aim_trainer.rs`: the view, with the arena as an iced `canvas::Program`.
- `src/ui/mod.rs` / `src/ui/app.rs`: `Tab::AimTrainer`, its messages, the subscriptions (raw mouse
  and frames while playing), Esc and focus handling.
- AGENTS.md: a short Aim Trainer section under "What the app does".

## Testing

- `aim_trainer.rs` unit tests with a fixed seed: angle-to-pixel mapping (centre, edge of the view,
  103° across the monitor), counts per 360° (0.34 sens is about 15,126 counts), a dot bursts on time
  and costs 15, a miss costs 5, hits score and record reaction time, the spawn interval steps,
  the run ends at 0 health, results and accuracy.
- `storage.rs`: a default `AimTrainerState` isn't written; DPI and a best run round-trip.
- `src/ui/tests.rs`: opening the tab sends no requests; typing a sensitivity or DPI saves it and
  the fields open with it; Start is disabled for an empty or invalid sensitivity;
  Esc ends a run and shows results; a higher score replaces the best and saves, a lower one doesn't;
  losing focus pauses; leaving the tab ends the run.
- `raw_mouse.rs` has no unit tests (it is Windows message plumbing); it's checked by hand: with
  pointer speed at 10/11 and Enhance pointer precision on, one mouse sweep of a measured distance
  turns the same amount as in VALORANT at the same sensitivity.
