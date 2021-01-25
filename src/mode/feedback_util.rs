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
    // 5. Apply rounding (though in a different way than in control direction)
    let v =
        UnitValue::new_clamped((target_value.get() / FEEDBACK_EPSILON).round() * FEEDBACK_EPSILON);
    // 4. Apply target interval
    let (mut v, min_is_max_behavior) = if target_value_interval.contains(v) {
        // Feedback value is within target value interval
        (v, MinIsMaxBehavior::PreferOne)
    } else {
        // Feedback value is outside target value interval
        use OutOfRangeBehavior::*;
        match out_of_range_behavior {
            MinOrMax => {
                if v < target_value_interval.min_val() {
                    (
                        target_value_interval.min_val(),
                        MinIsMaxBehavior::PreferZero,
                    )
                } else {
                    (target_value_interval.max_val(), MinIsMaxBehavior::PreferOne)
                }
            }
            Min => (
                target_value_interval.min_val(),
                MinIsMaxBehavior::PreferZero,
            ),
            Ignore => return None,
        }
    };
    v = v.map_to_unit_interval_from(&target_value_interval, min_is_max_behavior);
    // 3. Apply reverse
    v = if reverse { v.inverse() } else { v };
    // 2. Apply transformation
    v = transformation
        .as_ref()
        .and_then(|t| t.transform(v, v).ok())
        .unwrap_or(v);
    // 1. Apply source interval
    Some(v.map_from_unit_interval_to(source_value_interval))
}
