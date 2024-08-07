use crate::{AbsoluteValue, ButtonUsage, FireMode, Interval};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct PressDurationProcessor {
    // # Configuration data (stays constant)
    fire_mode: FireMode,
    interval: Interval<Duration>,
    /// Double press detection: How long to wait for a second press
    multi_press_span: Duration,
    turbo_rate: Duration,
    // # Runtime data (changes during usage)
    last_button_press: Option<ButtonPress>,
    button_usage: ButtonUsage,
}

#[derive(Clone, Debug)]
struct ButtonPress {
    time: Instant,
    value: AbsoluteValue,
    /// Used for after-timeout-keep-firing mode.
    time_of_last_turbo_fire: Option<Instant>,
    /// Whether we already fired in response to this press.
    ///
    /// Important for after-timeout mode: We must not clear the press on first fire, otherwise we can't
    /// decide anymore what will happen on release.
    fired_already: bool,
    /// Number of tap-downs so far. Used for double-press detection.
    tap_down_count: u32,
    /// Whether the button has been released already.
    ///
    /// This is relevant for distinction between single and double press. A button press that is
    /// released after a short time can still develop into a double press, so we can't clear the press yet.
    released: bool,
}

impl ButtonPress {
    pub fn new(value: AbsoluteValue) -> Self {
        Self {
            time: Instant::now(),
            value,
            time_of_last_turbo_fire: None,
            fired_already: false,
            tap_down_count: 1,
            released: false,
        }
    }
}

const ZERO_DURATION: Duration = Duration::from_millis(0);

impl Default for PressDurationProcessor {
    fn default() -> Self {
        Self {
            fire_mode: FireMode::Normal,
            interval: Interval::new(ZERO_DURATION, ZERO_DURATION),
            multi_press_span: Duration::from_millis(300),
            turbo_rate: ZERO_DURATION,
            last_button_press: None,
            button_usage: ButtonUsage::Both,
        }
    }
}

impl PressDurationProcessor {
    pub fn new(
        mode: FireMode,
        interval: Interval<Duration>,
        turbo_rate: Duration,
        button_usage: ButtonUsage,
    ) -> PressDurationProcessor {
        PressDurationProcessor {
            fire_mode: mode,
            interval,
            turbo_rate,
            button_usage,
            ..Default::default()
        }
    }

    /// Should be called once at initialization time to check if this processor wants that you call
    /// `poll()`, regularly.
    pub fn wants_to_be_polled(&self) -> bool {
        // This must not depend on the button press state!
        use FireMode::*;
        match self.fire_mode {
            AfterTimeout | AfterTimeoutKeepFiring | OnSinglePress => true,
            Normal | OnDoublePress => false,
        }
    }

    pub fn process_press_or_release(
        &mut self,
        control_value: AbsoluteValue,
        button_usage: ButtonUsage,
    ) -> Option<AbsoluteValue> {
        let min = self.interval.min_val();
        let max = self.interval.max_val();
        match self.fire_mode {
            FireMode::Normal => {
                // In the past, the button usage setting was always checked before processing the press duration.
                // In normal fire mode, we keep doing it that way (although the button setting actually doesn't make
                // sense in case of min/max being > 0).
                if button_usage.should_ignore(control_value) {
                    return None;
                }
                if min == ZERO_DURATION && max == ZERO_DURATION {
                    // No-op case: Just fire immediately. If just min is zero, we don't fire
                    // immediately but wait for button release. That way we can support different
                    // stacked press durations (or just "fire on release" behavior no matter the
                    // press duration if user chooses max very high)!
                    return Some(control_value);
                }
                if control_value.is_on() {
                    // This is a button press.
                    // Don't fire now because we don't know yet how long it will be pressed.
                    self.last_button_press = Some(ButtonPress::new(control_value));
                    None
                } else {
                    // Looks like a button release.
                    // Measure duration since button press.
                    match self.last_button_press.take() {
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
                // This fire mode has been improved in 2.16.0 to let button release fire 0% if not prevented
                // by button usage setting.
                if min == ZERO_DURATION {
                    // No-op case: Fire immediately.
                    if button_usage.should_ignore(control_value) {
                        return None;
                    }
                    return Some(control_value);
                }
                if control_value.is_on() {
                    // Button press
                    self.last_button_press = Some(ButtonPress::new(control_value));
                    None
                } else {
                    // Button release
                    self.process_timeout_button_release(control_value)
                }
            }
            FireMode::AfterTimeoutKeepFiring => {
                // In the past, the button usage setting was always checked before processing the press duration.
                // We should keep doing it that way in order to not destroy existing setups. Also, that makes it
                // possible to keep firing even after releasing a button!
                if button_usage.should_ignore(control_value) {
                    return None;
                }
                if control_value.is_on() {
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
                    self.process_timeout_button_release(control_value)
                }
            }
            FireMode::OnSinglePress => {
                // Button usage setting doesn't make sense here. We need to process both press and release but only
                // output press. That's why we started hiding the dropdown in 2.16.1. If someone has previously used
                // the button filter together with this fire mode, it would have been a weird misconfiguration,
                // qualifying as "undefined behavior". Breaking change is okay.
                if control_value.is_on() {
                    // Button press
                    if let Some(press) = self.last_button_press.as_mut() {
                        // Must be more than single press already.
                        press.tap_down_count += 1;
                        press.time = Instant::now();
                    } else {
                        // First press
                        self.last_button_press = Some(ButtonPress::new(control_value));
                    };
                    None
                } else {
                    // Button release.
                    let fire_value = {
                        let press = self.last_button_press.as_mut()?;
                        if press.tap_down_count != 1 {
                            return None;
                        }
                        let elapsed = press.time.elapsed();
                        if elapsed < self.multi_press_span {
                            press.released = true;
                            return None;
                        }
                        if self.interval.max_val() != ZERO_DURATION
                            && elapsed > self.interval.max_val()
                        {
                            // Exceeded max press time
                            return None;
                        }
                        press.value
                    };
                    self.last_button_press = None;
                    Some(fire_value)
                }
            }
            FireMode::OnDoublePress => {
                // Button usage setting doesn't make sense here. We need to process both press and release but only
                // output press. That's why we started hiding the dropdown in 2.16.1. If someone has previously used
                // the button filter together with this fire mode, it would have been a weird misconfiguration,
                // qualifying as "undefined behavior". Breaking change is okay.
                if control_value.is_on() {
                    if let Some(press) = &self.last_button_press {
                        // Button was pressed before
                        let (result, next_press) = if press.time.elapsed() <= self.multi_press_span
                        {
                            // Double press detected
                            (Some(press.value), None)
                        } else {
                            // Previous press too long in past. Handle just like first press.
                            (None, Some(ButtonPress::new(control_value)))
                        };
                        self.last_button_press = next_press;
                        result
                    } else {
                        // First press
                        self.last_button_press = Some(ButtonPress::new(control_value));
                        None
                    }
                } else {
                    // Button release
                    None
                }
            }
        }
    }

    /// Should be called regularly if `wants_to_be_polled()` returned `true` at initialization
    /// time.
    pub fn poll(&mut self) -> Option<AbsoluteValue> {
        match self.fire_mode {
            FireMode::Normal | FireMode::OnDoublePress => None,
            FireMode::AfterTimeout => {
                let last_button_press = self.last_button_press.as_mut()?;
                if last_button_press.fired_already
                    || last_button_press.time.elapsed() < self.interval.min_val()
                {
                    return None;
                }
                last_button_press.fired_already = true;
                Some(last_button_press.value)
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
            FireMode::OnSinglePress => {
                let fire_value = {
                    let press = self.last_button_press.as_ref()?;
                    let elapsed = press.time.elapsed();
                    if elapsed < self.multi_press_span {
                        // Can't decide yet if this is a single press.
                        return None;
                    }
                    if self.interval.max_val() > ZERO_DURATION && !press.released {
                        // The button is still being hold.
                        if elapsed > self.interval.max_val() {
                            // The maximum hold time is already exceeded. Reset!
                            self.last_button_press = None;
                        }
                        return None;
                    }
                    if press.tap_down_count > 1 {
                        // Button was pressed more than one time and waiting time is over. Reset!
                        self.last_button_press = None;
                        return None;
                    }
                    press.value
                };
                self.last_button_press = None;
                Some(fire_value)
            }
        }
    }

    fn process_timeout_button_release(
        &mut self,
        control_value: AbsoluteValue,
    ) -> Option<AbsoluteValue> {
        let last_button_press = self.last_button_press.take()?;
        if self.button_usage == ButtonUsage::PressOnly {
            return None;
        }
        if last_button_press.time.elapsed() < self.interval.min_val() {
            return None;
        }
        Some(control_value)
    }
}
