//! The Aim Trainer's game: dots that grow until they burst, aimed at VALORANT's
//! sensitivity or with the Windows cursor. Dots sit at fixed directions in the world and are drawn
//! through VALORANT's 103° perspective; at VALORANT's sensitivity the view turns under a fixed
//! crosshair, as in game. Angles are degrees (yaw right, pitch down), times are milliseconds.
//! Nothing here knows about iced or Windows.

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
/// A dot's width when it bursts.
const DOT_MAX_DEGREES: f64 = 3.6;
const DOT_LIFETIME_MS: u64 = 2_200;
/// A longer frame gap (a stall, a debugger) counts as this long, so it can't burst every dot.
const MAX_STEP_MS: u64 = 100;

/// How fast dots come.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    /// Up to 4 dots at once.
    Easy,
    /// Up to 6 dots at once.
    #[default]
    Normal,
    /// Up to 11 dots at once.
    Hard,
}

impl Difficulty {
    /// How long until the next dot, stepping down as the score climbs.
    pub fn spawn_interval_ms(self, score: u32) -> u64 {
        let steps = match self {
            Self::Easy => [1_000, 850, 700, 600, 550],
            Self::Normal => [800, 700, 600, 500, 400],
            Self::Hard => [600, 500, 400, 300, 200],
        };
        steps[match score {
            0..5 => 0,
            5..20 => 1,
            20..35 => 2,
            35..50 => 3,
            _ => 4,
        }]
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

/// A unit direction in the world: x right, y down, z ahead at yaw and pitch zero.
type Direction = [f64; 3];

fn dot3(a: Direction, b: Direction) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The direction of a yaw and pitch in degrees, turned the way VALORANT turns: yaw right, pitch
/// down.
fn direction(yaw: f64, pitch: f64) -> Direction {
    let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
    [
        pitch.cos() * yaw.sin(),
        pitch.sin(),
        pitch.cos() * yaw.cos(),
    ]
}

/// The yaw and pitch of a direction, in degrees.
fn angles([x, y, z]: Direction) -> (f64, f64) {
    (
        x.atan2(z).to_degrees(),
        y.clamp(-1.0, 1.0).asin().to_degrees(),
    )
}

/// Where the view looks, in degrees: yaw right, pitch down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera {
    pub yaw: f64,
    pub pitch: f64,
}

/// VALORANT stops looking up or down just short of straight.
const MAX_PITCH_DEGREES: f64 = 89.0;

/// A camera's ahead, right and down, in the world.
type Axes = [Direction; 3];

impl Camera {
    fn axes(&self) -> Axes {
        let yaw = self.yaw.to_radians();
        [
            direction(self.yaw, self.pitch),
            [yaw.cos(), 0.0, -yaw.sin()],
            direction(self.yaw, self.pitch + 90.0),
        ]
    }
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

    /// The arena's centre, where VALORANT's crosshair sits.
    pub fn centre(&self) -> (f64, f64) {
        (self.width_px / 2.0, self.height_px / 2.0)
    }

    /// Where a direction lands, in pixels from the arena's top-left, or `None` behind the view.
    fn project(&self, [ahead, right, down]: Axes, direction: Direction) -> Option<(f64, f64)> {
        let depth = dot3(ahead, direction);
        (depth > 1e-6).then(|| {
            (
                self.width_px / 2.0 + self.focal_px * dot3(right, direction) / depth,
                self.height_px / 2.0 + self.focal_px * dot3(down, direction) / depth,
            )
        })
    }

    /// The direction under a pixel of the arena.
    #[cfg(test)]
    fn direction_at(&self, [ahead, right, down]: Axes, x: f64, y: f64) -> Direction {
        let (x, y) = (
            (x - self.width_px / 2.0) / self.focal_px,
            (y - self.height_px / 2.0) / self.focal_px,
        );
        let ray: Direction = std::array::from_fn(|i| ahead[i] + x * right[i] + y * down[i]);
        let length = dot3(ray, ray).sqrt();
        ray.map(|axis| axis / length)
    }

    /// A dot's drawn radius: the mean of its projected width and height, so dots away from the
    /// centre stretch the way they do in game.
    fn radius_px(&self, [ahead, ..]: Axes, direction: Direction, diameter: f64) -> f64 {
        let off_centre = dot3(ahead, direction).clamp(-1.0, 1.0).acos();
        let half = (diameter / 2.0).to_radians();
        let outer = (off_centre + half).min(89_f64.to_radians());
        let along = (outer.tan() - (off_centre - half).tan()) / 2.0;
        let across = half.tan() / off_centre.cos();
        self.focal_px * (along + across) / 2.0
    }

    #[cfg(test)]
    fn contains(&self, (x, y): (f64, f64)) -> bool {
        (0.0..=self.width_px).contains(&x) && (0.0..=self.height_px).contains(&y)
    }
}

/// A dot: a fixed direction in the world, so it stays put while the view turns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    direction: Direction,
    spawned_at_ms: u64,
}

impl Dot {
    /// The yaw and pitch that look straight at it.
    pub fn angles(&self) -> (f64, f64) {
        angles(self.direction)
    }
}

/// Grid lines across each wall, the floor and the ceiling of the room around the view.
const ROOM_DIVISIONS: u32 = 8;
/// How far in front of the view, as a share of the room's half width, a room line is cut off.
const ROOM_NEAR: f64 = 0.05;

/// A grid line of the room around the view, in pixels from the arena's top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoomLine {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub floor: bool,
}

/// Where a yaw and pitch from straight ahead meet the room's front wall.
fn front_wall(yaw: f64, pitch: f64) -> Direction {
    [yaw.to_radians().tan(), pitch.to_radians().tan(), 1.0]
}

/// The part of a room line at least `ROOM_NEAR` in front of the view, if any. Depth runs
/// straight along the line, so it's cut where it crosses `ROOM_NEAR`.
fn in_front(ahead: Direction, from: Direction, to: Direction) -> Option<(Direction, Direction)> {
    let (near_from, near_to) = (dot3(ahead, from), dot3(ahead, to));
    let cut = |behind: Direction, front: Direction, depth: f64, other: f64| -> Direction {
        let t = (ROOM_NEAR - depth) / (other - depth);
        std::array::from_fn(|i| behind[i] + (front[i] - behind[i]) * t)
    };
    match (near_from >= ROOM_NEAR, near_to >= ROOM_NEAR) {
        (false, false) => None,
        (false, true) => Some((cut(from, to, near_from, near_to), to)),
        (true, false) => Some((from, cut(to, from, near_to, near_from))),
        (true, true) => Some((from, to)),
    }
}

/// A dot as drawn now, in pixels from the arena's top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawnDot {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    /// Its radius when it bursts.
    pub burst_radius: f64,
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
    /// With a sensitivity, raw mouse counts aim at VALORANT's speed; without one, the Windows
    /// cursor's movement does, pointer speed and acceleration included.
    sensitivity: Option<f64>,
    mode: ViewMode,
    difficulty: Difficulty,
    /// Physical pixels per logical one, for the Windows cursor turning a 3D view.
    pixel_scale: f64,
    view: View,
    rng: Rng,
    now_ms: u64,
    next_spawn_ms: u64,
    camera: Camera,
    /// Where the crosshair points in 2D, as yaw and pitch from the view's centre.
    aim: (f64, f64),
    dots: Vec<Dot>,
    health: i32,
    score: u32,
    misses: u32,
    bursts: u32,
    reaction_total_ms: u64,
}

impl Game {
    pub fn new(
        sensitivity: Option<f64>,
        mode: ViewMode,
        difficulty: Difficulty,
        view: View,
        seed: u64,
    ) -> Self {
        Self {
            sensitivity,
            mode,
            difficulty,
            pixel_scale: 1.0,
            view,
            rng: Rng::new(seed),
            now_ms: 0,
            next_spawn_ms: 0,
            camera: Camera::default(),
            aim: (0.0, 0.0),
            dots: Vec::new(),
            health: START_HEALTH,
            score: 0,
            misses: 0,
            bursts: 0,
            reaction_total_ms: 0,
        }
    }

    pub fn sensitivity(&self) -> Option<f64> {
        self.sensitivity
    }

    pub fn sensitivity_mode(&self) -> SensitivityMode {
        match self.sensitivity {
            Some(_) => SensitivityMode::Valorant,
            None => SensitivityMode::Windows,
        }
    }

    /// How far a count turns the view or moves the crosshair, or `None` when the cursor aims
    /// itself.
    pub fn degrees_per_count(&self) -> Option<f64> {
        match self.sensitivity {
            Some(sensitivity) => Some(DEGREES_PER_COUNT * sensitivity),
            // A pixel turns the view by the angle of a pixel at the centre, so the view turns at
            // the cursor's desktop speed.
            None if self.mode == ViewMode::ThreeD => Some(
                (1.0 / (self.view.focal_px * self.pixel_scale))
                    .atan()
                    .to_degrees(),
            ),
            None => None,
        }
    }

    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    pub fn set_pixel_scale(&mut self, scale: f64) {
        if scale > 0.0 {
            self.pixel_scale = scale;
        }
    }

    /// The Windows cursor aims a 2D view itself, through `click_at`, and is its own crosshair.
    pub fn aims_with_cursor(&self) -> bool {
        self.sensitivity.is_none() && self.mode == ViewMode::TwoD
    }

    /// Where the crosshair is drawn, or `None` when the Windows cursor is the crosshair.
    pub fn crosshair(&self) -> Option<(f64, f64)> {
        if self.aims_with_cursor() {
            return None;
        }
        let (cx, cy) = self.view.centre();
        Some(match self.mode {
            ViewMode::ThreeD => (cx, cy),
            ViewMode::TwoD => (
                cx + self.view.focal_px * self.aim.0.to_radians().tan(),
                cy + self.view.focal_px * self.aim.1.to_radians().tan(),
            ),
        })
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// The arena changed size.
    pub fn set_view(&mut self, view: View) {
        self.view = view;
        self.aim_inside_view(self.aim.0, self.aim.1);
    }

    /// Mouse movement: raw counts at VALORANT's sensitivity, or the Windows cursor's physical
    /// pixels. It turns the view in 3D and moves the crosshair in 2D. Ignored when the cursor
    /// aims itself.
    pub fn move_aim(&mut self, dx: i32, dy: i32) {
        if self.is_over() {
            return;
        }
        let Some(per_count) = self.degrees_per_count() else {
            return;
        };
        let (dx, dy) = (f64::from(dx) * per_count, f64::from(dy) * per_count);
        match self.mode {
            ViewMode::ThreeD => {
                self.camera = Camera {
                    yaw: (self.camera.yaw + dx) % 360.0,
                    pitch: (self.camera.pitch + dy).clamp(-MAX_PITCH_DEGREES, MAX_PITCH_DEGREES),
                };
            }
            ViewMode::TwoD => self.aim_inside_view(self.aim.0 + dx, self.aim.1 + dy),
        }
    }

    /// Points the 2D crosshair at a yaw and pitch, stopped at the arena's edge. Each moves it
    /// along its own axis, so moving sideways keeps it level.
    fn aim_inside_view(&mut self, yaw: f64, pitch: f64) {
        let (half_yaw, half_pitch) = self.view.half_angles();
        self.aim = (
            yaw.clamp(-half_yaw, half_yaw),
            pitch.clamp(-half_pitch, half_pitch),
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
            self.next_spawn_ms = now + self.difficulty.spawn_interval_ms(self.score);
        }
    }

    /// Where dots appear, as the furthest yaw and pitch each way from straight ahead. In 3D it's
    /// fixed in front, half the view each way, so a dot never appears out of sight while the
    /// crosshair is on another; in 2D it's the whole view.
    fn spawn_zone(&self) -> (f64, f64) {
        let (hx, hy) = self.view.half_angles();
        match self.mode {
            ViewMode::TwoD => (hx, hy),
            ViewMode::ThreeD => {
                let half = |angle: f64| (angle.to_radians().tan() / 2.0).atan().to_degrees();
                (half(hx), half(hy))
            }
        }
    }

    /// A new dot somewhere in the spawn zone.
    fn spawn(&mut self) {
        let (zx, zy) = self.spawn_zone();
        let margin = DOT_MAX_DEGREES / 2.0;
        let (yaw_range, pitch_range) = ((zx - margin).max(0.0), (zy - margin).max(0.0));
        let yaw = self.rng.between(-yaw_range, yaw_range);
        let pitch = self.rng.between(-pitch_range, pitch_range);
        let at = front_wall(yaw, pitch);
        let length = dot3(at, at).sqrt();
        self.dots.push(Dot {
            direction: at.map(|axis| axis / length),
            spawned_at_ms: self.now_ms,
        });
    }

    /// A shot at the crosshair. Ignored when the cursor aims itself.
    pub fn click(&mut self) {
        if let Some((x, y)) = self.crosshair() {
            self.click_at(x, y);
        }
    }

    /// A shot at a point in the arena, in pixels from its top-left: hits the newest dot drawn
    /// under it, or misses.
    pub fn click_at(&mut self, aim_x: f64, aim_y: f64) {
        if self.is_over() {
            return;
        }
        let now = self.now_ms;
        let axes = self.camera.axes();
        let hit = self.dots.iter().rposition(|dot| {
            self.draw(dot, axes)
                .is_some_and(|dot| (dot.x - aim_x).hypot(dot.y - aim_y) <= dot.radius)
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

    fn draw(&self, dot: &Dot, axes: Axes) -> Option<DrawnDot> {
        let (x, y) = self.view.project(axes, dot.direction)?;
        let diameter = dot_diameter(self.now_ms - dot.spawned_at_ms);
        Some(DrawnDot {
            x,
            y,
            radius: self.view.radius_px(axes, dot.direction, diameter),
            burst_radius: self.view.radius_px(axes, dot.direction, DOT_MAX_DEGREES),
        })
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

    /// Each dot, oldest first.
    pub fn dots(&self) -> impl Iterator<Item = Dot> + '_ {
        self.dots.iter().copied()
    }

    /// The spawn zone's outline on the room's front wall, in 3D, so it's clear where to look.
    pub fn spawn_zone_lines(&self) -> Vec<((f64, f64), (f64, f64))> {
        if self.mode == ViewMode::TwoD {
            return Vec::new();
        }
        let axes = self.camera.axes();
        let (zx, zy) = self.spawn_zone();
        let corners = [(-zx, -zy), (zx, -zy), (zx, zy), (-zx, zy)].map(|(x, y)| front_wall(x, y));
        (0..4)
            .filter_map(|i| self.project_line(axes, corners[i], corners[(i + 1) % 4]))
            .collect()
    }

    /// A line in the room as drawn now, cut where it passes behind the view.
    fn project_line(
        &self,
        axes: Axes,
        from: Direction,
        to: Direction,
    ) -> Option<((f64, f64), (f64, f64))> {
        let (from, to) = in_front(axes[0], from, to)?;
        Some((self.view.project(axes, from)?, self.view.project(axes, to)?))
    }

    /// The grid of a cube-shaped room around the view, so turning has still walls to turn
    /// against instead of a void. Lines are cut where they pass behind the view.
    pub fn room_lines(&self) -> Vec<RoomLine> {
        let axes = self.camera.axes();
        let mut lines = Vec::new();
        for normal in 0..3 {
            for side in [-1.0, 1.0] {
                for along in (0..3).filter(|axis| *axis != normal) {
                    for step in 0..=ROOM_DIVISIONS {
                        let mut from = [0.0; 3];
                        from[normal] = side;
                        from[3 - normal - along] =
                            -1.0 + 2.0 * f64::from(step) / f64::from(ROOM_DIVISIONS);
                        let mut to = from;
                        from[along] = -1.0;
                        to[along] = 1.0;
                        if let Some((from, to)) = self.project_line(axes, from, to) {
                            lines.push(RoomLine {
                                from,
                                to,
                                floor: normal == 1 && side > 0.0,
                            });
                        }
                    }
                }
            }
        }
        lines
    }

    /// Each dot in front of the view as drawn now, oldest first.
    pub fn drawn_dots(&self) -> impl Iterator<Item = DrawnDot> + '_ {
        let axes = self.camera.axes();
        self.dots.iter().filter_map(move |dot| self.draw(dot, axes))
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
    /// The best run on Hard, which was the only pace before difficulties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best: Option<AimRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_easy: Option<AimRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_normal: Option<AimRun>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub difficulty: Difficulty,
    #[serde(default, skip_serializing_if = "is_default")]
    pub sensitivity_mode: SensitivityMode,
    #[serde(default, skip_serializing_if = "is_default")]
    pub view_mode: ViewMode,
    /// Runs fill the monitor.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fullscreen: bool,
}

/// How fast the mouse aims.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityMode {
    /// Raw mouse counts at VALORANT's sensitivity.
    #[default]
    Valorant,
    /// The Windows cursor's speed, pointer speed and acceleration included.
    Windows,
}

/// What the mouse moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    /// The view turns under a crosshair fixed at the centre, as in game.
    #[default]
    #[serde(rename = "3d")]
    ThreeD,
    /// The view stays still and the crosshair moves over it.
    #[serde(rename = "2d")]
    TwoD,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
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

    /// Each difficulty keeps its own best run.
    pub fn best(&self, difficulty: Difficulty) -> Option<AimRun> {
        match difficulty {
            Difficulty::Easy => self.best_easy,
            Difficulty::Normal => self.best_normal,
            Difficulty::Hard => self.best,
        }
    }

    pub fn best_mut(&mut self, difficulty: Difficulty) -> &mut Option<AimRun> {
        match difficulty {
            Difficulty::Easy => &mut self.best_easy,
            Difficulty::Normal => &mut self.best_normal,
            Difficulty::Hard => &mut self.best,
        }
    }
}

/// A finished run as saved: each difficulty keeps its best one for the whole app.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AimRun {
    pub score: u32,
    pub misses: u32,
    pub bursts: u32,
    pub avg_reaction_ms: Option<u32>,
    pub duration_ms: u64,
    /// With the DPI, `None` for a run aimed with the Windows cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitivity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<u32>,
    pub finished_at_unix: i64,
}

impl AimRun {
    pub fn new(
        results: RunResults,
        sensitivity: Option<f64>,
        dpi: Option<u32>,
        finished_at_unix: i64,
    ) -> Self {
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
        Game::new(Some(SENS), ViewMode::ThreeD, Difficulty::Hard, view(), 42)
    }

    fn step(game: &mut Game, ms: u64) {
        for _ in 0..ms / 100 {
            game.advance(100);
        }
    }

    /// Turns the crosshair onto the newest dot the way a mouse would, in whole counts.
    fn aim_at_newest(game: &mut Game) {
        let (yaw, pitch) = game.dots().last().expect("a dot").angles();
        let camera = game.camera();
        let per_count = game.degrees_per_count().expect("raw aim");
        game.move_aim(
            ((yaw - camera.yaw) / per_count).round() as i32,
            ((pitch - camera.pitch) / per_count).round() as i32,
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
        let ahead = Camera::default();
        let (x, y) = full
            .project(ahead.axes(), direction(51.5, 0.0))
            .expect("in front");
        assert!((x - 1920.0).abs() < 1e-6 && (y - 540.0).abs() < 1e-6);
        assert_eq!(
            full.project(ahead.axes(), direction(0.0, 0.0)),
            Some((960.0, 540.0))
        );

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
        let ahead = Camera::default();
        assert!(
            view.radius_px(ahead.axes(), direction(30.0, 0.0), 3.6)
                > view.radius_px(ahead.axes(), direction(0.0, 0.0), 3.6) * 1.1
        );
    }

    #[test]
    fn a_click_hits_inside_the_drawn_dot_and_misses_just_outside() {
        let mut game = Game::new(None, ViewMode::TwoD, Difficulty::Hard, view(), 42);
        game.dots.push(Dot {
            direction: direction(30.0, 0.0),
            spawned_at_ms: 0,
        });
        game.now_ms = 300;
        let dot = game.drawn_dots().next().expect("drawn");
        game.click_at(dot.x, dot.y + dot.radius * 1.05);
        assert_eq!((game.score(), game.results().misses), (0, 1));
        game.click_at(dot.x, dot.y + dot.radius * 0.95);
        assert_eq!(game.score(), 1);
    }

    #[test]
    fn cursor_aim_shoots_where_it_clicks_and_ignores_raw_counts() {
        let mut game = Game::new(None, ViewMode::TwoD, Difficulty::Hard, view(), 42);
        game.move_aim(1_000, 1_000);
        assert_eq!(game.camera(), Camera::default());
        game.advance(0);
        let dot = game.drawn_dots().last().expect("a dot");
        game.click_at(dot.x, dot.y);
        assert_eq!(game.score(), 1);
        game.click_at(0.0, 0.0);
        assert_eq!(game.results().misses, 1);
    }

    #[test]
    fn the_mouse_turns_the_view_right_and_up() {
        let mut game = game();
        game.move_aim(10, 0);
        assert!(game.camera().yaw > 0.0);
        game.move_aim(0, -10);
        assert!(game.camera().pitch < 0.0);
    }

    #[test]
    fn a_dot_stays_put_in_the_world_while_the_view_turns() {
        let mut game = game();
        game.advance(0);
        let angles = game.dots().next().expect("a dot").angles();
        let before = game.drawn_dots().next().expect("drawn");
        game.move_aim(20, 0);
        let after = game.drawn_dots().next().expect("drawn");
        assert_eq!(game.dots().next().expect("a dot").angles(), angles);
        assert!(after.x < before.x, "turning right slides the dot left");
    }

    #[test]
    fn turning_onto_a_dot_brings_it_under_the_centre_crosshair() {
        let mut game = game();
        game.advance(0);
        aim_at_newest(&mut game);
        let dot = game.drawn_dots().next().expect("drawn");
        let (cx, cy) = game.view().centre();
        assert!((dot.x - cx).hypot(dot.y - cy) < dot.radius);
    }

    #[test]
    fn spawn_interval_steps_down_with_score() {
        let steps = |difficulty: Difficulty| -> Vec<u64> {
            [0, 4, 5, 19, 20, 34, 35, 49, 50, 200]
                .into_iter()
                .map(|score| difficulty.spawn_interval_ms(score))
                .collect()
        };
        assert_eq!(
            steps(Difficulty::Hard),
            [600, 600, 500, 500, 400, 400, 300, 300, 200, 200]
        );
        // At their fastest, as many dots as each difficulty says live at once.
        for (difficulty, most) in [
            (Difficulty::Easy, 4),
            (Difficulty::Normal, 6),
            (Difficulty::Hard, 11),
        ] {
            let fastest = difficulty.spawn_interval_ms(u32::MAX);
            assert_eq!(DOT_LIFETIME_MS.div_ceil(fastest), most, "{difficulty:?}");
            assert!(steps(difficulty).is_sorted_by(|a, b| a >= b));
        }
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
        let view = game.view();
        assert_eq!(game.drawn_dots().count(), 2);
        assert!(game.drawn_dots().all(|dot| view.contains((dot.x, dot.y))));
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
    fn the_view_turns_all_the_way_round_but_stops_short_of_straight_up() {
        let mut game = game();
        game.move_aim(1_000_000, -1_000_000);
        assert!(game.camera().yaw.abs() < 360.0);
        assert_eq!(game.camera().pitch, -MAX_PITCH_DEGREES);
    }

    #[test]
    fn in_3d_dots_spawn_in_a_marked_zone_in_front_wherever_the_view_turns() {
        let mut game = game();
        assert_eq!(game.spawn_zone_lines().len(), 4);
        // Facing away, the zone's outline is behind the view and dots still spawn in front.
        game.move_aim(
            (180.0 / game.degrees_per_count().expect("raw aim")) as i32,
            0,
        );
        assert!(game.spawn_zone_lines().is_empty());
        for _ in 0..50 {
            step(&mut game, 1_000);
            game.click();
        }
        let (hx, hy) = game.view().half_angles();
        assert!(game.dots().count() > 0);
        for dot in game.dots() {
            let [x, y, z] = dot.direction;
            let (yaw, pitch) = ((x / z).atan().to_degrees(), (y / z).atan().to_degrees());
            assert!(z > 0.0 && yaw.abs() < hx / 2.0 && pitch.abs() < hy / 2.0);
        }
        // Resizing keeps them all.
        let count = game.dots().count();
        game.set_view(View::new(1920.0, 2.0, 2.0).expect("view"));
        assert_eq!(game.dots().count(), count);

        let flat = Game::new(Some(SENS), ViewMode::TwoD, Difficulty::Hard, view(), 42);
        assert!(flat.spawn_zone_lines().is_empty());
    }

    #[test]
    fn the_room_turns_with_the_view_and_is_cut_behind_it() {
        let mut game = game();
        let (cx, _) = game.view().centre();
        let through_centre = |game: &Game| {
            game.room_lines()
                .iter()
                .any(|line| (line.from.0 - cx).abs() < 1e-6 && (line.to.0 - cx).abs() < 1e-6)
        };
        // The front wall's middle line runs straight up the centre, and the floor is below it.
        assert!(through_centre(&game));
        assert!(game.room_lines().iter().any(|line| line.floor));
        game.move_aim(100, 0);
        assert!(!through_centre(&game));
        for _ in 0..30 {
            game.move_aim(997, 331);
            assert!(game.room_lines().iter().all(|line| {
                [line.from.0, line.from.1, line.to.0, line.to.1]
                    .iter()
                    .all(|v| v.is_finite())
            }));
        }
    }

    #[test]
    fn in_2d_the_crosshair_moves_over_a_still_view_and_stops_at_its_edge() {
        let mut game = Game::new(Some(SENS), ViewMode::TwoD, Difficulty::Hard, view(), 42);
        game.advance(0);
        let dot = game.drawn_dots().next().expect("a dot");
        let (cx, cy) = game.view().centre();
        let focal = game.view().focal_px;
        let per_count = game.degrees_per_count().expect("raw aim");
        let counts =
            |offset: f64| ((offset / focal).atan().to_degrees() / per_count).round() as i32;
        // Moving sideways keeps the crosshair level.
        game.move_aim(counts(dot.x - cx), 0);
        assert!((game.crosshair().expect("drawn").1 - cy).abs() < 1e-9);
        game.move_aim(0, counts(dot.y - cy));
        assert_eq!(game.camera(), Camera::default());
        game.click();
        assert_eq!(game.score(), 1);

        game.move_aim(1_000_000, -1_000_000);
        let (x, y) = game.crosshair().expect("drawn");
        assert!(
            (x - view().width_px).abs() < 1e-6 && y.abs() < 1e-6,
            "{x}, {y}"
        );
    }

    #[test]
    fn the_windows_cursor_turns_a_3d_view_at_its_desktop_speed() {
        let mut game = Game::new(None, ViewMode::ThreeD, Difficulty::Hard, view(), 42);
        game.set_pixel_scale(1.5);
        let (cx, cy) = game.view().centre();
        let target = game
            .view()
            .direction_at(game.camera().axes(), cx + 100.0, cy);
        // 100 logical pixels are 150 physical ones at 150% scaling.
        game.move_aim(150, 0);
        let (x, _) = game
            .view()
            .project(game.camera().axes(), target)
            .expect("in front");
        assert!((x - cx).abs() < 1.0, "{x} vs {cx}");
        assert_eq!(game.crosshair(), Some((cx, cy)));
    }

    #[test]
    fn accuracy_is_hits_over_clicks() {
        assert_eq!(accuracy(0, 0), None);
        assert_eq!(accuracy(1, 1), Some(0.5));
        assert_eq!(accuracy(3, 0), Some(1.0));
    }
}
