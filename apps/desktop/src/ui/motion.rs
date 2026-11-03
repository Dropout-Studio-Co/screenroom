use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

const TWO_PI: f32 = std::f32::consts::TAU;

const MAX_STEP: f32 = 0.064;

const MAX_LIFTS: usize = 64;

const MAX_RAILS: usize = 16;

pub const REST: f32 = 0.01;

pub const DECELERATION: f32 = 0.998;

pub const RUBBER_FACTOR: f32 = 0.55;

pub const ARRIVAL_SCRIM: f32 = 0.55;

pub const TICK: Duration = Duration::from_millis(8);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    value: f32,

    velocity: f32,
}

impl Spring {
    pub const fn at(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn velocity(&self) -> f32 {
        self.velocity
    }

    pub fn is_finite(&self) -> bool {
        self.value.is_finite() && self.velocity.is_finite()
    }

    pub fn carry(&mut self, velocity: f32) {
        if velocity.is_finite() {
            self.velocity = velocity;
        }
    }

    pub fn place(&mut self, value: f32) {
        if value.is_finite() {
            self.value = value;
            self.velocity = 0.0;
        }
    }

    pub fn step(&mut self, target: f32, dt: Duration, damping: f32, response: f32) -> f32 {
        if !self.is_finite() || !target.is_finite() {
            if target.is_finite() {
                self.value = target;
            }
            self.velocity = 0.0;
            return self.value;
        }

        let dt = dt.as_secs_f32().clamp(0.0, MAX_STEP);
        if dt <= 0.0 {
            return self.value;
        }

        let response = response.clamp(0.001, 10.0);
        let damping = damping.clamp(0.0, 4.0);
        let omega = TWO_PI / response;
        let (value, velocity) = solve(self.value - target, self.velocity, omega, damping, dt);

        self.value = value + target;
        self.velocity = velocity;

        if !self.is_finite() || self.settled_at(target, response) {
            self.value = target;
            self.velocity = 0.0;
        }

        self.value
    }

    pub fn settled_at(&self, target: f32, response: f32) -> bool {
        if !self.is_finite() || !target.is_finite() {
            return true;
        }
        let omega = TWO_PI / response.clamp(0.001, 10.0);
        let remaining = (self.value - target).abs() + self.velocity.abs() / omega;
        remaining < REST
    }
}

fn solve(offset: f32, velocity: f32, omega: f32, damping: f32, dt: f32) -> (f32, f32) {
    let decay = damping * omega;

    if (damping - 1.0).abs() < 1.0e-4 {
        let b = velocity + omega * offset;
        let envelope = (-omega * dt).exp();
        (
            (offset + b * dt) * envelope,
            (velocity - omega * b * dt) * envelope,
        )
    } else if damping < 1.0 {
        let damped = omega * (1.0 - damping * damping).sqrt();
        let envelope = (-decay * dt).exp();
        let (sin, cos) = (damped * dt).sin_cos();
        let b = (velocity + decay * offset) / damped;
        (
            envelope * (offset * cos + b * sin),
            envelope * (velocity * cos - (offset * damped + decay * b) * sin),
        )
    } else {
        let damped = omega * (damping * damping - 1.0).sqrt();
        let envelope = (-decay * dt).exp();
        let b = (velocity + decay * offset) / damped;
        (
            envelope * (offset * damped.cosh() + b * damped.sinh()),
            envelope * (velocity * damped.cosh() + (offset * damped - decay * b) * damped.sinh()),
        )
    }
}

pub fn project(velocity: f32, deceleration: f32) -> f32 {
    if !velocity.is_finite() {
        return 0.0;
    }
    let deceleration = if deceleration.is_finite() {
        deceleration.clamp(0.0, 0.999_999)
    } else {
        DECELERATION
    };
    velocity / 1_000.0 * deceleration / (1.0 - deceleration)
}

pub fn rubberband(overshoot: f32, dimension: f32) -> f32 {
    if !overshoot.is_finite() || !dimension.is_finite() || dimension <= 0.0 || overshoot == 0.0 {
        return 0.0;
    }
    let sign = if overshoot < 0.0 { -1.0 } else { 1.0 };
    let resisted = 1.0 - 1.0 / (overshoot.abs() * RUBBER_FACTOR / dimension + 1.0);
    sign * resisted * dimension
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    pub damping: f32,

    pub response: f32,
}

impl Spec {
    pub const fn new(damping: f32, response: f32) -> Self {
        Self { damping, response }
    }

    pub fn settled(&self, spring: &Spring, target: f32) -> bool {
        spring.settled_at(target, self.response)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub arrive: Spec,

    pub focus: Spec,

    pub rail: Spec,

    pub reveal: Spec,

    pub chrome: Spec,

    pub flick: Spec,

    pub fade: Spec,

    pub lift: f32,

    pub travel: f32,

    pub reveal_scale: f32,

    pub rubber_room: f32,

    pub page_rubber_room: f32,
}

pub const MOTION: Motion = Motion {
    arrive: Spec::new(1.0, 0.38),
    focus: Spec::new(1.0, 0.30),
    rail: Spec::new(0.92, 0.40),
    reveal: Spec::new(0.85, 0.34),
    chrome: Spec::new(1.0, 0.32),
    flick: Spec::new(0.8, 0.42),
    fade: Spec::new(1.0, 0.17),
    lift: 0.055,
    travel: 22.0,
    reveal_scale: 0.035,
    rubber_room: 96.0,
    page_rubber_room: 120.0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Full,

    Reduced,
}

impl Policy {
    pub fn from_flag(reduced: bool) -> Self {
        if reduced { Self::Reduced } else { Self::Full }
    }

    pub fn is_reduced(self) -> bool {
        self == Self::Reduced
    }

    pub fn spec(self, spec: Spec) -> Spec {
        match self {
            Self::Full => spec,
            Self::Reduced => MOTION.fade,
        }
    }

    pub fn travel(self, pixels: f32) -> f32 {
        match self {
            Self::Full => pixels,
            Self::Reduced => 0.0,
        }
    }

    pub fn lift(self, amount: f32) -> f32 {
        match self {
            Self::Full => amount,
            Self::Reduced => 0.0,
        }
    }
}

pub fn lift_scale(lift: f32) -> f32 {
    if !lift.is_finite() {
        return 1.0;
    }
    1.0 + lift.clamp(0.0, 1.0) * MOTION.lift
}

pub fn reveal_scale(reveal: f32) -> f32 {
    if !reveal.is_finite() {
        return 1.0;
    }
    1.0 + (1.0 - reveal.clamp(0.0, 1.0)) * MOTION.reveal_scale
}

pub fn with_alpha(color: iced::Color, alpha: f32) -> iced::Color {
    let alpha = if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        1.0
    };
    iced::Color {
        a: color.a * alpha,
        ..color
    }
}

pub fn mix(from: iced::Color, to: iced::Color, amount: f32) -> iced::Color {
    let amount = if amount.is_finite() {
        amount.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let blend = |a: f32, b: f32| a + (b - a) * amount;
    iced::Color {
        r: blend(from.r, to.r),
        g: blend(from.g, to.g),
        b: blend(from.b, to.b),
        a: blend(from.a, to.a),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RailExtent {
    pub limit: f32,

    pub viewport: f32,

    pub room: f32,
}

impl RailExtent {
    pub fn new(limit: f32, viewport: f32, room: f32) -> Self {
        Self {
            limit: if limit.is_finite() {
                limit.max(0.0)
            } else {
                0.0
            },
            viewport: if viewport.is_finite() {
                viewport.max(0.0)
            } else {
                0.0
            },
            room: if room.is_finite() { room.max(0.0) } else { 0.0 },
        }
    }

    pub fn reach(&self) -> f32 {
        self.limit + self.room
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Channel {
    spring: Spring,

    target: f32,

    spec: Spec,
}

impl Channel {
    pub const fn at(value: f32) -> Self {
        Self {
            spring: Spring::at(value),
            target: value,
            spec: MOTION.arrive,
        }
    }

    pub fn value(&self) -> f32 {
        self.spring.value()
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn retarget(&mut self, target: f32, spec: Spec) {
        if target.is_finite() {
            self.target = target;
            self.spec = spec;
        }
    }

    pub fn release(&mut self, velocity: f32) {
        self.spring.carry(velocity);
    }

    pub fn place(&mut self, value: f32) {
        self.spring.place(value);
        self.target = value;
    }

    pub fn advance(&mut self, dt: Duration, policy: Policy) -> f32 {
        let spec = policy.spec(self.spec);
        self.spring
            .step(self.target, dt, spec.damping, spec.response)
    }

    pub fn settled(&self) -> bool {
        self.spec.settled(&self.spring, self.target)
    }

    pub fn moving(&self) -> bool {
        !self.settled()
    }
}

#[derive(Debug)]
pub struct MotionState {
    policy: Policy,

    reduced: bool,

    lifts: BTreeMap<usize, Spring>,

    lift_target: usize,

    rails: HashMap<String, Channel>,

    page: Channel,

    chrome: Channel,

    reveal: Channel,

    arrival: Channel,

    arrival_travel: f32,

    last: Option<Instant>,
}

impl Default for MotionState {
    fn default() -> Self {
        Self::new(false)
    }
}

impl MotionState {
    pub fn new(reduced: bool) -> Self {
        Self {
            policy: Policy::from_flag(reduced),
            reduced,
            lifts: BTreeMap::new(),
            lift_target: usize::MAX,
            rails: HashMap::new(),
            page: Channel::at(0.0),
            chrome: Channel::at(0.0),
            reveal: Channel::at(0.0),
            arrival: Channel::at(1.0),
            arrival_travel: 0.0,
            last: None,
        }
    }

    pub fn reduced(&self) -> bool {
        self.reduced
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub fn set_reduced(&mut self, reduced: bool) {
        if reduced != self.reduced {
            self.reduced = reduced;
            self.policy = Policy::from_flag(reduced);
        }
    }

    pub fn focus(&mut self, index: usize) {
        if index == self.lift_target {
            return;
        }
        self.lift_target = index;
        if self.lifts.len() < MAX_LIFTS {
            self.lifts.entry(index).or_insert_with(|| Spring::at(0.0));
        }
    }

    pub fn lift(&self, index: usize) -> f32 {
        self.lifts
            .get(&index)
            .map(Spring::value)
            .unwrap_or(0.0)
            .clamp(0.0, 1.0)
    }

    pub fn focus_index(&self) -> Option<usize> {
        (self.lift_target != usize::MAX).then_some(self.lift_target)
    }

    pub fn release_focus(&mut self) {
        self.lift_target = usize::MAX;
    }

    pub fn clear_lifts(&mut self) {
        self.lifts.clear();
        self.lift_target = usize::MAX;
    }

    pub fn moving_rails(&self) -> Vec<(String, f32)> {
        self.rails
            .iter()
            .filter(|(_, channel)| channel.moving())
            .map(|(label, channel)| (label.clone(), channel.value()))
            .collect()
    }

    pub fn rail_offset(&self, label: &str) -> Option<f32> {
        self.rails.get(label).map(Channel::value)
    }

    pub fn scroll_rail(&mut self, label: &str, target: f32, extent: RailExtent) {
        if !target.is_finite() || extent.viewport <= 0.0 {
            return;
        }
        if self.rails.len() >= MAX_RAILS && !self.rails.contains_key(label) {
            return;
        }
        let entry = self
            .rails
            .entry(label.to_string())
            .or_insert_with(|| Channel::at(0.0));
        entry.retarget(resist(target, extent), MOTION.rail);
    }

    pub fn release_rail(&mut self, label: &str, velocity: f32, limit: f32) {
        let Some(channel) = self.rails.get_mut(label) else {
            return;
        };
        let projected = channel.value() + project(velocity, DECELERATION);
        channel.release(velocity);
        channel.retarget(projected.clamp(0.0, limit.max(0.0)), MOTION.flick);
    }

    pub fn adopt_rail(&mut self, label: &str, offset: f32) {
        if !offset.is_finite() {
            return;
        }
        if let Some(channel) = self.rails.get_mut(label) {
            if (channel.value() - offset).abs() > 2.0 {
                channel.place(offset);
            }
            return;
        }
        if self.rails.len() < MAX_RAILS {
            self.rails.insert(label.to_string(), Channel::at(offset));
        }
    }

    pub fn flush_rails(&mut self) {
        self.rails.clear();
    }

    pub fn scroll_page(&mut self, target: f32) {
        if target.is_finite() {
            self.page.retarget(target.max(0.0), MOTION.rail);
        }
    }

    pub fn page_offset(&self) -> f32 {
        self.page.value()
    }

    pub fn page_settled(&self) -> bool {
        self.page.settled()
    }

    pub fn place_page(&mut self, offset: f32) {
        self.page.place(offset);
    }

    pub fn place_chrome(&mut self, visible: bool) {
        self.chrome.place(if visible { 1.0 } else { 0.0 });
    }

    pub fn set_chrome(&mut self, visible: bool) {
        let target = if visible { 1.0 } else { 0.0 };
        if (self.chrome.target() - target).abs() > f32::EPSILON {
            self.chrome.retarget(target, MOTION.chrome);
        }
    }

    pub fn chrome(&self) -> f32 {
        self.chrome.value()
    }

    pub fn set_reveal(&mut self, open: bool) {
        let target = if open { 1.0 } else { 0.0 };
        if (self.reveal.target() - target).abs() > f32::EPSILON {
            self.reveal.retarget(target, MOTION.reveal);
        }
    }

    pub fn reveal(&self) -> f32 {
        self.reveal.value()
    }

    pub fn arrive(&mut self, direction: f32) {
        self.arrival_travel = self.policy.travel(MOTION.travel) * direction;
        if self.arrival.settled() {
            self.arrival.place(0.0);
        }
        self.arrival.retarget(1.0, MOTION.arrive);
    }

    pub fn arrival_offset(&self) -> f32 {
        if self.policy.is_reduced() {
            return 0.0;
        }
        let remaining = 1.0 - self.arrival.value().clamp(0.0, 1.0);
        self.arrival_travel * remaining
    }

    pub fn arrival_scrim(&self) -> f32 {
        if self.policy.is_reduced() {
            return 0.0;
        }
        (1.0 - self.arrival.value().clamp(0.0, 1.0)) * ARRIVAL_SCRIM
    }

    pub fn busy(&self) -> bool {
        self.lifts.iter().any(|(index, spring)| {
            let goal = if *index == self.lift_target { 1.0 } else { 0.0 };
            !MOTION.focus.settled(spring, goal)
        }) || self.rails.values().any(Channel::moving)
            || self.page.moving()
            || self.chrome.moving()
            || self.reveal.moving()
            || self.arrival.moving()
    }

    pub fn idle(&self) -> bool {
        !self.busy()
    }

    pub fn step(&mut self, now: Instant) -> Duration {
        let dt = self
            .last
            .map(|last| now.saturating_duration_since(last))
            .unwrap_or(TICK)
            .min(Duration::from_millis(64));
        self.last = Some(now);
        self.advance(dt);
        if self.idle() {
            self.last = None;
        }
        dt
    }

    pub fn advance(&mut self, dt: Duration) {
        let policy = self.policy;
        let target = self.lift_target;
        let focus = policy.spec(MOTION.focus);

        self.lifts.retain(|index, spring| {
            let goal = if *index == target { 1.0 } else { 0.0 };
            spring.step(goal, dt, focus.damping, focus.response);
            *index == target || !focus.settled(spring, goal)
        });

        for channel in self.rails.values_mut() {
            channel.advance(dt, policy);
        }

        self.page.advance(dt, policy);
        self.chrome.advance(dt, policy);
        self.reveal.advance(dt, policy);
        self.arrival.advance(dt, policy);
    }
}

fn resist(target: f32, extent: RailExtent) -> f32 {
    if !target.is_finite() || extent.viewport <= 0.0 {
        return target;
    }
    if target < 0.0 {
        return -rubberband(-target, extent.viewport).min(extent.room);
    }
    if target > extent.limit {
        let give = rubberband(target - extent.limit, extent.viewport);
        return extent.limit + give.min(extent.room);
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: Duration = Duration::from_millis(8);

    fn settle(spring: &mut Spring, target: f32, damping: f32, response: f32) -> Vec<f32> {
        let mut track = Vec::new();
        for _ in 0..600 {
            track.push(spring.step(target, STEP, damping, response));
        }
        track
    }

    #[test]
    fn a_critically_damped_spring_settles_without_overshoot() {
        let mut spring = Spring::at(0.0);
        let track = settle(&mut spring, 1.0, 1.0, 0.34);

        assert!(
            track.windows(2).all(|pair| pair[1] >= pair[0] - 1.0e-6),
            "a critically damped spring may never travel backwards"
        );
        assert!(
            track.iter().all(|value| *value <= 1.0 + 1.0e-5),
            "a critically damped spring may never pass its target"
        );
        assert!(
            (track.last().copied().unwrap() - 1.0).abs() < 1.0e-4,
            "it must arrive"
        );
        assert_eq!(spring.velocity(), 0.0, "and come to rest exactly");
    }

    #[test]
    fn an_under_damped_spring_overshoots_and_still_settles() {
        let mut spring = Spring::at(0.0);
        let track = settle(&mut spring, 1.0, 0.5, 0.34);
        let peak = track.iter().copied().fold(f32::MIN, f32::max);

        assert!(peak > 1.02, "a bouncy spring must visibly pass the target");
        assert!(
            (track.last().copied().unwrap() - 1.0).abs() < 1.0e-4,
            "it settles in the end"
        );
    }

    #[test]
    fn retargeting_mid_flight_keeps_the_value_and_the_velocity() {
        let mut spring = Spring::at(0.0);
        for _ in 0..10 {
            spring.step(1.0, STEP, 1.0, 0.34);
        }
        let before = spring.value();
        let speed = spring.velocity();
        assert!(
            before > 0.0 && before < 1.0,
            "the test needs a mid-flight value"
        );
        assert!(speed > 0.0);

        let after = spring.step(0.0, Duration::ZERO, 1.0, 0.34);
        assert!(
            (after - before).abs() < 1.0e-6,
            "a retarget may not jump the value"
        );
        assert!(
            (spring.velocity() - speed).abs() < 1.0e-6,
            "and may not drop the velocity it already carries"
        );

        let mut values = Vec::new();
        for _ in 0..200 {
            values.push(spring.step(0.0, STEP, 1.0, 0.34));
        }

        assert!(
            values.iter().any(|value| *value < before),
            "the new target wins once the carried velocity is spent"
        );
        let peak = values
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(index, _)| index)
            .expect("the track is not empty");
        assert!(
            peak > 0,
            "the carried velocity is spent first, and that is the continuity"
        );
        assert!(
            values[peak..]
                .windows(2)
                .all(|pair| pair[1] <= pair[0] + 1.0e-5),
            "after the peak it falls without wobbling back up"
        );
        assert!(
            (values.last().copied().unwrap()).abs() < 1.0e-4,
            "it comes to rest on the new target"
        );
    }

    #[test]
    fn a_projection_is_monotonic_in_the_release_velocity() {
        let still = project(0.0, DECELERATION);
        let slow = project(400.0, DECELERATION);
        let fast = project(2_000.0, DECELERATION);

        assert_eq!(still, 0.0);
        assert!(slow > 0.0 && fast > slow);
        assert!((fast - 998.0).abs() < 0.5, "2000 px/s projects to 998 px");
        assert!(project(-900.0, DECELERATION) < 0.0);
        assert_eq!(project(f32::NAN, DECELERATION), 0.0);
    }

    #[test]
    fn a_rubber_band_resists_progressively_and_is_the_identity_at_zero() {
        assert_eq!(rubberband(0.0, 800.0), 0.0);

        let first = rubberband(40.0, 800.0);
        let second = rubberband(200.0, 800.0);
        let third = rubberband(10_000.0, 800.0);

        assert!(
            first > 0.0 && first < 40.0,
            "it must give, but less than asked"
        );
        assert!(second > first, "more overshoot still moves further");
        assert!(
            second - first < 160.0,
            "each extra pixel of overshoot must buy less movement"
        );
        assert!(third < 800.0, "it can never exceed the dimension");
        assert!(rubberband(-120.0, 800.0) < 0.0);
        assert_eq!(rubberband(10.0, 0.0), 0.0);
        assert_eq!(rubberband(f32::INFINITY, 800.0), 0.0);
    }

    #[test]
    fn the_per_pixel_gain_of_a_rubber_band_falls_as_it_stretches() {
        let gain = |offset: f32| rubberband(offset + 1.0, 900.0) - rubberband(offset, 900.0);
        assert!(gain(0.0) > gain(100.0));
        assert!(gain(100.0) > gain(500.0));
        assert!(gain(500.0) > 0.0);
    }

    #[test]
    fn a_stalled_frame_is_clamped_instead_of_teleporting_the_spring() {
        let mut spring = Spring::at(0.3);
        spring.carry(12.0);
        assert_eq!(spring.step(1.0, Duration::ZERO, 1.0, 0.3), 0.3);

        let after = spring.step(1.0, Duration::from_secs(30), 1.0, 0.3);
        assert!(spring.is_finite());
        assert!(
            after > 0.3 && after < 1.0,
            "a long frame advances at most the clamp, never the whole distance"
        );

        for _ in 0..600 {
            spring.step(1.0, STEP, 1.0, 0.3);
        }
        assert_eq!(spring.value(), 1.0);
        assert_eq!(spring.velocity(), 0.0);
    }

    #[test]
    fn a_nonsense_target_snaps_instead_of_leaking_nan() {
        let mut spring = Spring::at(0.5);
        spring.carry(f32::NAN);
        assert_eq!(spring.step(f32::NAN, STEP, 1.0, 0.3), 0.5);
        assert!(spring.is_finite());

        let mut dirty = Spring::at(f32::NAN);
        dirty.step(1.0, STEP, 1.0, 0.3);
        assert!(dirty.is_finite());
    }

    #[test]
    fn the_response_time_sets_how_long_the_travel_takes() {
        let mut quick = Spring::at(0.0);
        let mut slow = Spring::at(0.0);
        for _ in 0..30 {
            quick.step(1.0, STEP, 1.0, 0.20);
            slow.step(1.0, STEP, 1.0, 0.60);
        }
        assert!(
            quick.value() > slow.value(),
            "a shorter response arrives sooner at the same moment"
        );
    }

    #[test]
    fn a_reduced_policy_collapses_every_spring_to_the_short_fade() {
        assert_eq!(Policy::Full.spec(MOTION.focus), MOTION.focus);
        assert_eq!(Policy::Reduced.spec(MOTION.focus), MOTION.fade);
        assert_eq!(Policy::Full.travel(22.0), 22.0);
        assert_eq!(Policy::Reduced.travel(22.0), 0.0);
        assert_eq!(Policy::Full.lift(0.055), 0.055);
        assert_eq!(Policy::Reduced.lift(0.055), 0.0);
        const {
            assert!(MOTION.fade.response >= 0.15 && MOTION.fade.response <= 0.20);
            assert!(MOTION.fade.damping >= 1.0);
        }
    }

    #[test]
    fn the_house_curves_stay_subtle_and_measured() {
        const {
            assert!(MOTION.lift >= 0.03 && MOTION.lift <= 0.06);
            assert!(MOTION.travel >= 12.0 && MOTION.travel <= 32.0);
            assert!(MOTION.arrive.damping == 1.0);
            assert!(MOTION.chrome.damping == 1.0);
            assert!(MOTION.focus.damping == 1.0);
            assert!(MOTION.focus.response >= 0.28 && MOTION.focus.response <= 0.40);
            assert!(MOTION.flick.damping < 1.0);
            assert!(MOTION.reveal.damping < 1.0);
        }
    }

    #[test]
    fn the_scale_helpers_are_the_identity_when_nothing_is_lifted() {
        assert!((lift_scale(0.0) - 1.0).abs() < 1.0e-6);
        assert!(lift_scale(1.0) > 1.0);
        assert!((lift_scale(f32::NAN) - 1.0).abs() < 1.0e-6);
        assert!((reveal_scale(1.0) - 1.0).abs() < 1.0e-6);
        assert!(reveal_scale(0.0) > 1.0);
    }

    #[test]
    fn an_alpha_helper_never_leaves_the_unit_range() {
        let ink = iced::Color::WHITE;
        assert_eq!(with_alpha(ink, 0.0).a, 0.0);
        assert_eq!(with_alpha(ink, 1.0).a, 1.0);
        assert_eq!(with_alpha(ink, -3.0).a, 0.0);
        assert_eq!(with_alpha(ink, 7.0).a, 1.0);
    }

    #[test]
    fn a_mix_walks_from_one_colour_to_the_other_and_clamps_both_ends() {
        let from = iced::Color::BLACK;
        let to = iced::Color::WHITE;
        assert_eq!(mix(from, to, 0.0), from);
        assert_eq!(mix(from, to, 1.0), to);
        let half = mix(from, to, 0.5);
        assert!((half.r - 0.5).abs() < 1.0e-6);
        assert_eq!(mix(from, to, -1.0), from);
        assert_eq!(mix(from, to, 9.0), to);
    }

    #[test]
    fn a_motion_state_is_quiet_at_rest_and_busy_only_while_something_moves() {
        let mut state = MotionState::new(false);
        assert!(state.idle(), "a fresh state must not ask for a tick");

        state.focus(3);
        assert!(!state.idle(), "a focus change must start the tick");
        for _ in 0..400 {
            state.advance(STEP);
        }
        assert!(state.idle(), "and it must stop again once settled");
        assert!(state.lift(3) > 0.99);
        assert_eq!(state.lift(2), 0.0);
    }

    #[test]
    fn a_focus_move_carries_the_old_card_down_instead_of_dropping_it() {
        let mut state = MotionState::new(false);
        state.focus(1);
        for _ in 0..40 {
            state.advance(STEP);
        }
        let lifted = state.lift(1);
        assert!(lifted > 0.9);

        state.focus(2);
        assert!(
            (state.lift(1) - lifted).abs() < 1.0e-6,
            "the card that lost focus must still be lifted on the frame it lost it"
        );
        for _ in 0..400 {
            state.advance(STEP);
        }
        assert!(state.lift(1) < 0.01);
        assert!(state.lift(2) > 0.99);
    }

    #[test]
    fn a_repeated_focus_on_the_same_card_does_not_restart_anything() {
        let mut state = MotionState::new(false);
        state.focus(4);
        for _ in 0..40 {
            state.advance(STEP);
        }
        let value = state.lift(4);
        state.focus(4);
        assert_eq!(state.lift(4), value);
    }

    #[test]
    fn a_rail_overshoots_with_resistance_and_settles_at_the_resisted_position() {
        let mut state = MotionState::new(false);
        let extent = RailExtent::new(1_000.0, 800.0, 400.0);
        state.scroll_rail("Newly added", 1_400.0, extent);
        for _ in 0..600 {
            state.advance(STEP);
        }
        let settled = state.rail_offset("Newly added").unwrap();
        let expected = 1_000.0 + rubberband(400.0, 800.0);
        assert!(
            (settled - expected).abs() < 0.5,
            "the rail gives at its end instead of stopping dead"
        );
        assert!(settled > 1_000.0 && settled < 1_400.0);
    }

    #[test]
    fn a_rail_can_never_travel_further_than_the_room_it_was_given() {
        let mut state = MotionState::new(false);
        let extent = RailExtent::new(1_000.0, 900.0, 96.0);
        state.scroll_rail("Films", 9_000.0, extent);
        for _ in 0..600 {
            state.advance(STEP);
        }
        let settled = state.rail_offset("Films").unwrap();
        assert!(
            settled <= extent.reach() + 0.01,
            "a hard flick may not scroll past the room the content actually has"
        );
        assert!(settled > 1_000.0);
    }

    #[test]
    fn a_rail_inside_its_range_is_placed_exactly() {
        let mut state = MotionState::new(false);
        state.scroll_rail(
            "Continue watching",
            320.0,
            RailExtent::new(1_000.0, 800.0, 96.0),
        );
        for _ in 0..400 {
            state.advance(STEP);
        }
        assert!((state.rail_offset("Continue watching").unwrap() - 320.0).abs() < 0.5);
    }

    #[test]
    fn a_rail_can_be_retargeted_before_it_arrives() {
        let mut state = MotionState::new(false);
        state.scroll_rail("Films", 900.0, RailExtent::new(2_000.0, 800.0, 96.0));
        for _ in 0..6 {
            state.advance(STEP);
        }
        let midway = state.rail_offset("Films").unwrap();
        assert!(midway > 0.0 && midway < 900.0);
        state.scroll_rail("Films", 100.0, RailExtent::new(2_000.0, 800.0, 96.0));
        assert!(
            (state.rail_offset("Films").unwrap() - midway).abs() < 1.0e-6,
            "a retarget may not teleport the rail"
        );
    }

    #[test]
    fn a_released_rail_lands_where_its_velocity_projects() {
        let mut state = MotionState::new(false);
        state.scroll_rail("Films", 400.0, RailExtent::new(2_000.0, 800.0, 96.0));
        for _ in 0..400 {
            state.advance(STEP);
        }
        state.release_rail("Films", 300.0, 2_000.0);
        for _ in 0..600 {
            state.advance(STEP);
        }
        let landed = state.rail_offset("Films").unwrap();
        assert!(
            (landed - (400.0 + project(300.0, DECELERATION))).abs() < 1.0,
            "a flick projects its landing point from the release velocity"
        );
    }

    #[test]
    fn a_release_past_the_end_lands_on_the_end_instead_of_running_off() {
        let mut state = MotionState::new(false);
        state.scroll_rail("Films", 900.0, RailExtent::new(1_000.0, 800.0, 96.0));
        state.release_rail("Films", 8_000.0, 1_000.0);
        for _ in 0..600 {
            state.advance(STEP);
        }
        assert_eq!(state.rail_offset("Films").unwrap(), 1_000.0);
    }

    #[test]
    fn an_adopted_manual_scroll_does_not_move_a_rail_that_already_matches() {
        let mut state = MotionState::new(false);
        state.scroll_rail("Films", 500.0, RailExtent::new(2_000.0, 800.0, 96.0));
        for _ in 0..400 {
            state.advance(STEP);
        }
        state.adopt_rail("Films", 500.4);
        assert!((state.rail_offset("Films").unwrap() - 500.0).abs() < 1.0);

        state.adopt_rail("Films", 120.0);
        assert_eq!(state.rail_offset("Films").unwrap(), 120.0);
        assert!(state.idle());
    }

    #[test]
    fn a_rail_the_user_scrolled_by_hand_starts_from_where_it_actually_is() {
        let mut state = MotionState::new(false);
        state.adopt_rail("Series", 640.0);
        assert_eq!(state.rail_offset("Series").unwrap(), 640.0);

        state.scroll_rail("Series", 900.0, RailExtent::new(2_000.0, 800.0, 96.0));
        assert_eq!(
            state.rail_offset("Series").unwrap(),
            640.0,
            "a spring must start from the value currently on screen"
        );
    }

    #[test]
    fn a_reduced_state_produces_no_travel_and_no_scale() {
        let mut state = MotionState::new(true);
        state.focus(0);
        state.arrive(1.0);
        assert_eq!(state.arrival_offset(), 0.0, "reduced motion may not slide");
        assert_eq!(state.arrival_scrim(), 0.0);
        assert!(state.busy());
        for _ in 0..200 {
            state.advance(STEP);
        }
        assert!(state.idle());
    }

    #[test]
    fn an_arrival_that_is_already_running_is_carried_instead_of_restarted() {
        let mut state = MotionState::new(false);
        state.arrive(1.0);
        for _ in 0..10 {
            state.advance(STEP);
        }
        let offset = state.arrival_offset();
        assert!(offset > 0.0 && offset < MOTION.travel);

        state.arrive(-1.0);
        let after = state.arrival_offset();
        assert!(
            after.abs() <= MOTION.travel,
            "a second arrival must not restart the slide from zero"
        );
        for _ in 0..200 {
            state.advance(STEP);
        }
        assert_eq!(state.arrival_offset(), 0.0);
        assert_eq!(state.arrival_scrim(), 0.0);
    }

    #[test]
    fn the_player_chrome_reveals_and_hides_without_being_asked_twice() {
        let mut state = MotionState::new(false);
        state.set_chrome(true);
        assert!(!state.idle());
        for _ in 0..400 {
            state.advance(STEP);
        }
        assert!((state.chrome() - 1.0).abs() < 1.0e-3);

        state.set_chrome(true);
        assert!(state.idle(), "the same request twice costs nothing");

        state.set_chrome(false);
        assert!(!state.idle());
    }
}
