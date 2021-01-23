use crate::{Interval, MinIsMaxBehavior, OutOfRangeBehavior, Transformation, UnitValue};

/// When interpreting target value, make only 4 fractional digits matter.
///
/// If we don't do this and target min == target max, even the slightest imprecision of the actual
/// target value (which in practice often occurs with FX parameters not taking exactly the desired
/// value) could result in a totally different feedback value. Maybe it would be better to determine
/// the epsilon dependent on the source precision (e.g. 1.0/128.0 in case of short MIDI messages)
/// but right now this should suffice to solve the immediate problem.  
pub const FEEDBACK_EPSILON: f64 = 0.00001;

pub fn feedback<T: Transformation>(
    target_value: UnitValue,
    reverse: bool,
    transformation: &Option<T>,
    source_value_interval: &Interval<UnitValue>,
    target_value_interval: &Interval<UnitValue>,
    out_of_range_behavior: OutOfRangeBehavior,
) -> Option<UnitValue> {
    let rounded_target_value =
        UnitValue::new_clamped((target_value.get() / FEEDBACK_EPSILON).round() * FEEDBACK_EPSILON);
    // 1. Apply reverse
    let potentially_inversed_value = if reverse {
        rounded_target_value.inverse()
    } else {
        rounded_target_value
    };
    let potentially_reversed_target_interval = if reverse {
        target_value_interval.inverse()
    } else {
        *target_value_interval
    };
    // 2. Apply target interval
    let (target_bound_value, min_is_max_behavior) =
        if potentially_reversed_target_interval.contains(potentially_inversed_value) {
            // Feedback value is within target value interval
            (potentially_inversed_value, MinIsMaxBehavior::PreferOne)
        } else {
            // Feedback value is outside target value interval
            use OutOfRangeBehavior::*;
            match out_of_range_behavior {
                MinOrMax => {
                    if potentially_inversed_value < potentially_reversed_target_interval.min_val() {
                        (
                            potentially_reversed_target_interval.min_val(),
                            MinIsMaxBehavior::PreferZero,
                        )
                    } else {
                        (
                            potentially_reversed_target_interval.max_val(),
                            MinIsMaxBehavior::PreferOne,
                        )
                    }
                }
                Min => (
                    potentially_reversed_target_interval.min_val(),
                    MinIsMaxBehavior::PreferZero,
                ),
                Ignore => return None,
            }
        };
    let full_interval_value = target_bound_value
        .map_to_unit_interval_from(&potentially_reversed_target_interval, min_is_max_behavior);
    // 3. Apply transformation
    let transformed_value = transformation
        .as_ref()
        .and_then(|t| t.transform(full_interval_value, full_interval_value).ok())
        .unwrap_or(full_interval_value);
    // 4. Apply source interval
    let source_value = transformed_value.map_from_unit_interval_to(source_value_interval);
    Some(source_value)
}
