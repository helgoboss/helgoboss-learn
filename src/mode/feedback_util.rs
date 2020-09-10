use crate::{
    Interval, MinIsMaxBehavior, OutOfRangeBehavior, Transformation, UnitValue, FEEDBACK_EPSILON,
};

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
    let (target_bound_value, min_is_max_behavior) =
        if target_value_interval.contains(rounded_target_value) {
            // Feedback value is within target value interval
            (rounded_target_value, MinIsMaxBehavior::PreferOne)
        } else {
            // Feedback value is outside target value interval
            use OutOfRangeBehavior::*;
            match out_of_range_behavior {
                MinOrMax => {
                    if rounded_target_value < target_value_interval.min_val() {
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
    let potentially_inversed_value = if reverse {
        target_bound_value.inverse()
    } else {
        target_bound_value
    };
    let transformed_value = transformation
        .as_ref()
        .and_then(|t| {
            t.transform(potentially_inversed_value, potentially_inversed_value)
                .ok()
        })
        .unwrap_or(potentially_inversed_value);
    let full_interval_value =
        transformed_value.map_to_unit_interval_from(target_value_interval, min_is_max_behavior);
    let source_value = full_interval_value.map_from_unit_interval_to(source_value_interval);
    Some(source_value)
}
