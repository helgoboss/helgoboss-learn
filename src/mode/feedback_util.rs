use crate::ControlValue::AbsoluteContinuous;
use crate::{
    AbsoluteValue, Interval, IntervalMatchResult, MinIsMaxBehavior, OutOfRangeBehavior,
    Transformation, UnitValue, BASE_EPSILON,
};

/// When interpreting target value, make only 4 fractional digits matter.
///
/// If we don't do this and target min == target max, even the slightest imprecision of the actual
/// target value (which in practice often occurs with FX parameters not taking exactly the desired
/// value) could result in a totally different feedback value. Maybe it would be better to determine
/// the epsilon dependent on the source precision (e.g. 1.0/128.0 in case of short MIDI messages)
/// but right now this should suffice to solve the immediate problem.  
pub const FEEDBACK_EPSILON: f64 = BASE_EPSILON;

pub(crate) fn feedback<T: Transformation>(
    target_value: AbsoluteValue,
    reverse: bool,
    transformation: &Option<T>,
    source_value_interval: &Interval<UnitValue>,
    discrete_source_value_interval: &Interval<u32>,
    target_value_interval: &Interval<UnitValue>,
    discrete_target_value_interval: &Interval<u32>,
    out_of_range_behavior: OutOfRangeBehavior,
    is_discrete_mode: bool,
) -> Option<AbsoluteValue> {
    // Filter
    let interval_match_result = target_value.matches_tolerant(
        target_value_interval,
        discrete_target_value_interval,
        FEEDBACK_EPSILON,
    );
    let (target_bound_value, min_is_max_behavior) = if interval_match_result.matches() {
        // Target value is within target value interval
        (target_value, MinIsMaxBehavior::PreferOne)
    } else {
        // Target value is outside target value interval
        out_of_range_behavior.process(
            target_value,
            interval_match_result,
            target_value_interval,
            discrete_target_value_interval,
        )?
    };
    // 4. Apply target interval
    // Tolerant interval bounds test because of https://github.com/helgoboss/realearn/issues/263.
    // TODO-medium The most elaborate solution to deal with discrete values would be to actually
    //  know which interval of floating point values represents a specific discrete target value.
    //  However, is there a generic way to know that? Taking the target step size as epsilon in this
    //  case sounds good but we still don't know if the target respects approximate values, if it
    //  rounds them or uses more a ceil/floor approach ... I don't think this is standardized for
    //  VST parameters. We could solve it for our own parameters in future. Until then, having a
    //  fixed epsilon deals at least with most issues I guess.
    let mut v = target_bound_value.normalize(
        target_value_interval,
        discrete_target_value_interval,
        min_is_max_behavior,
        is_discrete_mode,
        FEEDBACK_EPSILON,
    );
    // 3. Apply reverse
    v = if reverse {
        v.inverse(discrete_target_value_interval.span())
    } else {
        v
    };
    // 2. Apply transformation
    if let Some(transformation) = transformation.as_ref() {
        if let Ok(res) = v.transform(transformation, Some(v), is_discrete_mode) {
            v = res;
        }
    };
    // 1. Apply source interval
    Some(v.denormalize(
        source_value_interval,
        discrete_source_value_interval,
        is_discrete_mode,
    ))
}
