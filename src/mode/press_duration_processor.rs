use crate::{FireMode, Interval, UnitValue};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct PressDurationProcessor {
    fire_mode: FireMode,
    interval: Interval<Duration>,
    last_button_press: Option<ButtonPress>,
}

#[derive(Clone, Debug)]
struct ButtonPress {
    time: Instant,
    value: UnitValue,
}

const ZERO_DURATION: Duration = Duration::from_millis(0);

impl Default for PressDurationProcessor {
    fn default() -> Self {
        Self {
            fire_mode: FireMode::WhenButtonReleased,
            interval: Interval::new(ZERO_DURATION, ZERO_DURATION),
            last_button_press: None,
        }
    }
}

impl PressDurationProcessor {
    pub fn new(mode: FireMode, interval: Interval<Duration>) -> PressDurationProcessor {
        PressDurationProcessor {
            fire_mode: mode,
            interval,
            last_button_press: None,
        }
    }

    pub fn wants_to_be_polled(&self) -> bool {
        self.fire_mode.wants_to_be_polled()
    }

    pub fn poll(&mut self) -> Option<UnitValue> {
        let fire_value = {
            let last_button_press = self.last_button_press.as_ref()?;
            if last_button_press.time.elapsed() >= self.interval.min_val() {
                Some(last_button_press.value)
            } else {
                None
            }
        };
        if fire_value.is_some() {
            self.last_button_press = None;
        }
        fire_value
    }

    pub fn process_press_or_release(&mut self, control_value: UnitValue) -> Option<UnitValue> {
        let min = self.interval.min_val();
        let max = self.interval.max_val();
        if min == ZERO_DURATION && max == ZERO_DURATION {
            // Standard case: Just fire immediately.
            return Some(control_value);
        }
        if control_value.is_zero() {
            // Looks like a button release.
            if self.fire_mode == FireMode::WhenButtonReleased {
                // Measure duration since button press.
                match std::mem::replace(&mut self.last_button_press, None) {
                    // Button has not been pressed before. Just ignore.
                    None => None,
                    // Button has been pressed before.
                    Some(press) => {
                        if self.interval.contains(press.time.elapsed()) {
                            // Duration within interval. Fire initial press value.
                            Some(press.value)
                        } else {
                            // Released too early or too late.
                            None
                        }
                    }
                }
            } else {
                self.last_button_press = None;
                None
            }
        } else {
            // This is a button press. Don't fire now because we don't know yet how long
            // it will be pressed.
            self.last_button_press = Some(ButtonPress {
                time: Instant::now(),
                value: control_value,
            });
            None
        }
    }
}
