use crate::{ControlValue, Interval, UnitValue};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct PressDurationProcessor {
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
            interval: Interval::new(ZERO_DURATION, ZERO_DURATION),
            last_button_press: None,
        }
    }
}

impl PressDurationProcessor {
    pub fn new(interval: Interval<Duration>) -> PressDurationProcessor {
        PressDurationProcessor {
            interval,
            last_button_press: None,
        }
    }

    pub fn process(&mut self, control_value: UnitValue) -> Option<UnitValue> {
        let min = self.interval.min_val();
        let max = self.interval.max_val();
        if min == ZERO_DURATION && max == ZERO_DURATION {
            // Standard case: Just fire immediately.
            return Some(control_value);
        }
        if control_value.is_zero() {
            // Looks like a button release. Measure duration since button press.
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
