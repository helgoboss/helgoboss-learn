use crate::{FireMode, Interval, UnitValue};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct PressDurationProcessor {
    // Configuration data (stays constant)
    fire_mode: FireMode,
    interval: Interval<Duration>,
    turbo_rate: Duration,
    // Runtime data (changes during usage)
    last_button_press: Option<ButtonPress>,
}

#[derive(Clone, Debug)]
struct ButtonPress {
    time: Instant,
    value: UnitValue,
    time_of_last_turbo_fire: Option<Instant>,
}

impl ButtonPress {
    pub fn new(value: UnitValue) -> Self {
        Self {
            time: Instant::now(),
            value,
            time_of_last_turbo_fire: None,
        }
    }
}

const ZERO_DURATION: Duration = Duration::from_millis(0);

impl Default for PressDurationProcessor {
    fn default() -> Self {
        Self {
            fire_mode: FireMode::WhenButtonReleased,
            interval: Interval::new(ZERO_DURATION, ZERO_DURATION),
            turbo_rate: ZERO_DURATION,
            last_button_press: None,
        }
    }
}

impl PressDurationProcessor {
    pub fn new(
        mode: FireMode,
        interval: Interval<Duration>,
        turbo_rate: Duration,
    ) -> PressDurationProcessor {
        PressDurationProcessor {
            fire_mode: mode,
            interval,
            turbo_rate,
            ..Default::default()
        }
    }

    /// Should be called once at initialization time to check if this processor wants that you call
    /// `poll()`, regularly.
    pub fn wants_to_be_polled(&self) -> bool {
        // This must not depend on the button press state!
        self.fire_mode.wants_to_be_polled()
    }

    pub fn process_press_or_release(&mut self, control_value: UnitValue) -> Option<UnitValue> {
        let min = self.interval.min_val();
        let max = self.interval.max_val();
        match self.fire_mode {
            FireMode::WhenButtonReleased => {
                if min == ZERO_DURATION && max == ZERO_DURATION {
                    // N-op case: Just fire immediately. If just min is zero, we don't fire
                    // immediately but wait for button release. That way we can support different
                    // stacked press durations (or just "fire on release" behavior no matter the
                    // press duration if user chooses max very high)!
                    return Some(control_value);
                }
                if control_value.get() > 0.0 {
                    // This is a button press.
                    // Don't fire now because we don't know yet how long it will be pressed.
                    self.last_button_press = Some(ButtonPress::new(control_value));
                    None
                } else {
                    // Looks like a button release.
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
                }
            }
            FireMode::AfterTimeout => {
                if min == ZERO_DURATION {
                    // No-op case: Fire immediately.
                    return Some(control_value);
                }
                if control_value.get() > 0.0 {
                    // Button press
                    self.last_button_press = Some(ButtonPress::new(control_value));
                    None
                } else {
                    // Button release
                    self.last_button_press = None;
                    None
                }
            }
            FireMode::AfterTimeoutKeepFiring => {
                if control_value.get() > 0.0 {
                    // Button press
                    let mut button_press = ButtonPress::new(control_value);
                    let result = if min == ZERO_DURATION {
                        // No initial delay. Fire immediately and count as first turbo fire!
                        button_press.time_of_last_turbo_fire = Some(Instant::now());
                        Some(control_value)
                    } else {
                        // Initial delay (wait for timeout).
                        None
                    };
                    self.last_button_press = Some(button_press);
                    result
                } else {
                    // Button release
                    self.last_button_press = None;
                    None
                }
            }
        }
    }

    /// Should be called regularly if `wants_to_be_polled()` returned `true` at initialization
    /// time.
    pub fn poll(&mut self) -> Option<UnitValue> {
        match self.fire_mode {
            FireMode::WhenButtonReleased => None,
            FireMode::AfterTimeout => {
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
            FireMode::AfterTimeoutKeepFiring => {
                let last_button_press = self.last_button_press.as_mut()?;
                if let Some(last_turbo) = last_button_press.time_of_last_turbo_fire {
                    // We are in turbo stage already.
                    if last_turbo.elapsed() >= self.turbo_rate {
                        // Subsequent turbo fire!
                        last_button_press.time_of_last_turbo_fire = Some(Instant::now());
                        Some(last_button_press.value)
                    } else {
                        // Not yet time for next turbo fire.
                        None
                    }
                } else if last_button_press.time.elapsed() >= self.interval.min_val() {
                    // We reached the initial delay. First turbo fire!
                    last_button_press.time_of_last_turbo_fire = Some(Instant::now());
                    Some(last_button_press.value)
                } else {
                    None
                }
            }
        }
    }
}
