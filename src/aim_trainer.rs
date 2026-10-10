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
