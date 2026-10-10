# Aim Trainer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an Aim Trainer tab: WAIUA-style red dots that grow until they burst, aimed with a
crosshair that turns at the player's VALORANT sensitivity from raw mouse counts.

**Architecture:** The game (rules, angles, results, saved types) is a plain module,
`src/aim_trainer.rs`, with no iced or Windows types. `src/raw_mouse.rs` reads Windows Raw Input on
its own thread and clips the cursor. `src/ui/aim.rs` holds the tab's state and handles its
messages; `src/ui/screens/aim_trainer.rs` draws the HUD, the arena (an iced `canvas`) and the two
modals.

**Tech Stack:** Rust 2024, iced 0.14 (`canvas`, `stack`, `window::frames`, `event::listen_with`,
`Subscription::run`), windows-sys 0.61, serde.

**Spec:** `docs/superpowers/specs/2026-10-09-aim-trainer-design.md`. Design: Pencil file
`~/.pencil/documents/2517e1ae-923d-4a91-a3cb-f6528b4323a8/pencil-new.pen`, frames `mqoBa` (Ready),
`kw664` (Playing), `eHyAg` (Game over) at y=12580. Read both before starting.

## Global Constraints

- No new crates. windows-sys gains only the `Win32_UI_Input` and `Win32_System_LibraryLoader` features.
- The crosshair turns `0.07° × sens` per raw count; VALORANT's horizontal FOV is 103°.
- Health 100; a burst costs 15; a miss costs 5. Dots grow from 0.3° to 3.6° in 2,200 ms.
- Spawn interval: 600 ms, 500 from score 5, 400 from 20, 300 from 35, 200 from 50.
- A frame gap counts as at most 100 ms.
- `accounts.json` gains one field, `aim_trainer`, written only when it differs from its default. DPI
  800 isn't written. One best run for the whole app.
- Sensitivity and DPI are plain fields. Nothing is read from the account or from Riot.
- The arena is the same size in every state; setup and results are modals over it.
- `cargo clippy --all-targets` stays clean (CI denies warnings); `cargo test` passes.
- Tests never run tasks: they assert on state and `task.units()`. No test may call
  `ClipCursor` or register raw input, so those only ever happen inside tasks.

## Review Focus

1. Every way a run stops releases the cursor: Esc, game over, leaving the tab, focus loss and
   closing the window. A stuck `ClipCursor` traps the user's mouse on one pixel. (Task 4 tests
   each path returns a task and leaves the run.)
2. Typed sensitivities like `0,35`, ` 0.4 `, `abc`, `0`, `-1` and `NaN`: a comma works as the
   decimal point; anything not a positive finite number is kept as text, isn't saved and keeps
   Start disabled. (Tasks 1 and 4.)
3. A zero-sized arena (window minimized, first layout) must not make a `View`, or spawning divides
   an empty range. (Tasks 1 and 4.)
4. A long frame gap (the PC stalls, the user comes back to a paused run) must not burst every dot
   at once. (Task 1: `advance` caps the step at 100 ms.)
5. Clicks before Start and a second Start during a run are ignored. (Task 4.)

---

### Task 1: Game module

**Files:**
- Create: `src/aim_trainer.rs`
- Modify: `src/lib.rs` (add `pub mod aim_trainer;` in alphabetical order, after `account_transfer`)

**Interfaces:**
- Produces (all `pub` in `crate::aim_trainer`):
  - `const DEGREES_PER_COUNT: f64`, `HORIZONTAL_FOV_DEGREES: f64`, `DEFAULT_DPI: u32`,
    `FALLBACK_MONITOR_WIDTH: f32`
  - `fn spawn_interval_ms(score: u32) -> u64`, `fn counts_per_360(sensitivity: f64) -> f64`,
    `fn cm_per_360(sensitivity: f64, dpi: u32) -> f64`, `fn parse_sensitivity(&str) -> Option<f64>`,
    `fn parse_dpi(&str) -> Option<u32>`, `fn dot_diameter(age_ms: u64) -> f64`,
    `fn accuracy(score: u32, misses: u32) -> Option<f64>`
  - `struct View` (`Clone, Copy, Debug, PartialEq`): `View::new(monitor_width: f32, arena_width: f32,
    arena_height: f32) -> Option<View>`, `half_angles(&self) -> (f64, f64)`,
    `to_px(&self, yaw: f64, pitch: f64) -> (f64, f64)`,
    `radius_px(&self, yaw: f64, pitch: f64, diameter: f64) -> f64`
  - `struct Dot { pub yaw: f64, pub pitch: f64, .. }` (`Clone, Copy, Debug, PartialEq`)
  - `struct Game` (`Clone, Debug`): `new(sensitivity: f64, view: View, seed: u64)`,
    `sensitivity()`, `view()`, `set_view(View)`, `move_aim(dx: i32, dy: i32)`,
    `advance(elapsed_ms: u64)`, `click()`, `is_over()`, `health() -> i32`, `score() -> u32`,
    `aim() -> (f64, f64)`, `dots() -> impl Iterator<Item = (Dot, f64)>` (dot, diameter in degrees),
    `results() -> RunResults`
  - `struct RunResults { score, misses, bursts: u32, avg_reaction_ms: Option<u32>, duration_ms: u64 }`
    with `accuracy()`
  - `struct AimTrainerState { sensitivity: Option<f64>, dpi: Option<u32>, best: Option<AimRun> }`
    (`Default`, serde) with `is_default()` and `dpi() -> u32`
  - `struct AimRun { score, misses, bursts: u32, avg_reaction_ms: Option<u32>, duration_ms: u64,
    sensitivity: f64, dpi: u32, finished_at_unix: i64 }` (`Copy`, serde) with
    `AimRun::new(RunResults, sensitivity: f64, dpi: u32, finished_at_unix: i64)` and `accuracy()`

Angles are degrees from the arena's centre: yaw positive to the right, pitch positive downward
(the same direction as screen y and raw mouse y).

- [ ] **Step 1: Write the failing tests**

Create `src/aim_trainer.rs` with only the test module, so it fails to compile:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SENS: f64 = 0.34;

    fn view() -> View {
        View::new(1920.0, 976.0, 664.0).expect("view")
    }

    fn game() -> Game {
        Game::new(SENS, view(), 42)
    }

    fn step(game: &mut Game, ms: u64) {
        for _ in 0..ms / 100 {
            game.advance(100);
        }
    }

    /// Turns the crosshair onto the newest dot the way a mouse would, in whole counts.
    fn aim_at_newest(game: &mut Game) {
        let (dot, _) = game.dots().last().expect("a dot");
        let (yaw, pitch) = game.aim();
        let per_count = DEGREES_PER_COUNT * SENS;
        game.move_aim(
            ((dot.yaw - yaw) / per_count).round() as i32,
            ((dot.pitch - pitch) / per_count).round() as i32,
        );
    }

    #[test]
    fn sensitivity_converts_like_valorant() {
        assert!((counts_per_360(SENS) - 15_126.05).abs() < 0.01);
        assert!((cm_per_360(SENS, 800) - 48.03).abs() < 0.01);
    }

    #[test]
    fn typed_sensitivity_must_be_a_positive_number() {
        assert_eq!(parse_sensitivity("0.34"), Some(0.34));
        assert_eq!(parse_sensitivity(" 0.4 "), Some(0.4));
        assert_eq!(parse_sensitivity("0,35"), Some(0.35));
        for bad in ["", "abc", "0", "-1", "NaN", "inf"] {
            assert_eq!(parse_sensitivity(bad), None, "{bad}");
        }
        assert_eq!(parse_dpi("1600"), Some(1600));
        assert_eq!(parse_dpi("0"), None);
        assert_eq!(parse_dpi("8x"), None);
    }

    #[test]
    fn view_matches_valorants_field_of_view() {
        let full = View::new(1920.0, 1920.0, 1080.0).expect("view");
        assert!((full.half_angles().0 - 51.5).abs() < 1e-9);
        let (x, y) = full.to_px(51.5, 0.0);
        assert!((x - 1920.0).abs() < 1e-6 && (y - 540.0).abs() < 1e-6);
        assert_eq!(full.to_px(0.0, 0.0), (960.0, 540.0));

        // A 976px arena on a 1920px monitor shows about 32.6° each way.
        assert!((view().half_angles().0 - 32.58).abs() < 0.01);
    }

    #[test]
    fn a_view_needs_a_real_size() {
        assert_eq!(View::new(1920.0, 0.0, 600.0), None);
        assert_eq!(View::new(1920.0, 800.0, 0.0), None);
        assert_eq!(View::new(0.0, 800.0, 600.0), None);
    }

    #[test]
    fn dots_near_the_edge_draw_larger() {
        let view = view();
        // 30° out, the width stretches by 1/cos²(30°) ≈ 1.33 and the height not at all.
        assert!(view.radius_px(30.0, 0.0, 3.6) > view.radius_px(0.0, 0.0, 3.6) * 1.1);
    }

    #[test]
    fn spawn_interval_steps_down_with_score() {
        let steps: Vec<u64> = [0, 4, 5, 19, 20, 34, 35, 49, 50, 200]
            .into_iter()
            .map(spawn_interval_ms)
            .collect();
        assert_eq!(steps, [600, 600, 500, 500, 400, 400, 300, 300, 200, 200]);
    }

    #[test]
    fn dots_spawn_at_the_start_then_on_the_interval() {
        let mut game = game();
        game.advance(0);
        assert_eq!(game.dots().count(), 1);
        step(&mut game, 500);
        assert_eq!(game.dots().count(), 1);
        step(&mut game, 100);
        assert_eq!(game.dots().count(), 2);
        let (hx, hy) = game.view().half_angles();
        assert!(game.dots().all(|(dot, _)| dot.yaw.abs() <= hx && dot.pitch.abs() <= hy));
    }

    #[test]
    fn a_dot_bursts_after_its_lifetime_and_costs_health() {
        let mut game = game();
        game.advance(0);
        step(&mut game, 2_100);
        assert_eq!(game.health(), 100);
        step(&mut game, 100);
        assert_eq!(game.health(), 85);
        assert_eq!(game.results().bursts, 1);
    }

    #[test]
    fn a_missed_click_costs_health() {
        let mut game = game();
        game.click();
        assert_eq!(game.health(), 95);
        assert_eq!(game.results().misses, 1);
    }

    #[test]
    fn a_hit_scores_and_records_the_reaction_time() {
        let mut game = game();
        game.advance(0);
        step(&mut game, 300);
        aim_at_newest(&mut game);
        game.click();
        let results = game.results();
        assert_eq!((results.score, results.misses), (1, 0));
        assert_eq!(results.avg_reaction_ms, Some(300));
        assert_eq!(game.dots().count(), 0);
        assert_eq!(game.health(), 100);
    }

    #[test]
    fn a_long_frame_gap_counts_as_one_short_step() {
        let mut game = game();
        game.advance(0);
        game.advance(60_000);
        assert_eq!(game.health(), 100);
        assert_eq!(game.results().duration_ms, 100);
    }

    #[test]
    fn the_run_ends_at_zero_health_and_then_ignores_input() {
        let mut game = game();
        for _ in 0..20 {
            game.click();
        }
        assert!(game.is_over());
        assert_eq!(game.health(), 0);
        game.click();
        game.advance(100);
        assert_eq!(game.results().misses, 20);
        assert_eq!(game.results().duration_ms, 0);
    }

    #[test]
    fn the_crosshair_stays_inside_the_view() {
        let mut game = game();
        game.move_aim(1_000_000, -1_000_000);
        let (hx, hy) = game.view().half_angles();
        assert_eq!(game.aim(), (hx, -hy));
    }

    #[test]
    fn shrinking_the_view_drops_outside_dots_for_free() {
        let mut game = game();
        game.advance(0);
        game.set_view(View::new(1920.0, 2.0, 2.0).expect("view"));
        assert_eq!(game.dots().count(), 0);
        assert_eq!(game.health(), 100);
        let (hx, hy) = game.view().half_angles();
        assert!(game.aim().0.abs() <= hx && game.aim().1.abs() <= hy);
    }

    #[test]
    fn accuracy_is_hits_over_clicks() {
        assert_eq!(accuracy(0, 0), None);
        assert_eq!(accuracy(1, 1), Some(0.5));
        assert_eq!(accuracy(3, 0), Some(1.0));
    }
}
```

- [ ] **Step 2: Add the module to `src/lib.rs` and run the tests to see them fail**

Add `pub mod aim_trainer;` after `pub mod account_transfer;` in `src/lib.rs`.

Run: `cargo test --lib aim_trainer`
Expected: FAIL to compile (`cannot find type View`, `cannot find function parse_sensitivity`, ...).

- [ ] **Step 3: Write the implementation above the test module**

```rust
//! The Aim Trainer's game: WAIUA-style dots that grow until they burst, aimed at VALORANT's
//! sensitivity. Angles are degrees from the arena's centre (yaw right, pitch down), times are
//! milliseconds. Nothing here knows about iced or Windows.

use serde::{Deserialize, Serialize};

/// VALORANT turns this many degrees per mouse count at sensitivity 1.
pub const DEGREES_PER_COUNT: f64 = 0.07;
/// VALORANT's horizontal field of view, across the whole monitor.
pub const HORIZONTAL_FOV_DEGREES: f64 = 103.0;
pub const DEFAULT_DPI: u32 = 800;
/// Used when the monitor's width can't be read.
pub const FALLBACK_MONITOR_WIDTH: f32 = 1920.0;

const START_HEALTH: i32 = 100;
const BURST_DAMAGE: i32 = 15;
const MISS_DAMAGE: i32 = 5;
const DOT_START_DEGREES: f64 = 0.3;
const DOT_MAX_DEGREES: f64 = 3.6;
const DOT_LIFETIME_MS: u64 = 2_200;
/// A longer frame gap (a stall, a debugger) counts as this long, so it can't burst every dot.
const MAX_STEP_MS: u64 = 100;

/// How long until the next dot, from WAIUA's steps.
pub fn spawn_interval_ms(score: u32) -> u64 {
    match score {
        0..5 => 600,
        5..20 => 500,
        20..35 => 400,
        35..50 => 300,
        _ => 200,
    }
}

pub fn counts_per_360(sensitivity: f64) -> f64 {
    360.0 / (DEGREES_PER_COUNT * sensitivity)
}

pub fn cm_per_360(sensitivity: f64, dpi: u32) -> f64 {
    counts_per_360(sensitivity) / f64::from(dpi) * 2.54
}

/// A typed sensitivity: a positive number, with a comma accepted as the decimal point.
pub fn parse_sensitivity(input: &str) -> Option<f64> {
    let value: f64 = input.trim().replace(',', ".").parse().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

pub fn parse_dpi(input: &str) -> Option<u32> {
    input.trim().parse().ok().filter(|dpi| *dpi > 0)
}

/// A dot's width in degrees once it has lived `age_ms`.
pub fn dot_diameter(age_ms: u64) -> f64 {
    let grown = age_ms.min(DOT_LIFETIME_MS) as f64 / DOT_LIFETIME_MS as f64;
    DOT_START_DEGREES + (DOT_MAX_DEGREES - DOT_START_DEGREES) * grown
}

pub fn accuracy(score: u32, misses: u32) -> Option<f64> {
    let clicks = score + misses;
    (clicks > 0).then(|| f64::from(score) / f64::from(clicks))
}

/// The slice of VALORANT's view that fits in the arena, as it would sit on this monitor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    focal_px: f64,
    width_px: f64,
    height_px: f64,
}

impl View {
    /// `monitor_width` and the arena's size must be in the same units; iced's logical pixels are.
    pub fn new(monitor_width: f32, arena_width: f32, arena_height: f32) -> Option<Self> {
        if !(monitor_width >= 1.0 && arena_width >= 1.0 && arena_height >= 1.0) {
            return None;
        }
        let half_fov = (HORIZONTAL_FOV_DEGREES / 2.0).to_radians();
        Some(Self {
            focal_px: f64::from(monitor_width) / 2.0 / half_fov.tan(),
            width_px: f64::from(arena_width),
            height_px: f64::from(arena_height),
        })
    }

    /// The furthest yaw and pitch the arena shows each way from its centre.
    pub fn half_angles(&self) -> (f64, f64) {
        (
            (self.width_px / 2.0 / self.focal_px).atan().to_degrees(),
            (self.height_px / 2.0 / self.focal_px).atan().to_degrees(),
        )
    }

    /// Where an angle lands, in pixels from the arena's top-left.
    pub fn to_px(&self, yaw: f64, pitch: f64) -> (f64, f64) {
        (
            self.width_px / 2.0 + self.focal_px * yaw.to_radians().tan(),
            self.height_px / 2.0 + self.focal_px * pitch.to_radians().tan(),
        )
    }

    /// A dot's drawn radius: the mean of its projected width and height, so dots near the edge
    /// stretch the way they do in game.
    pub fn radius_px(&self, yaw: f64, pitch: f64, diameter: f64) -> f64 {
        let half = diameter / 2.0;
        let width = self.to_px(yaw + half, pitch).0 - self.to_px(yaw - half, pitch).0;
        let height = self.to_px(yaw, pitch + half).1 - self.to_px(yaw, pitch - half).1;
        (width + height) / 4.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    pub yaw: f64,
    pub pitch: f64,
    spawned_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunResults {
    pub score: u32,
    pub misses: u32,
    pub bursts: u32,
    pub avg_reaction_ms: Option<u32>,
    pub duration_ms: u64,
}

impl RunResults {
    pub fn accuracy(&self) -> Option<f64> {
        accuracy(self.score, self.misses)
    }
}

/// One run, from Start until health runs out or the player stops it.
#[derive(Clone, Debug)]
pub struct Game {
    sensitivity: f64,
    view: View,
    rng: Rng,
    now_ms: u64,
    next_spawn_ms: u64,
    aim: (f64, f64),
    dots: Vec<Dot>,
    health: i32,
    score: u32,
    misses: u32,
    bursts: u32,
    reaction_total_ms: u64,
}

impl Game {
    pub fn new(sensitivity: f64, view: View, seed: u64) -> Self {
        Self {
            sensitivity,
            view,
            rng: Rng::new(seed),
            now_ms: 0,
            next_spawn_ms: 0,
            aim: (0.0, 0.0),
            dots: Vec::new(),
            health: START_HEALTH,
            score: 0,
            misses: 0,
            bursts: 0,
            reaction_total_ms: 0,
        }
    }

    pub fn sensitivity(&self) -> f64 {
        self.sensitivity
    }

    pub fn view(&self) -> View {
        self.view
    }

    /// The arena changed size: keep the crosshair inside and drop dots that no longer fit, free.
    pub fn set_view(&mut self, view: View) {
        self.view = view;
        let (hx, hy) = view.half_angles();
        self.aim = (self.aim.0.clamp(-hx, hx), self.aim.1.clamp(-hy, hy));
        self.dots
            .retain(|dot| dot.yaw.abs() <= hx && dot.pitch.abs() <= hy);
    }

    /// Raw mouse counts, turned the way VALORANT turns them.
    pub fn move_aim(&mut self, dx: i32, dy: i32) {
        if self.is_over() {
            return;
        }
        let per_count = DEGREES_PER_COUNT * self.sensitivity;
        let (hx, hy) = self.view.half_angles();
        self.aim = (
            (self.aim.0 + f64::from(dx) * per_count).clamp(-hx, hx),
            (self.aim.1 + f64::from(dy) * per_count).clamp(-hy, hy),
        );
    }

    pub fn advance(&mut self, elapsed_ms: u64) {
        if self.is_over() {
            return;
        }
        self.now_ms += elapsed_ms.min(MAX_STEP_MS);
        let now = self.now_ms;

        let before = self.dots.len();
        self.dots
            .retain(|dot| now - dot.spawned_at_ms < DOT_LIFETIME_MS);
        let burst = (before - self.dots.len()) as u32;
        self.bursts += burst;
        self.health -= BURST_DAMAGE * burst as i32;
        if self.is_over() {
            return;
        }

        if now >= self.next_spawn_ms {
            self.spawn();
            self.next_spawn_ms = now + spawn_interval_ms(self.score);
        }
    }

    fn spawn(&mut self) {
        let (hx, hy) = self.view.half_angles();
        let margin = DOT_MAX_DEGREES / 2.0;
        let yaw_range = (hx - margin).max(0.0);
        let pitch_range = (hy - margin).max(0.0);
        self.dots.push(Dot {
            yaw: self.rng.between(-yaw_range, yaw_range),
            pitch: self.rng.between(-pitch_range, pitch_range),
            spawned_at_ms: self.now_ms,
        });
    }

    /// A shot at the crosshair: hits the newest dot under it, or misses.
    pub fn click(&mut self) {
        if self.is_over() {
            return;
        }
        let now = self.now_ms;
        let (yaw, pitch) = self.aim;
        let hit = self.dots.iter().rposition(|dot| {
            let radius = dot_diameter(now - dot.spawned_at_ms) / 2.0;
            (dot.yaw - yaw).hypot(dot.pitch - pitch) <= radius
        });
        match hit {
            Some(index) => {
                let dot = self.dots.remove(index);
                self.score += 1;
                self.reaction_total_ms += now - dot.spawned_at_ms;
            }
            None => {
                self.misses += 1;
                self.health -= MISS_DAMAGE;
            }
        }
    }

    pub fn is_over(&self) -> bool {
        self.health <= 0
    }

    pub fn health(&self) -> i32 {
        self.health.max(0)
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn aim(&self) -> (f64, f64) {
        self.aim
    }

    /// Each dot with its current width in degrees, oldest first.
    pub fn dots(&self) -> impl Iterator<Item = (Dot, f64)> + '_ {
        self.dots
            .iter()
            .map(|dot| (*dot, dot_diameter(self.now_ms - dot.spawned_at_ms)))
    }

    pub fn results(&self) -> RunResults {
        RunResults {
            score: self.score,
            misses: self.misses,
            bursts: self.bursts,
            avg_reaction_ms: (self.score > 0).then(|| {
                u32::try_from(self.reaction_total_ms / u64::from(self.score)).unwrap_or(u32::MAX)
            }),
            duration_ms: self.now_ms,
        }
    }
}

/// xorshift64: plenty for dot positions, without adding `rand`.
#[derive(Clone, Debug)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn between(&mut self, low: f64, high: f64) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        let unit = (self.0 >> 11) as f64 / (1_u64 << 53) as f64;
        low + (high - low) * unit
    }
}

/// What `accounts.json` keeps for the Aim Trainer. Left out while it's all default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AimTrainerState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitivity: Option<f64>,
    /// `None` is `DEFAULT_DPI`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best: Option<AimRun>,
}

// Sensitivities only come from `parse_sensitivity`, which never returns NaN.
impl Eq for AimTrainerState {}

impl AimTrainerState {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn dpi(&self) -> u32 {
        self.dpi.unwrap_or(DEFAULT_DPI)
    }
}

/// A finished run as saved: the best one is kept for the whole app.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AimRun {
    pub score: u32,
    pub misses: u32,
    pub bursts: u32,
    pub avg_reaction_ms: Option<u32>,
    pub duration_ms: u64,
    pub sensitivity: f64,
    pub dpi: u32,
    pub finished_at_unix: i64,
}

impl AimRun {
    pub fn new(results: RunResults, sensitivity: f64, dpi: u32, finished_at_unix: i64) -> Self {
        Self {
            score: results.score,
            misses: results.misses,
            bursts: results.bursts,
            avg_reaction_ms: results.avg_reaction_ms,
            duration_ms: results.duration_ms,
            sensitivity,
            dpi,
            finished_at_unix,
        }
    }

    pub fn accuracy(&self) -> Option<f64> {
        accuracy(self.score, self.misses)
    }
}
```

- [ ] **Step 4: Run the tests and clippy**

Run: `cargo test --lib aim_trainer` then `cargo clippy --all-targets`
Expected: all `aim_trainer::tests` pass, clippy clean. If clippy flags the `as` casts in
`dot_diameter`, `between` or `advance`, they're value-safe (≤ 2,200, 53-bit, dot count); add
`#[allow(clippy::cast_precision_loss)]` / `cast_possible_truncation` on that function with a
one-line reason only if it actually warns.

- [ ] **Step 5: Commit**

```bash
git add src/aim_trainer.rs src/lib.rs
git commit -m "Add the Aim Trainer game: growing dots aimed at VALORANT sensitivity"
```

### Task 2: Save Aim Trainer state in accounts.json

**Files:**
- Modify: `src/storage.rs` (`StoredState`, its `Default`, tests module)

**Interfaces:**
- Consumes: `crate::aim_trainer::{AimTrainerState, AimRun}` (Task 1)
- Produces: `StoredState.aim_trainer: AimTrainerState`

- [ ] **Step 1: Write the failing test** at the end of `mod tests` in `src/storage.rs`

```rust
    #[test]
    fn aim_trainer_state_is_only_saved_once_set() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let mut state = StoredState::default();

        repo.save(&state).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("read");
        assert!(!saved.contains("aim_trainer"), "{saved}");

        state.aim_trainer.sensitivity = Some(0.34);
        state.aim_trainer.dpi = Some(1600);
        state.aim_trainer.best = Some(crate::aim_trainer::AimRun {
            score: 64,
            misses: 6,
            bursts: 4,
            avg_reaction_ms: Some(412),
            duration_ms: 72_000,
            sensitivity: 0.34,
            dpi: 1600,
            finished_at_unix: 1_791_331_200,
        });
        repo.save(&state).expect("save");
        assert_eq!(repo.load().expect("load").aim_trainer, state.aim_trainer);
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --lib storage::tests::aim_trainer_state_is_only_saved_once_set`
Expected: FAIL to compile (`no field aim_trainer on StoredState`).

- [ ] **Step 3: Add the field**

In `StoredState`, after `presence_status`:

```rust
    /// The Aim Trainer's sensitivity, DPI and best run. Left out until one is set.
    #[serde(default, skip_serializing_if = "AimTrainerState::is_default")]
    pub aim_trainer: AimTrainerState,
```

Add `use crate::aim_trainer::AimTrainerState;` to the imports, and `aim_trainer:
AimTrainerState::default(),` to `impl Default for StoredState`.

- [ ] **Step 4: Run the storage tests and clippy**

Run: `cargo test --lib storage` then `cargo clippy --all-targets`
Expected: PASS, clean. (`StoredState` derives `Eq`; `AimTrainerState`'s manual `Eq` covers it.)

- [ ] **Step 5: Commit**

```bash
git add src/storage.rs
git commit -m "Save the Aim Trainer's sensitivity, DPI and best run in accounts.json"
```

### Task 3: Raw mouse input and cursor clip

**Files:**
- Create: `src/raw_mouse.rs`
- Modify: `src/lib.rs` (`pub mod raw_mouse;` after `pub mod launch;`), `Cargo.toml` (windows-sys features)

**Interfaces:**
- Produces: `crate::raw_mouse::movements() -> impl Stream<Item = (i32, i32)>` (for
  `Subscription::run`), `hold_cursor()`, `release_cursor()`

No unit tests: this is Windows message plumbing. It's checked by hand in Task 5.

- [ ] **Step 1: Add the windows-sys features**

In `Cargo.toml`, add to the `windows-sys` feature list (keep it alphabetical):
`"Win32_System_LibraryLoader"` and `"Win32_UI_Input"`.

- [ ] **Step 2: Write `src/raw_mouse.rs`**

```rust
//! Raw mouse movement for the Aim Trainer, read the way VALORANT reads it: Raw Input counts,
//! before Windows pointer speed and acceleration. iced 0.14 drops winit's raw device events, so a
//! thread with its own message-only window reads them. Windows sends a process's raw mouse input
//! to one window, so this takes it from winit while a run plays; iced ignores it anyway.

use std::cell::RefCell;
use std::ptr::{null, null_mut};

use iced::futures::{SinkExt, Stream};
use tokio::sync::mpsc::UnboundedSender;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ClipCursor, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetMessageW, HWND_MESSAGE, MSG, PostMessageW, PostQuitMessage, RegisterClassW, WM_CLOSE,
    WM_DESTROY, WM_INPUT, WNDCLASSW,
};

const GENERIC_DESKTOP: u16 = 0x01;
const MOUSE: u16 = 0x02;

thread_local! {
    static SENDER: RefCell<Option<UnboundedSender<(i32, i32)>>> = const { RefCell::new(None) };
}

/// Relative mouse counts while subscribed. Ending the subscription closes the reader's window,
/// which gives the raw mouse input back.
pub fn movements() -> impl Stream<Item = (i32, i32)> {
    iced::stream::channel(64, async |mut output| {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let Some(_reader) = Reader::start(sender) else {
            return;
        };
        // A 1000 Hz mouse sends a message every millisecond; one per wake-up is plenty.
        while let Some((mut dx, mut dy)) = receiver.recv().await {
            while let Ok((x, y)) = receiver.try_recv() {
                dx += x;
                dy += y;
            }
            if output.send((dx, dy)).await.is_err() {
                break;
            }
        }
    })
}

/// Holds the cursor on its current pixel, inside Prime, so a fast flick can't click elsewhere.
pub fn hold_cursor() {
    let mut point = POINT::default();
    // SAFETY: both calls only read or write the local values passed to them.
    unsafe {
        if GetCursorPos(&mut point) != 0 {
            let rect = RECT {
                left: point.x,
                top: point.y,
                right: point.x + 1,
                bottom: point.y + 1,
            };
            ClipCursor(&rect);
        }
    }
}

pub fn release_cursor() {
    // SAFETY: a null rectangle frees the cursor.
    unsafe {
        ClipCursor(null());
    }
}

/// The reader thread's window, kept as an address because `HWND` isn't `Send`.
struct Reader {
    window: usize,
}

impl Reader {
    fn start(sender: UnboundedSender<(i32, i32)>) -> Option<Self> {
        let (ready, started) = std::sync::mpsc::channel();
        std::thread::spawn(move || read_on_this_thread(sender, ready));
        started.recv().ok().flatten().map(|window| Self { window })
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: posting to a window that's already gone just fails.
        unsafe {
            PostMessageW(self.window as HWND, WM_CLOSE, 0, 0);
        }
    }
}

fn read_on_this_thread(
    sender: UnboundedSender<(i32, i32)>,
    ready: std::sync::mpsc::Sender<Option<usize>>,
) {
    SENDER.with_borrow_mut(|slot| *slot = Some(sender));
    let class_name: Vec<u16> = "PrimeRawMouse".encode_utf16().chain([0]).collect();

    // SAFETY: `class_name` outlives the window; the window and its messages stay on this thread.
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        // Fails harmlessly once the class exists from an earlier run.
        RegisterClassW(&class);
        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            let _ = ready.send(None);
            return;
        }
        let device = RAWINPUTDEVICE {
            usUsagePage: GENERIC_DESKTOP,
            usUsage: MOUSE,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        };
        if RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) == 0 {
            DestroyWindow(window);
            let _ = ready.send(None);
            return;
        }
        let _ = ready.send(Some(window as usize));

        let mut message = MSG::default();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_INPUT => {
            // SAFETY: Windows passes the raw input handle in `lparam` for WM_INPUT.
            if let Some(movement) = unsafe { read_movement(lparam) } {
                SENDER.with_borrow(|sender| {
                    if let Some(sender) = sender {
                        let _ = sender.send(movement);
                    }
                });
            }
            // SAFETY: WM_INPUT must reach DefWindowProc so Windows can clean up.
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_DESTROY => {
            let device = RAWINPUTDEVICE {
                usUsagePage: GENERIC_DESKTOP,
                usUsage: MOUSE,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: null_mut(),
            };
            // SAFETY: `device` is a local value; quitting ends this thread's message loop.
            unsafe {
                RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32);
                PostQuitMessage(0);
            }
            0
        }
        // SAFETY: everything else gets Windows' default handling, including WM_CLOSE.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Relative movement from one WM_INPUT; absolute devices (tablets, remote desktop) are ignored.
unsafe fn read_movement(lparam: LPARAM) -> Option<(i32, i32)> {
    let mut input = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    // SAFETY: `input` is large enough for a mouse packet and `size` says so.
    let read = unsafe {
        GetRawInputData(
            lparam as HRAWINPUT,
            RID_INPUT,
            (&raw mut input).cast(),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if read == u32::MAX || input.header.dwType != RIM_TYPEMOUSE {
        return None;
    }
    // SAFETY: the header says this packet is a mouse's.
    let mouse = unsafe { input.data.mouse };
    let relative = mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0;
    (relative && (mouse.lLastX != 0 || mouse.lLastY != 0)).then_some((mouse.lLastX, mouse.lLastY))
}
```

- [ ] **Step 3: Build and lint**

Run: `cargo build` then `cargo clippy --all-targets`
Expected: builds; clippy clean. Nothing calls this yet, but it's `pub` in a library crate, so no
dead-code warning and no `#[allow]` is needed. If a windows-sys name doesn't resolve, look it up in
`~/.cargo/registry/src/*/windows-sys-0.61.2/src/Windows/Win32/UI/Input/mod.rs` or
`.../WindowsAndMessaging/mod.rs` rather than adding features beyond the two above.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/raw_mouse.rs
git commit -m "Read raw mouse counts on their own thread for the Aim Trainer"
```

### Task 4: Aim Trainer tab state, messages and subscriptions

**Files:**
- Create: `src/ui/aim.rs`
- Modify: `src/ui/mod.rs` (`mod aim;`, `Tab::AimTrainer` and every match on `Tab`, `Message::Aim`,
  `PrimeApp.aim`, `app_subscription`), `src/ui/app.rs` (boot, `TabSelected`, `escape_message`,
  `CloseRequested`, `load_active_tab`, the `Message::Aim` arm), `src/ui/shell.rs` and
  `src/ui/screens/mod.rs` (only the exhaustive matches; the nav item and the view are Task 5)
- Test: `src/ui/tests.rs`

**Interfaces:**
- Consumes: Task 1's `Game`, `View`, `AimRun`, `parse_sensitivity`, `parse_dpi`, `DEFAULT_DPI`,
  `FALLBACK_MONITOR_WIDTH`; Task 2's `StoredState.aim_trainer`; Task 3's `movements`,
  `hold_cursor`, `release_cursor`.
- Produces (in `crate::ui::aim`, all `pub(super)`): `enum AimMessage` (below), `struct
  AimTrainerTab { phase, sensitivity_input, dpi_input, monitor_width, arena, last_frame }`,
  `enum AimPhase { Ready, Playing(Box<Game>), Paused(Box<Game>), Over { game, run, previous_best,
  new_best } }` with `game()` and `is_running()`, `AimTrainerTab::{sensitivity(), view(),
  can_start()}`, `PrimeApp::{update_aim, open_aim_trainer, end_aim_run, pause_aim_run}`,
  `fn subscription(app: &PrimeApp) -> Option<Subscription<Message>>`.
- `Tab::AimTrainer` displays as "Aim Trainer"; `Message::Aim(AimMessage)`.

- [ ] **Step 1: Write the failing tests** at the end of `src/ui/tests.rs`

Add `use super::aim::{AimMessage, AimPhase};` to the imports at the top of `src/ui/tests.rs`
(and `use std::time::Duration;` if it isn't there).

```rust
fn aim(message: AimMessage) -> Message {
    Message::Aim(message)
}

/// The Aim Trainer tab, laid out on a 1920px monitor, with 0.34 typed in.
fn aim_app(dir: &Path) -> PrimeApp {
    let mut app = test_app(dir);
    let _ = app.update(Message::TabSelected(Tab::AimTrainer));
    let _ = app.update(aim(AimMessage::MonitorMeasured(Some(1920.0))));
    let _ = app.update(aim(AimMessage::ArenaResized(iced::Size::new(976.0, 664.0))));
    let _ = app.update(aim(AimMessage::SensitivityChanged("0.34".into())));
    app
}

/// Sends frames `ms` apart in 100 ms steps, the way `window::frames` would.
fn aim_frames(app: &mut PrimeApp, ms: u64) {
    let mut at = app.aim.last_frame.unwrap_or_else(iced::time::Instant::now);
    let _ = app.update(aim(AimMessage::Frame(at)));
    for _ in 0..ms / 100 {
        at += Duration::from_millis(100);
        let _ = app.update(aim(AimMessage::Frame(at)));
    }
}

/// Moves the crosshair onto the newest dot in whole counts and clicks it.
fn aim_hit_newest(app: &mut PrimeApp) {
    let AimPhase::Playing(game) = &app.aim.phase else {
        panic!("not playing");
    };
    let (dot, _) = game.dots().last().expect("a dot");
    let (yaw, pitch) = game.aim();
    let per_count = crate::aim_trainer::DEGREES_PER_COUNT * game.sensitivity();
    let dx = ((dot.yaw - yaw) / per_count).round() as i32;
    let dy = ((dot.pitch - pitch) / per_count).round() as i32;
    let _ = app.update(aim(AimMessage::MouseMoved(dx, dy)));
    let _ = app.update(aim(AimMessage::Clicked));
}

#[test]
fn aim_trainer_opens_with_the_saved_sensitivity_and_dpi() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.state.aim_trainer.sensitivity = Some(0.4);
    app.state.aim_trainer.dpi = Some(1600);

    let _ = app.update(Message::TabSelected(Tab::AimTrainer));

    assert_eq!(app.aim.sensitivity_input, "0.4");
    assert_eq!(app.aim.dpi_input, "1600");
}

#[test]
fn aim_sensitivity_is_saved_only_when_valid() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    assert_eq!(app.state.aim_trainer.sensitivity, Some(0.34));

    let task = app.update(aim(AimMessage::SensitivityChanged("0,35".into())));
    assert!(task.units() > 0, "saves accounts.json");
    assert_eq!(app.state.aim_trainer.sensitivity, Some(0.35));

    for bad in ["abc", "0", "-1", ""] {
        let task = app.update(aim(AimMessage::SensitivityChanged(bad.into())));
        assert_eq!(task.units(), 0, "{bad} isn't saved");
        assert_eq!(app.aim.sensitivity_input, bad, "the field keeps what was typed");
        assert_eq!(app.state.aim_trainer.sensitivity, Some(0.35));
        assert!(!app.aim.can_start(), "{bad} can't start a run");
        let _ = app.update(aim(AimMessage::Start));
        assert!(matches!(app.aim.phase, AimPhase::Ready));
    }
}

#[test]
fn aim_dpi_of_800_is_not_saved() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());

    let _ = app.update(aim(AimMessage::DpiChanged("1600".into())));
    assert_eq!(app.state.aim_trainer.dpi, Some(1600));
    let _ = app.update(aim(AimMessage::DpiChanged("800".into())));
    assert_eq!(app.state.aim_trainer.dpi, None);
    let _ = app.update(aim(AimMessage::DpiChanged("0".into())));
    assert_eq!(app.state.aim_trainer.dpi, None);
    assert_eq!(app.aim.dpi_input, "0");
}

#[test]
fn aim_run_starts_once_and_esc_ends_it_with_results() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());

    let _ = app.update(aim(AimMessage::Clicked));
    assert!(matches!(app.aim.phase, AimPhase::Ready), "clicks before Start do nothing");

    let task = app.update(aim(AimMessage::Start));
    assert!(task.units() > 0, "holds the cursor");
    assert!(matches!(app.aim.phase, AimPhase::Playing(_)));
    aim_frames(&mut app, 0);
    aim_hit_newest(&mut app);

    let _ = app.update(aim(AimMessage::Start));
    let AimPhase::Playing(game) = &app.aim.phase else {
        panic!("still playing");
    };
    assert_eq!(game.score(), 1, "a second Start doesn't restart the run");

    let task = app.update(Message::EscapePressed);
    assert!(task.units() > 0, "releases the cursor");
    let AimPhase::Over { run, .. } = &app.aim.phase else {
        panic!("Esc shows the results");
    };
    assert_eq!(run.score, 1);

    let _ = app.update(aim(AimMessage::Done));
    assert!(matches!(app.aim.phase, AimPhase::Ready));
}

#[test]
fn aim_best_run_is_replaced_only_by_a_higher_score() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let best = |score| crate::aim_trainer::AimRun {
        score,
        misses: 0,
        bursts: 0,
        avg_reaction_ms: None,
        duration_ms: 0,
        sensitivity: 0.34,
        dpi: 800,
        finished_at_unix: 0,
    };
    app.state.aim_trainer.best = Some(best(1));

    let _ = app.update(aim(AimMessage::Start));
    aim_frames(&mut app, 0);
    aim_hit_newest(&mut app);
    aim_frames(&mut app, 600);
    aim_hit_newest(&mut app);
    let task = app.update(aim(AimMessage::Stop));

    assert!(task.units() > 0);
    let AimPhase::Over { new_best, previous_best, .. } = &app.aim.phase else {
        panic!("over");
    };
    assert!(*new_best);
    assert_eq!(previous_best.map(|run| run.score), Some(1));
    assert_eq!(app.state.aim_trainer.best.map(|run| run.score), Some(2));

    app.state.aim_trainer.best = Some(best(5));
    let _ = app.update(aim(AimMessage::Start));
    let _ = app.update(aim(AimMessage::Stop));
    let AimPhase::Over { new_best, .. } = &app.aim.phase else {
        panic!("over");
    };
    assert!(!*new_best);
    assert_eq!(app.state.aim_trainer.best.map(|run| run.score), Some(5));
}

#[test]
fn aim_run_pauses_on_focus_loss_and_resumes() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));
    aim_frames(&mut app, 0);
    aim_hit_newest(&mut app);

    let task = app.update(aim(AimMessage::FocusLost));
    assert!(task.units() > 0, "releases the cursor");
    assert!(matches!(app.aim.phase, AimPhase::Paused(_)));
    let _ = app.update(aim(AimMessage::SensitivityChanged("2".into())));
    assert_eq!(app.state.aim_trainer.sensitivity, Some(0.34), "read-only while paused");

    let _ = app.update(aim(AimMessage::Start));
    let AimPhase::Playing(game) = &app.aim.phase else {
        panic!("resumed");
    };
    assert_eq!(game.score(), 1, "resuming keeps the run");
}

#[test]
fn aim_run_ends_when_leaving_the_tab_and_pauses_when_closing() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));
    let _ = app.update(Message::TabSelected(Tab::Settings));
    assert!(matches!(app.aim.phase, AimPhase::Over { .. }));

    let _ = app.update(Message::TabSelected(Tab::AimTrainer));
    let _ = app.update(aim(AimMessage::Start));
    let _ = app.update(Message::CloseRequested(iced::window::Id::unique()));
    assert!(matches!(app.aim.phase, AimPhase::Paused(_)), "closing releases the cursor first");
}

#[test]
fn aim_arena_of_zero_size_is_ignored() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::TabSelected(Tab::AimTrainer));
    let _ = app.update(aim(AimMessage::SensitivityChanged("0.34".into())));
    let _ = app.update(aim(AimMessage::ArenaResized(iced::Size::ZERO)));
    assert!(!app.aim.can_start(), "no arena to play in yet");

    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));
    let before = app.aim.phase.game().expect("game").view();
    let _ = app.update(aim(AimMessage::ArenaResized(iced::Size::ZERO)));
    assert_eq!(app.aim.phase.game().expect("game").view(), before);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib ui::tests::aim`
Expected: FAIL to compile (`no variant AimTrainer`, `unresolved import super::aim`).

- [ ] **Step 3: Write `src/ui/aim.rs`**

```rust
//! The Aim Trainer tab's state and how `PrimeApp` handles its messages. The game is
//! `crate::aim_trainer`; raw mouse input and the cursor clip are `crate::raw_mouse`.

use std::time::{SystemTime, UNIX_EPOCH};

use iced::{Size, Subscription, Task, mouse, window};

use super::{Message, PrimeApp, Tab};
use crate::aim_trainer::{self, AimRun, Game, View};
use crate::raw_mouse;

#[derive(Clone, Debug)]
pub(super) enum AimMessage {
    SensitivityChanged(String),
    DpiChanged(String),
    MonitorMeasured(Option<f32>),
    ArenaResized(Size),
    /// Start from Ready, Resume from Paused, Play again from Game over.
    Start,
    /// Esc during a run: end it and show its results.
    Stop,
    /// Game over's Done: back to Ready.
    Done,
    MouseMoved(i32, i32),
    Clicked,
    Frame(iced::time::Instant),
    FocusLost,
}

#[derive(Debug, Default)]
pub(super) struct AimTrainerTab {
    pub(super) phase: AimPhase,
    pub(super) sensitivity_input: String,
    pub(super) dpi_input: String,
    pub(super) monitor_width: Option<f32>,
    pub(super) arena: Option<Size>,
    pub(super) last_frame: Option<iced::time::Instant>,
}

#[derive(Debug, Default)]
pub(super) enum AimPhase {
    #[default]
    Ready,
    Playing(Box<Game>),
    /// Prime lost focus mid-run: the clock is stopped and the cursor released.
    Paused(Box<Game>),
    Over {
        game: Box<Game>,
        run: AimRun,
        previous_best: Option<AimRun>,
        new_best: bool,
    },
}

impl AimPhase {
    pub(super) fn game(&self) -> Option<&Game> {
        match self {
            Self::Ready => None,
            Self::Playing(game) | Self::Paused(game) | Self::Over { game, .. } => Some(game),
        }
    }

    pub(super) fn is_running(&self) -> bool {
        matches!(self, Self::Playing(_) | Self::Paused(_))
    }
}

impl AimTrainerTab {
    pub(super) fn sensitivity(&self) -> Option<f64> {
        aim_trainer::parse_sensitivity(&self.sensitivity_input)
    }

    pub(super) fn view(&self) -> Option<View> {
        let arena = self.arena?;
        let monitor = self
            .monitor_width
            .unwrap_or(aim_trainer::FALLBACK_MONITOR_WIDTH);
        View::new(monitor, arena.width, arena.height)
    }

    pub(super) fn can_start(&self) -> bool {
        !matches!(self.phase, AimPhase::Playing(_))
            && self.sensitivity().is_some()
            && self.view().is_some()
    }
}

impl PrimeApp {
    pub(super) fn update_aim(&mut self, message: AimMessage) -> Task<Message> {
        match message {
            AimMessage::SensitivityChanged(input) => {
                if self.aim.phase.is_running() {
                    return Task::none();
                }
                let value = aim_trainer::parse_sensitivity(&input);
                self.aim.sensitivity_input = input;
                match value {
                    Some(value) if self.state.aim_trainer.sensitivity != Some(value) => {
                        self.state.aim_trainer.sensitivity = Some(value);
                        self.save_task()
                    }
                    _ => Task::none(),
                }
            }
            AimMessage::DpiChanged(input) => {
                if self.aim.phase.is_running() {
                    return Task::none();
                }
                // The default isn't written.
                let dpi = aim_trainer::parse_dpi(&input)
                    .map(|dpi| (dpi != aim_trainer::DEFAULT_DPI).then_some(dpi));
                self.aim.dpi_input = input;
                match dpi {
                    Some(dpi) if self.state.aim_trainer.dpi != dpi => {
                        self.state.aim_trainer.dpi = dpi;
                        self.save_task()
                    }
                    _ => Task::none(),
                }
            }
            AimMessage::MonitorMeasured(width) => {
                self.aim.monitor_width = width;
                self.refresh_aim_view();
                Task::none()
            }
            AimMessage::ArenaResized(size) => {
                self.aim.arena = Some(size);
                self.refresh_aim_view();
                Task::none()
            }
            AimMessage::Start => self.start_aim_run(),
            AimMessage::Stop => self.end_aim_run(),
            AimMessage::Done => {
                if matches!(self.aim.phase, AimPhase::Over { .. }) {
                    self.aim.phase = AimPhase::Ready;
                }
                Task::none()
            }
            AimMessage::MouseMoved(dx, dy) => {
                if let AimPhase::Playing(game) = &mut self.aim.phase {
                    game.move_aim(dx, dy);
                }
                Task::none()
            }
            AimMessage::Clicked => {
                let AimPhase::Playing(game) = &mut self.aim.phase else {
                    return Task::none();
                };
                game.click();
                if game.is_over() {
                    self.end_aim_run()
                } else {
                    Task::none()
                }
            }
            AimMessage::Frame(now) => {
                let AimPhase::Playing(game) = &mut self.aim.phase else {
                    return Task::none();
                };
                let elapsed = self.aim.last_frame.map_or(0, |last| {
                    u64::try_from(now.saturating_duration_since(last).as_millis())
                        .unwrap_or(u64::MAX)
                });
                self.aim.last_frame = Some(now);
                game.advance(elapsed);
                if game.is_over() {
                    self.end_aim_run()
                } else {
                    Task::none()
                }
            }
            AimMessage::FocusLost => self.pause_aim_run(),
        }
    }

    /// Opening the tab: the fields show the saved values, and the monitor is measured again in
    /// case the window moved.
    pub(super) fn open_aim_trainer(&mut self) -> Task<Message> {
        if !self.aim.phase.is_running() {
            self.aim.sensitivity_input = self
                .state
                .aim_trainer
                .sensitivity
                .map(|value| value.to_string())
                .unwrap_or_default();
            self.aim.dpi_input = self.state.aim_trainer.dpi().to_string();
        }
        window::latest().then(|id| {
            id.map_or_else(Task::none, |id| {
                window::monitor_size(id).map(|size| {
                    Message::Aim(AimMessage::MonitorMeasured(size.map(|size| size.width)))
                })
            })
        })
    }

    fn refresh_aim_view(&mut self) {
        let Some(view) = self.aim.view() else {
            return;
        };
        if let AimPhase::Playing(game) | AimPhase::Paused(game) = &mut self.aim.phase {
            game.set_view(view);
        }
    }

    fn start_aim_run(&mut self) -> Task<Message> {
        if !self.aim.can_start() {
            return Task::none();
        }
        let (Some(view), Some(sensitivity)) = (self.aim.view(), self.aim.sensitivity()) else {
            return Task::none();
        };
        self.aim.phase = match std::mem::take(&mut self.aim.phase) {
            AimPhase::Paused(game) => AimPhase::Playing(game),
            _ => AimPhase::Playing(Box::new(Game::new(sensitivity, view, seed()))),
        };
        self.aim.last_frame = None;
        Task::future(async { raw_mouse::hold_cursor() }).discard()
    }

    /// Esc, game over or leaving the tab: show the results and keep a new best.
    pub(super) fn end_aim_run(&mut self) -> Task<Message> {
        let game = match std::mem::take(&mut self.aim.phase) {
            AimPhase::Playing(game) | AimPhase::Paused(game) => game,
            other => {
                self.aim.phase = other;
                return Task::none();
            }
        };
        let run = AimRun::new(
            game.results(),
            game.sensitivity(),
            self.state.aim_trainer.dpi(),
            unix_now(),
        );
        let previous_best = self.state.aim_trainer.best;
        let new_best = run.score > previous_best.map_or(0, |best| best.score);
        let save = if new_best {
            self.state.aim_trainer.best = Some(run);
            self.save_task()
        } else {
            Task::none()
        };
        self.aim.last_frame = None;
        self.aim.phase = AimPhase::Over {
            game,
            run,
            previous_best,
            new_best,
        };
        Task::batch([release_cursor_task(), save])
    }

    /// Focus loss or closing the window: stop the clock and give the cursor back.
    pub(super) fn pause_aim_run(&mut self) -> Task<Message> {
        if !matches!(self.aim.phase, AimPhase::Playing(_)) {
            return Task::none();
        }
        if let AimPhase::Playing(game) = std::mem::take(&mut self.aim.phase) {
            self.aim.phase = AimPhase::Paused(game);
        }
        self.aim.last_frame = None;
        release_cursor_task()
    }
}

/// Raw mouse counts, frames and clicks, only while a run plays on screen.
pub(super) fn subscription(app: &PrimeApp) -> Option<Subscription<Message>> {
    let playing = matches!(app.aim.phase, AimPhase::Playing(_));
    (app.active_tab == Tab::AimTrainer && playing).then(|| {
        Subscription::batch([
            Subscription::run(raw_mouse::movements)
                .map(|(dx, dy)| Message::Aim(AimMessage::MouseMoved(dx, dy))),
            window::frames().map(|now| Message::Aim(AimMessage::Frame(now))),
            iced::event::listen_with(run_event),
        ])
    })
}

fn run_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            Some(Message::Aim(AimMessage::Clicked))
        }
        iced::Event::Window(window::Event::Unfocused) => Some(Message::Aim(AimMessage::FocusLost)),
        _ => None,
    }
}

fn release_cursor_task() -> Task<Message> {
    Task::future(async { raw_mouse::release_cursor() }).discard()
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |elapsed| elapsed.subsec_nanos().into())
        ^ unix_now().unsigned_abs()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
}
```

- [ ] **Step 4: Wire it into `src/ui/mod.rs`**

- Add `mod aim;` beside the other `mod` lines, and `use aim::{AimMessage, AimTrainerTab};`.
- `enum Tab`: add `AimTrainer,` after `Loadout,`. `Display`: `Tab::AimTrainer =>
  f.write_str("Aim Trainer"),`.
- `TabScrollOffsets`: add an `aim_trainer: AbsoluteOffset` field and its `get`/`set` arms.
- `enum Message`: add `Aim(AimMessage),` after `TabSelected(Tab),`.
- `struct PrimeApp`: add `aim: AimTrainerTab,` (with a `/// The Aim Trainer tab's fields and run.`
  doc line, like its neighbours).
- `app_subscription`: before `Subscription::batch(subscriptions)`:

```rust
    if let Some(aim) = aim::subscription(app) {
        subscriptions.push(aim);
    }
```

- [ ] **Step 5: Wire it into `src/ui/app.rs`**

- `fn save_task` becomes `pub(super) fn save_task`, since `src/ui/aim.rs` saves through it.
- Boot's `PrimeApp { .. }` literal: `aim: AimTrainerTab::default(),` (import it from `super::aim`
  alongside the other `super::` imports, together with `AimMessage` and `AimPhase`).
- The update `match`, after `Message::TabSelected`: `Message::Aim(message) => self.update_aim(message),`
- `Message::TabSelected(tab)`: first line of the arm:
  `let ended_run = if tab == Tab::AimTrainer { Task::none() } else { self.end_aim_run() };`
  and add `ended_run` as the first item of the arm's `Task::batch([...])`.
- `escape_message`: first lines of the function:

```rust
        if self.aim.phase.is_running() {
            return Some(Message::Aim(AimMessage::Stop));
        }
```

- `Message::CloseRequested(id)`: first lines of the arm:

```rust
                // A run holds the cursor on one pixel; pausing gives it back first.
                if matches!(self.aim.phase, AimPhase::Playing(_)) {
                    let release = self.pause_aim_run();
                    return Task::batch([release, self.handle_message(Message::CloseRequested(id))]);
                }
```

- `load_active_tab`: an arm before the final `_ => Task::none(),`:
  `Tab::AimTrainer => self.open_aim_trainer(),`
- The match near `start_account_launch` (`Tab::Shop | Tab::Loadout | Tab::LiveMatch => ...`): add
  `Tab::AimTrainer` to the `Tab::Accounts | Tab::Settings => Task::none()` arm. It shows no
  account data.

- [ ] **Step 6: Make the remaining `Tab` matches compile**

Run `cargo build`; each error is an exhaustive `match` on `Tab`. Expected ones:
- `src/ui/screens/mod.rs` `tab`: `Tab::AimTrainer => aim_trainer::tab(app),` — for this task,
  point it at a stub so it builds: add `mod aim_trainer;` and create
  `src/ui/screens/aim_trainer.rs` with

```rust
use iced::Element;

use crate::ui::{Message, PrimeApp};

pub(super) fn tab(_app: &PrimeApp) -> Element<'_, Message> {
    iced::widget::Space::new().into()
}
```

  Task 5 replaces it.
- `fills_page`: `Tab::AimTrainer => true,` (the arena fills the page; nothing scrolls).
- `src/ui/shell.rs` `header_gap`: `Tab::AimTrainer => 20.0,`; `tab_button`'s icon match:
  `Tab::AimTrainer => theme::Icon::Crosshair,`.

- [ ] **Step 7: Run the tests and clippy**

Run: `cargo test` then `cargo clippy --all-targets`
Expected: all tests pass, including the 8 `aim_` UI tests; clippy clean.

- [ ] **Step 8: Commit**

```bash
git add src/ui
git commit -m "Run Aim Trainer rounds: raw mouse, frames, pause, Esc and best run"
```

### Task 5: Aim Trainer view, nav item and docs

**Files:**
- Modify: `src/ui/screens/aim_trainer.rs` (replace the stub), `src/ui/shell.rs` (nav item),
  `src/ui/theme.rs` (`Icon::Trophy`), `AGENTS.md`
- Create: `assets/icons/trophy.svg`

**Interfaces:**
- Consumes: Task 4's `AimTrainerTab`, `AimPhase`, `AimMessage`; Task 1's `View`,
  `cm_per_360`, `HORIZONTAL_FOV_DEGREES`, `AimRun`.

The design is the source of truth: export the three frames at scale 1 and match spacing, sizes and
colours to them (Pencil MCP: `Export(["mqoBa","kw664","eHyAg"], "png", <scratchpad>, {scale: 1})`;
read exact values from the nodes with `Get`). Keep everything the design shows.

- [ ] **Step 1: Add the trophy icon**

Run: `curl -sL https://unpkg.com/lucide-static@1.48.0/icons/trophy.svg -o assets/icons/trophy.svg`
Check it starts with `<!-- @license lucide-static v1.48.0 - ISC -->` like `crosshair.svg`.
In `src/ui/theme.rs` add `Trophy,` to `enum Icon` (alphabetical among the others) and
`Icon::Trophy => include_bytes!("../../assets/icons/trophy.svg"),` to `svg()`.

- [ ] **Step 2: Add the nav item** in `src/ui/shell.rs`'s sidebar `nav` column, between Loadout and
  Settings: `self.tab_button(Tab::AimTrainer),`

- [ ] **Step 3: Write the view** — replace `src/ui/screens/aim_trainer.rs`:

```rust
//! The Aim Trainer tab: score, accuracy, best and health over a fixed-size arena, with the setup
//! and results as modals inside it.

use iced::widget::{canvas, column, container, row, space, stack};
use iced::{
    Border, Color, Element, Length, Point, Rectangle, Renderer, Shadow, Size, Theme, Vector,
    alignment, mouse,
};
use time::{OffsetDateTime, UtcOffset};

use crate::aim_trainer::{self, AimRun};
use crate::ui::aim::{AimMessage, AimPhase, AimTrainerTab};
use crate::ui::theme::{self, Icon, text};
use crate::ui::{Message, PrimeApp};

const HEALTH_BAR_WIDTH: f32 = 260.0;

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    column![hud(app), arena(app)].spacing(20).into()
}

fn hud(app: &PrimeApp) -> Element<'_, Message> {
    let game = app.aim.phase.game();
    let score = game.map_or(0, |game| game.score());
    let accuracy = game.and_then(|game| game.results().accuracy());
    let health = game.map_or(100, |game| game.health());
    let best = app.state.aim_trainer.best.map(|run| run.score);

    let health_bar = column![
        row![
            text("Health").size(11).font(theme::SEMIBOLD_FONT).color(theme::MUTED),
            space().width(Length::Fill),
            text(format!("{health}%"))
                .size(11)
                .font(theme::MONO_FONT)
                .line_height(theme::MONO_LINE_HEIGHT)
                .color(theme::MUTED),
        ],
        container(
            container(space())
                .width(HEALTH_BAR_WIDTH * health as f32 / 100.0)
                .height(8)
                .style(|_| filled(theme::ACCENT, 4.0)),
        )
        .width(HEALTH_BAR_WIDTH)
        .height(8)
        .style(|_| filled(theme::RAISED, 4.0)),
    ]
    .spacing(6)
    .width(HEALTH_BAR_WIDTH);

    row![
        stat("Score", score.to_string(), theme::TEXT),
        stat("Accuracy", percent(accuracy), theme::TEXT),
        stat("Best", best.map_or("—".into(), |best| best.to_string()), theme::MUTED),
        space().width(Length::Fill),
        health_bar,
    ]
    .spacing(32)
    .align_y(alignment::Vertical::Bottom)
    .into()
}

fn stat<'a>(label: &'a str, value: String, color: Color) -> Element<'a, Message> {
    column![
        text(label).size(11).font(theme::SEMIBOLD_FONT).color(theme::MUTED),
        text(value)
            .size(22)
            .font(theme::DISPLAY_FONT)
            .line_height(theme::DISPLAY_LINE_HEIGHT)
            .color(color),
    ]
    .spacing(2)
    .into()
}

fn arena(app: &PrimeApp) -> Element<'_, Message> {
    let mut layers = stack![
        canvas(Arena { tab: &app.aim })
            .width(Length::Fill)
            .height(Length::Fill)
    ];
    match &app.aim.phase {
        AimPhase::Playing(_) => {
            layers = layers.push(
                container(
                    text("Esc to stop")
                        .size(11)
                        .font(theme::MONO_FONT)
                        .line_height(theme::MONO_LINE_HEIGHT)
                        .color(theme::FAINT),
                )
                .padding(16),
            );
        }
        AimPhase::Ready => layers = layers.push(modal(setup_card(app, false))),
        AimPhase::Paused(_) => layers = layers.push(modal(setup_card(app, true))),
        AimPhase::Over {
            run,
            previous_best,
            new_best,
            ..
        } => layers = layers.push(modal(results_card(run, previous_best.as_ref(), *new_best))),
    }

    container(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(|_| container::Style {
            background: Some(theme::SURFACE.into()),
            border: Border {
                color: theme::LINE,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// A card centred over the dimmed arena.
fn modal(card: Element<'_, Message>) -> Element<'_, Message> {
    container(card)
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Color { a: 0.6, ..theme::BG }.into()),
            ..container::Style::default()
        })
        .into()
}

fn card_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::LINE,
            width: 1.0,
            radius: 14.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
            offset: Vector::new(0.0, 16.0),
            blur_radius: 48.0,
        },
        ..container::Style::default()
    }
}

fn setup_card(app: &PrimeApp, paused: bool) -> Element<'_, Message> {
    let tab = &app.aim;
    let sensitivity = tab.sensitivity();
    let dpi = aim_trainer::parse_dpi(&tab.dpi_input);

    let mut sensitivity_input =
        theme::text_input("e.g. 0.35", &tab.sensitivity_input).font(theme::MONO_FONT);
    let mut dpi_input = theme::text_input("800", &tab.dpi_input).font(theme::MONO_FONT);
    // A run keeps the sensitivity it started with, so the fields are read-only while paused.
    if !paused {
        sensitivity_input = sensitivity_input
            .on_input(|value| Message::Aim(AimMessage::SensitivityChanged(value)));
        dpi_input = dpi_input.on_input(|value| Message::Aim(AimMessage::DpiChanged(value)));
    }

    let (edpi, cm) = match (sensitivity, dpi) {
        (Some(sensitivity), Some(dpi)) => (
            format!("{}", (sensitivity * f64::from(dpi)).round()),
            format!("{:.1} cm", aim_trainer::cm_per_360(sensitivity, dpi)),
        ),
        _ => ("—".to_string(), "—".to_string()),
    };

    let start_label = if paused { "Resume" } else { "Start" };
    let start = theme::button(theme::icon_label(Icon::Play, start_label, theme::BG))
        .style(theme::danger_button_style)
        .padding([9, 16])
        .on_press_maybe(tab.can_start().then_some(Message::Aim(AimMessage::Start)));

    let mut card = column![
        column![
            text(if paused { "Paused" } else { "Ready" })
                .size(20)
                .font(theme::DISPLAY_FONT)
                .line_height(theme::DISPLAY_LINE_HEIGHT),
            text("Pop the dots before they grow. Your crosshair moves at your VALORANT sensitivity.")
                .size(13)
                .color(theme::MUTED),
        ]
        .spacing(4),
        row![
            field("VALORANT sensitivity", sensitivity_input.into(), Length::Fill),
            field("Mouse DPI", dpi_input.into(), Length::Fixed(130.0)),
        ]
        .spacing(12),
        row![
            tile("eDPI", edpi),
            tile("cm / 360°", cm),
            tile("FOV", format!("{}°", aim_trainer::HORIZONTAL_FOV_DEGREES)),
        ]
        .spacing(8),
    ]
    .spacing(18);

    if let Some(best) = &app.state.aim_trainer.best {
        card = card.push(
            container(
                row![
                    theme::icon(Icon::Trophy, 15.0, theme::GOLD),
                    text(format!("Best {}", best.score)).size(13).font(theme::SEMIBOLD_FONT),
                    text(best_detail(best)).size(12).color(theme::MUTED),
                ]
                .spacing(10)
                .align_y(alignment::Vertical::Center),
            )
            .width(Length::Fill)
            .padding([12, 14])
            .style(|_| container::Style {
                border: Border {
                    color: theme::LINE,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..container::Style::default()
            }),
        );
    }

    card = card.push(
        row![
            row![
                theme::icon(Icon::Mouse, 13.0, theme::FAINT),
                text("Raw input · Esc to stop").size(11).color(theme::FAINT),
            ]
            .spacing(6)
            .align_y(alignment::Vertical::Center),
            space().width(Length::Fill),
            start,
        ]
        .align_y(alignment::Vertical::Center),
    );

    container(card).width(460).padding(24).style(card_style).into()
}

fn results_card<'a>(
    run: &AimRun,
    previous_best: Option<&AimRun>,
    new_best: bool,
) -> Element<'a, Message> {
    let mut top = row![
        text("Game over")
            .size(20)
            .font(theme::DISPLAY_FONT)
            .line_height(theme::DISPLAY_LINE_HEIGHT),
        space().width(Length::Fill),
    ]
    .align_y(alignment::Vertical::Center);
    if new_best {
        top = top.push(
            container(
                row![
                    theme::icon(Icon::Trophy, 13.0, theme::GOLD),
                    text("New best").size(12).font(theme::SEMIBOLD_FONT).color(theme::GOLD),
                ]
                .spacing(6)
                .align_y(alignment::Vertical::Center),
            )
            .padding([4, 9])
            .style(|_| filled(Color { a: 0.12, ..theme::GOLD }, 6.0)),
        );
    }

    let compared = match previous_best {
        Some(previous) if new_best => format!("dots · previous best {}", previous.score),
        Some(best) => format!("dots · best {}", best.score),
        None => "dots".to_string(),
    };

    let note = format!(
        "At {} sens · {} DPI ({:.1} cm/360°)",
        run.sensitivity,
        run.dpi,
        aim_trainer::cm_per_360(run.sensitivity, run.dpi)
    );

    let actions = row![
        space().width(Length::Fill),
        theme::button(text("Done").size(13).font(theme::BOLD_FONT))
            .padding([9, 14])
            .on_press(Message::Aim(AimMessage::Done)),
        theme::button(theme::icon_label(Icon::RotateCcw, "Play again", theme::BG))
            .style(theme::danger_button_style)
            .padding([9, 14])
            .on_press(Message::Aim(AimMessage::Start)),
    ]
    .spacing(8);

    container(
        column![
            top,
            row![
                text(run.score.to_string())
                    .size(56)
                    .font(theme::DISPLAY_FONT)
                    .line_height(1.0),
                text(compared).size(13).color(theme::MUTED),
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Bottom),
            row![
                tile("Hits", run.score.to_string()),
                tile("Misses", run.misses.to_string()),
                tile("Burst", run.bursts.to_string()),
            ]
            .spacing(8),
            row![
                tile("Accuracy", percent(run.accuracy())),
                tile("Avg reaction", reaction(run.avg_reaction_ms)),
                tile("Time", clock(run.duration_ms)),
            ]
            .spacing(8),
            text(note).size(11).color(theme::FAINT),
            actions,
        ]
        .spacing(18),
    )
    .width(440)
    .padding(24)
    .style(card_style)
    .into()
}

fn field<'a>(label: &'a str, input: Element<'a, Message>, width: Length) -> Element<'a, Message> {
    column![
        text(label).size(12).font(theme::SEMIBOLD_FONT).color(theme::MUTED),
        input
    ]
    .spacing(6)
    .width(width)
    .into()
}

fn tile<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    container(
        column![
            text(label).size(11).font(theme::SEMIBOLD_FONT).color(theme::MUTED),
            text(value)
                .size(14)
                .font(theme::MONO_SEMIBOLD_FONT)
                .line_height(theme::MONO_LINE_HEIGHT),
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .padding([10, 12])
    .style(|_| filled(theme::RAISED, 8.0))
    .into()
}

fn filled(color: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(color.into()),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

fn percent(accuracy: Option<f64>) -> String {
    accuracy.map_or("—".to_string(), |accuracy| format!("{:.0}%", accuracy * 100.0))
}

fn reaction(ms: Option<u32>) -> String {
    ms.map_or("—".to_string(), |ms| format!("{ms} ms"))
}

fn clock(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn best_detail(best: &AimRun) -> String {
    let date = OffsetDateTime::from_unix_timestamp(best.finished_at_unix)
        .map(|at| at.to_offset(UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)))
        .map(|at| format!("{} {}", &at.month().to_string()[..3], at.day()))
        .unwrap_or_default();
    format!(
        "{} accuracy · {} · {date}",
        percent(best.accuracy()),
        reaction(best.avg_reaction_ms)
    )
}

/// Draws the dots and the crosshair, hides the cursor during a run, and reports the arena's size,
/// which sets how much of VALORANT's view fits.
struct Arena<'a> {
    tab: &'a AimTrainerTab,
}

impl canvas::Program<Message> for Arena<'_> {
    type State = Option<Size>;

    fn update(
        &self,
        state: &mut Self::State,
        _event: &iced::Event,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        (*state != Some(size)).then(|| {
            *state = Some(size);
            canvas::Action::publish(Message::Aim(AimMessage::ArenaResized(size)))
        })
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let Some(game) = self.tab.phase.game() else {
            return vec![frame.into_geometry()];
        };
        let view = game.view();
        let playing = matches!(self.tab.phase, AimPhase::Playing(_));
        // Behind a modal the run's last dots stay, faded.
        let color = if playing {
            theme::ACCENT
        } else {
            Color { a: 0.25, ..theme::ACCENT }
        };
        for (dot, diameter) in game.dots() {
            let (x, y) = view.to_px(dot.yaw, dot.pitch);
            let radius = view.radius_px(dot.yaw, dot.pitch, diameter);
            frame.fill(
                &canvas::Path::circle(Point::new(x as f32, y as f32), radius as f32),
                color,
            );
        }
        if playing {
            let (x, y) = view.to_px(game.aim().0, game.aim().1);
            draw_crosshair(&mut frame, Point::new(x as f32, y as f32));
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if matches!(self.tab.phase, AimPhase::Playing(_)) {
            mouse::Interaction::Hidden
        } else {
            mouse::Interaction::default()
        }
    }
}

/// VALORANT's default look: four white 6×2 arms 4px out and a centre dot, each with a black
/// 1px outline so it shows over a dot.
fn draw_crosshair(frame: &mut canvas::Frame, center: Point) {
    let parts = [
        (-10.0, -1.0, 6.0, 2.0),
        (4.0, -1.0, 6.0, 2.0),
        (-1.0, -10.0, 2.0, 6.0),
        (-1.0, 4.0, 2.0, 6.0),
        (-1.0, -1.0, 2.0, 2.0),
    ];
    for (color, grow) in [(Color::BLACK, 1.0), (Color::WHITE, 0.0)] {
        for (x, y, width, height) in parts {
            frame.fill_rectangle(
                Point::new(center.x + x - grow, center.y + y - grow),
                Size::new(width + grow * 2.0, height + grow * 2.0),
                color,
            );
        }
    }
}
```

If an iced name differs in 0.14 (for example `canvas::Action::publish`, `container::center`,
`on_press_maybe` or `Text::line_height(1.0)`), check
`~/.cargo/registry/src/*/iced_widget-0.14*/src/` and use that version's spelling; don't change the
behaviour.

- [ ] **Step 4: Build, test, lint**

Run: `cargo test` then `cargo clippy --all-targets`
Expected: PASS, clean.

- [ ] **Step 5: Check against the design and by hand** (ask the user before `cargo run`; it uses
  the real `accounts.json`, but this tab only writes `aim_trainer` to it)

With `cargo run`, open Aim Trainer and compare each state to its frame at native size (see the
`pencil-fullres-compare` approach: export at scale 1, screenshot Prime, crop the same box):
1. Ready (`mqoBa`): fields, tiles, best row and Start match; Start is disabled with an empty or
   invalid sensitivity.
2. Playing (`kw664`): the cursor is hidden, the crosshair moves, dots grow and burst, health drops.
3. Game over (`eHyAg`): results, New best badge, Done and Play again.
4. Esc ends the run; Alt+Tab pauses it and the cursor is free; switching tabs ends it.
5. Raw input: set Windows pointer speed to 10/11 and turn Enhance pointer precision on. At the same
   sensitivity, a mouse sweep that turns 360° in VALORANT should move the crosshair from one arena
   edge to the other in the expected number of counts (use `counts_per_360`: with the arena showing
   ±32.6°, edge to edge is 65.2/360 of a full turn). Put the Windows settings back afterwards.

- [ ] **Step 6: Document it in `AGENTS.md`**

Under "What the app does", after the Loadout tab paragraph:

```markdown
Aim Trainer tab: WAIUA-style red dots that grow until they burst (health 100, a burst costs 15, a
miss 5). The crosshair turns `0.07° × sens` per raw mouse count, read with Windows Raw Input
(`src/raw_mouse.rs`) because iced drops winit's raw device events, so Windows pointer speed and
acceleration don't matter. Dots sit where they would at VALORANT's 103° FOV on this monitor. During
a run the cursor is hidden and held on one pixel; Esc, leaving the tab or game over end the run,
focus loss pauses it. Sensitivity, DPI and one best run for the whole app are saved in
`accounts.json` (`aim_trainer`).
```

Add `aim_trainer.rs` and `raw_mouse.rs` to the "Outside the UI" list in "Code layout".

- [ ] **Step 7: Commit**

```bash
git add assets/icons/trophy.svg src/ui AGENTS.md
git commit -m "Show the Aim Trainer tab: HUD, arena, setup and results modals"
```
