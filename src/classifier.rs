use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Debug)]
struct AxisState {
    keys: [char; 2],
    held_keys: [bool; 2],
    press_times: [Option<f64>; 2],
    cs_candidate: Option<CounterStrafeCandidate>,
    cs_release_key: Option<char>,
    cs_release_time: Option<f64>,
    cs_press_key: Option<char>,
    cs_press_time: Option<f64>,
    overlap_start_time: Option<f64>,
    micro_candidate_duration: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
struct CounterStrafeCandidate {
    release_key: char,
    release_time_ms: f64,
    press_key: char,
    press_time_ms: f64,
}

impl AxisState {
    fn new(keys: [char; 2]) -> Self {
        Self {
            keys,
            held_keys: [false; 2],
            press_times: [None; 2],
            cs_candidate: None,
            cs_release_key: None,
            cs_release_time: None,
            cs_press_key: None,
            cs_press_time: None,
            overlap_start_time: None,
            micro_candidate_duration: None,
        }
    }

    fn on_press(&mut self, key: char, timestamp_ms: f64) {
        let Some(key_index) = self.key_index(key) else {
            return;
        };

        if self.held_keys[key_index] {
            return;
        }

        let other_index = 1 - key_index;
        let other = self.keys[other_index];
        self.held_keys[key_index] = true;
        self.press_times[key_index] = Some(timestamp_ms);

        if self.held_keys[other_index] {
            self.overlap_start_time = Some(timestamp_ms);
        }

        if self.cs_release_key == Some(other) && self.cs_press_time.is_none() {
            self.cs_press_key = Some(key);
            self.cs_press_time = Some(timestamp_ms);
            if let Some(release_time_ms) = self.cs_release_time
                && timestamp_ms > release_time_ms
            {
                self.cs_candidate = Some(CounterStrafeCandidate {
                    release_key: other,
                    release_time_ms,
                    press_key: key,
                    press_time_ms: timestamp_ms,
                });
            }
            self.micro_candidate_duration = None;
        }

        self.micro_candidate_duration = None;
    }

    fn on_release(&mut self, key: char, timestamp_ms: f64) {
        let Some(key_index) = self.key_index(key) else {
            return;
        };

        if !self.held_keys[key_index] {
            return;
        }

        if let Some(press_time) = self.press_times[key_index] {
            let duration = timestamp_ms - press_time;
            if duration < 80.0 {
                self.micro_candidate_duration = Some(duration);
            }
        }

        self.held_keys[key_index] = false;
        self.press_times[key_index] = None;
        self.overlap_start_time = None;
        self.cs_release_key = Some(key);
        self.cs_release_time = Some(timestamp_ms);
        self.cs_press_key = None;
        self.cs_press_time = None;
    }

    fn classify_shot(&mut self, shot_time_ms: f64) -> AxisClassification {
        if let Some(overlap_start_time) = self.overlap_start_time {
            let has_clean_counter_after_overlap =
                self.cs_candidate.is_some_and(|candidate| {
                    candidate.release_time_ms > overlap_start_time
                        && candidate.press_time_ms > candidate.release_time_ms
                        && self.keys.contains(&candidate.release_key)
                        && self.keys.contains(&candidate.press_key)
                        && candidate.press_key != candidate.release_key
                }) || self.cs_press_time.is_some_and(|cs_press_time| {
                    self.cs_release_time.is_some_and(|cs_release_time| {
                        cs_release_time > overlap_start_time && cs_press_time > cs_release_time
                    })
                });

            if !has_clean_counter_after_overlap {
                let overlap_time = shot_time_ms - overlap_start_time;
                self.reset();
                return AxisClassification::Overlap {
                    overlap_time_ms: overlap_time,
                };
            }
        }

        if let Some(candidate) = self.cs_candidate
            && candidate.press_time_ms > candidate.release_time_ms
            && self.keys.contains(&candidate.release_key)
            && self.keys.contains(&candidate.press_key)
            && candidate.press_key != candidate.release_key
        {
            let cs_time = candidate.press_time_ms - candidate.release_time_ms;
            let shot_delay = shot_time_ms - candidate.press_time_ms;
            self.reset();
            return AxisClassification::CounterStrafe {
                cs_time_ms: cs_time,
                shot_delay_ms: shot_delay,
            };
        }

        self.reset();
        AxisClassification::Bad
    }

    fn reset(&mut self) {
        let overlap_start_time = if self.held_keys.iter().all(|held| *held) {
            self.overlap_start_time
        } else {
            None
        };

        self.cs_candidate = None;
        self.cs_release_key = None;
        self.cs_release_time = None;
        self.cs_press_key = None;
        self.cs_press_time = None;
        self.overlap_start_time = overlap_start_time;
        self.micro_candidate_duration = None;
    }

    fn key_index(&self, key: char) -> Option<usize> {
        self.keys.iter().position(|candidate| *candidate == key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum AxisClassification {
    CounterStrafe { cs_time_ms: f64, shot_delay_ms: f64 },
    Overlap { overlap_time_ms: f64 },
    Bad,
}

impl AxisClassification {
    fn selection_rank(self) -> u8 {
        match self {
            AxisClassification::Overlap { .. } => 3,
            AxisClassification::CounterStrafe {
                cs_time_ms,
                shot_delay_ms,
            } if counter_strafe_passes_thresholds(cs_time_ms, shot_delay_ms) => 2,
            AxisClassification::CounterStrafe { .. } => 1,
            AxisClassification::Bad => 0,
        }
    }

    fn primary_time(self) -> Option<f64> {
        match self {
            AxisClassification::CounterStrafe { cs_time_ms, .. } => Some(cs_time_ms),
            AxisClassification::Overlap { overlap_time_ms } => Some(overlap_time_ms),
            AxisClassification::Bad => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShotLabel {
    CounterStrafe,
    Overlap,
    Bad,
}

impl ShotLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            ShotLabel::CounterStrafe => "Counter-strafe",
            ShotLabel::Overlap => "Overlap",
            ShotLabel::Bad => "Bad",
        }
    }
}

impl fmt::Display for ShotLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShotClassification {
    pub label: ShotLabel,
    pub cs_time_ms: Option<f64>,
    pub shot_delay_ms: Option<f64>,
    pub overlap_time_ms: Option<f64>,
}

impl ShotClassification {
    pub fn counter_strafe(cs_time_ms: f64, shot_delay_ms: f64) -> Self {
        Self {
            label: ShotLabel::CounterStrafe,
            cs_time_ms: Some(cs_time_ms),
            shot_delay_ms: Some(shot_delay_ms),
            overlap_time_ms: None,
        }
    }

    pub fn overlap(overlap_time_ms: f64) -> Self {
        Self {
            label: ShotLabel::Overlap,
            cs_time_ms: None,
            shot_delay_ms: None,
            overlap_time_ms: Some(overlap_time_ms),
        }
    }

    pub fn bad() -> Self {
        Self {
            label: ShotLabel::Bad,
            cs_time_ms: None,
            shot_delay_ms: None,
            overlap_time_ms: None,
        }
    }

    pub fn bad_with_timing(cs_time_ms: f64, shot_delay_ms: f64) -> Self {
        Self {
            label: ShotLabel::Bad,
            cs_time_ms: Some(cs_time_ms),
            shot_delay_ms: Some(shot_delay_ms),
            overlap_time_ms: None,
        }
    }

    pub fn display_text(&self) -> String {
        let mut lines = vec![format!("Classification: {}", self.label)];

        match self.label {
            ShotLabel::CounterStrafe => {
                if let (Some(cs_time), Some(shot_delay)) = (self.cs_time_ms, self.shot_delay_ms) {
                    lines.push(format!("CS time: {cs_time:.0} ms"));
                    lines.push(format!("Shot delay: {shot_delay:.0} ms"));
                }
            }
            ShotLabel::Overlap => {
                if let Some(overlap_time) = self.overlap_time_ms {
                    lines.push(format!("Overlap: {overlap_time:.0} ms"));
                }
            }
            ShotLabel::Bad => {
                if let (Some(cs_time), Some(shot_delay)) = (self.cs_time_ms, self.shot_delay_ms) {
                    lines.push(format!("CS time: {cs_time:.0} ms"));
                    lines.push(format!("Shot delay: {shot_delay:.0} ms"));
                }
            }
        }

        lines.join("\n")
    }
}

fn apply_counter_strafe_thresholds(base: ShotClassification) -> ShotClassification {
    match base.label {
        ShotLabel::Overlap => ShotClassification {
            label: ShotLabel::Overlap,
            overlap_time_ms: base.overlap_time_ms,
            cs_time_ms: None,
            shot_delay_ms: None,
        },
        ShotLabel::CounterStrafe => {
            if let (Some(cs_time), Some(shot_delay)) = (base.cs_time_ms, base.shot_delay_ms) {
                if counter_strafe_passes_thresholds(cs_time, shot_delay) {
                    ShotClassification::counter_strafe(cs_time, shot_delay)
                } else {
                    ShotClassification::bad_with_timing(cs_time, shot_delay)
                }
            } else {
                ShotClassification::bad()
            }
        }
        ShotLabel::Bad => {
            if let (Some(cs_time), Some(shot_delay)) = (base.cs_time_ms, base.shot_delay_ms) {
                ShotClassification::bad_with_timing(cs_time, shot_delay)
            } else {
                ShotClassification::bad()
            }
        }
    }
}

fn counter_strafe_passes_thresholds(cs_time_ms: f64, shot_delay_ms: f64) -> bool {
    shot_delay_ms <= 230.0 && (cs_time_ms <= 215.0 || shot_delay_ms <= 215.0)
}

#[derive(Clone, Debug)]
pub struct MovementClassifier {
    vertical: AxisState,
    horizontal: AxisState,
}

impl Default for MovementClassifier {
    fn default() -> Self {
        Self::new(['W', 'S'], ['A', 'D']).expect("default movement keys are valid")
    }
}

impl MovementClassifier {
    pub fn new(vertical_keys: [char; 2], horizontal_keys: [char; 2]) -> Result<Self, String> {
        validate_axis_keys(vertical_keys, "vertical")?;
        validate_axis_keys(horizontal_keys, "horizontal")?;
        validate_movement_keys(vertical_keys, horizontal_keys)?;

        Ok(Self {
            vertical: AxisState::new(vertical_keys.map(|key| key.to_ascii_uppercase())),
            horizontal: AxisState::new(horizontal_keys.map(|key| key.to_ascii_uppercase())),
        })
    }

    pub fn on_press(&mut self, key: char, timestamp_ms: f64) {
        let key = key.to_ascii_uppercase();

        if self.vertical.keys.contains(&key) {
            self.vertical.on_press(key, timestamp_ms);
        } else if self.horizontal.keys.contains(&key) {
            self.horizontal.on_press(key, timestamp_ms);
        }
    }

    pub fn on_release(&mut self, key: char, timestamp_ms: f64) {
        let key = key.to_ascii_uppercase();

        if self.vertical.keys.contains(&key) {
            self.vertical.on_release(key, timestamp_ms);
        } else if self.horizontal.keys.contains(&key) {
            self.horizontal.on_release(key, timestamp_ms);
        }
    }

    pub fn classify_shot(&mut self, shot_time_ms: f64) -> ShotClassification {
        let vertical = self.vertical.classify_shot(shot_time_ms);
        let horizontal = self.horizontal.classify_shot(shot_time_ms);
        let classification = choose_axis_classification(vertical, horizontal);

        let base = match classification {
            AxisClassification::CounterStrafe {
                cs_time_ms,
                shot_delay_ms,
            } => ShotClassification::counter_strafe(cs_time_ms, shot_delay_ms),
            AxisClassification::Overlap { overlap_time_ms } => {
                ShotClassification::overlap(overlap_time_ms)
            }
            AxisClassification::Bad => ShotClassification::bad(),
        };

        apply_counter_strafe_thresholds(base)
    }
}

fn validate_axis_keys(keys: [char; 2], axis_name: &str) -> Result<(), String> {
    let normalized = keys.map(|key| key.to_ascii_uppercase());
    if normalized[0] == normalized[1] {
        return Err(format!("{axis_name}_keys must contain two distinct keys"));
    }

    Ok(())
}

fn validate_movement_keys(
    vertical_keys: [char; 2],
    horizontal_keys: [char; 2],
) -> Result<(), String> {
    let mut keys = HashMap::new();

    for (axis_name, axis_keys) in [("vertical", vertical_keys), ("horizontal", horizontal_keys)] {
        for key in axis_keys {
            let key = key.to_ascii_uppercase();
            if let Some(existing_axis_name) = keys.insert(key, axis_name) {
                return Err(format!(
                    "movement_keys contains duplicate key '{key}' in {existing_axis_name} and {axis_name} axes"
                ));
            }
        }
    }

    Ok(())
}

fn choose_axis_classification(
    vertical: AxisClassification,
    horizontal: AxisClassification,
) -> AxisClassification {
    match vertical.selection_rank().cmp(&horizontal.selection_rank()) {
        std::cmp::Ordering::Greater => vertical,
        std::cmp::Ordering::Less => horizontal,
        std::cmp::Ordering::Equal => match (vertical.primary_time(), horizontal.primary_time()) {
            (Some(v_time), Some(h_time)) if v_time >= h_time => vertical,
            (Some(_), Some(_)) => horizontal,
            (Some(_), None) => vertical,
            _ => horizontal,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{MovementClassifier, ShotLabel, choose_axis_classification};

    #[test]
    fn classifies_counter_strafe_when_opposite_key_is_pressed_before_shot() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_release('A', 100.0);
        classifier.on_press('D', 132.0);

        let result = classifier.classify_shot(180.0);

        assert_eq!(result.label, ShotLabel::CounterStrafe);
        assert_eq!(result.cs_time_ms, Some(32.0));
        assert_eq!(result.shot_delay_ms, Some(48.0));
    }

    #[test]
    fn classifies_overlap_when_opposing_keys_are_held_before_shot() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_press('D', 24.0);

        let result = classifier.classify_shot(91.0);

        assert_eq!(result.label, ShotLabel::Overlap);
        assert_eq!(result.overlap_time_ms, Some(67.0));
    }

    #[test]
    fn delayed_counter_strafe_becomes_bad_with_timing_retained() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_release('A', 120.0);
        classifier.on_press('D', 150.0);

        let result = classifier.classify_shot(401.0);

        assert_eq!(result.label, ShotLabel::Bad);
        assert_eq!(result.cs_time_ms, Some(30.0));
        assert_eq!(result.shot_delay_ms, Some(251.0));
    }

    #[test]
    fn overlap_axis_outranks_counter_strafe_axis() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('W', 0.0);
        classifier.on_press('S', 25.0);
        classifier.on_press('A', 30.0);
        classifier.on_release('A', 100.0);
        classifier.on_press('D', 125.0);

        let result = classifier.classify_shot(160.0);

        assert_eq!(result.label, ShotLabel::Overlap);
        assert_eq!(result.overlap_time_ms, Some(135.0));
    }

    #[test]
    fn counter_strafe_survives_counter_key_release_before_shot() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_release('A', 100.0);
        classifier.on_press('D', 132.0);
        classifier.on_release('D', 170.0);

        let result = classifier.classify_shot(180.0);

        assert_eq!(result.label, ShotLabel::CounterStrafe);
        assert_eq!(result.cs_time_ms, Some(32.0));
        assert_eq!(result.shot_delay_ms, Some(48.0));
    }

    #[test]
    fn counter_strafe_after_overlap_survives_counter_key_release_before_shot() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_press('D', 20.0);
        classifier.on_release('D', 60.0);
        classifier.on_release('A', 100.0);
        classifier.on_press('D', 132.0);
        classifier.on_release('D', 150.0);

        let result = classifier.classify_shot(180.0);

        assert_eq!(result.label, ShotLabel::CounterStrafe);
        assert_eq!(result.cs_time_ms, Some(32.0));
        assert_eq!(result.shot_delay_ms, Some(48.0));
    }

    #[test]
    fn valid_axis_wins_when_other_counter_strafe_fails_thresholds() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('W', 0.0);
        classifier.on_release('W', 100.0);
        classifier.on_press('S', 300.0);
        classifier.on_press('A', 350.0);
        classifier.on_release('A', 400.0);
        classifier.on_press('D', 430.0);

        let result = classifier.classify_shot(540.0);

        assert_eq!(result.label, ShotLabel::CounterStrafe);
        assert_eq!(result.cs_time_ms, Some(30.0));
        assert_eq!(result.shot_delay_ms, Some(110.0));
    }

    #[test]
    fn classify_shot_applies_counter_strafe_thresholds() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_release('A', 120.0);
        classifier.on_press('D', 150.0);

        let result = classifier.classify_shot(401.0);

        assert_eq!(result.label, ShotLabel::Bad);
        assert_eq!(result.cs_time_ms, Some(30.0));
        assert_eq!(result.shot_delay_ms, Some(251.0));
    }

    #[test]
    fn duplicate_keys_across_axes_are_rejected() {
        let error = MovementClassifier::new(['W', 's'], ['S', 'D'])
            .expect_err("duplicate movement keys across axes should fail");

        assert_eq!(
            error,
            "movement_keys contains duplicate key 'S' in vertical and horizontal axes"
        );
    }

    #[test]
    fn held_overlap_is_reported_on_subsequent_shots() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_press('D', 24.0);

        let first_result = classifier.classify_shot(91.0);
        let second_result = classifier.classify_shot(120.0);

        assert_eq!(first_result.label, ShotLabel::Overlap);
        assert_eq!(first_result.overlap_time_ms, Some(67.0));
        assert_eq!(second_result.label, ShotLabel::Overlap);
        assert_eq!(second_result.overlap_time_ms, Some(96.0));
    }

    #[test]
    fn restarted_overlap_uses_fresh_overlap_start_time() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_press('D', 20.0);
        classifier.on_release('D', 100.0);
        classifier.on_press('D', 300.0);

        let result = classifier.classify_shot(350.0);

        assert_eq!(result.label, ShotLabel::Overlap);
        assert_eq!(result.overlap_time_ms, Some(50.0));
    }

    #[test]
    fn shot_after_overlap_ends_with_one_key_held_is_bad() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_press('D', 20.0);
        let first_result = classifier.classify_shot(80.0);
        classifier.on_release('A', 100.0);

        let result = classifier.classify_shot(140.0);

        assert_eq!(first_result.label, ShotLabel::Overlap);
        assert_eq!(first_result.overlap_time_ms, Some(60.0));
        assert_eq!(result.label, ShotLabel::Bad);
        assert_eq!(result.overlap_time_ms, None);
    }

    #[test]
    fn duplicate_press_after_overlap_does_not_create_clean_counter_strafe() {
        let mut classifier = MovementClassifier::default();

        classifier.on_press('A', 0.0);
        classifier.on_press('D', 20.0);
        classifier.on_release('A', 100.0);
        classifier.on_press('D', 110.0);

        let result = classifier.classify_shot(140.0);

        assert_eq!(result.label, ShotLabel::Bad);
        assert_eq!(result.cs_time_ms, None);
        assert_eq!(result.shot_delay_ms, None);
    }

    #[test]
    fn stray_release_does_not_start_counter_strafe_candidate() {
        let mut classifier = MovementClassifier::default();

        classifier.on_release('A', 100.0);
        classifier.on_press('D', 130.0);

        let result = classifier.classify_shot(160.0);

        assert_eq!(result.label, ShotLabel::Bad);
        assert_eq!(result.cs_time_ms, None);
        assert_eq!(result.shot_delay_ms, None);
    }

    #[test]
    fn equal_selection_rank_prefers_longer_primary_time() {
        let vertical = super::AxisClassification::Overlap {
            overlap_time_ms: 20.0,
        };
        let horizontal = super::AxisClassification::Overlap {
            overlap_time_ms: 80.0,
        };

        assert_eq!(choose_axis_classification(vertical, horizontal), horizontal);
    }
}
