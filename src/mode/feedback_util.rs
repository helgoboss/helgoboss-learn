use crate::{
    Interval, IntervalMatchResult, OutOfRangeBehavior, Transformation, UnitValue, BASE_EPSILON,
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
    target_value: UnitValue,
    reverse: bool,
    transformation: &Option<T>,
    source_value_interval: &Interval<UnitValue>,
    target_value_interval: &Interval<UnitValue>,
    out_of_range_behavior: OutOfRangeBehavior,
) -> Option<UnitValue> {
    let mut v = target_value;
    // 4. Apply target interval
    // Tolerant interval bounds test because of https://github.com/helgoboss/realearn/issues/263.
    // TODO-medium The most elaborate solution to deal with discrete values would be to actually
    //  know which interval of floating point values represents a specific discrete target value.
    //  However, is there a generic way to know that? Taking the target step size as epsilon in this
    //  case sounds good but we still don't know if the target respects approximate values, if it
    //  rounds them or uses more a ceil/floor approach ... I don't think this is standardized for
    //  VST parameters. We could solve it for our own parameters in future. Until then, having a
    //  fixed epsilon deals at least with most issues I guess.
    v = {
        use IntervalMatchResult::*;
        match target_value_interval.value_matches_tolerant(v, FEEDBACK_EPSILON) {
            Between => UnitValue::new_clamped(
                (v - target_value_interval.min_val()) / target_value_interval.span(),
            ),
            MinAndMax => UnitValue::MAX,
            Min => UnitValue::MIN,
            Max => UnitValue::MAX,
            Lower => match out_of_range_behavior {
                OutOfRangeBehavior::MinOrMax | OutOfRangeBehavior::Min => UnitValue::MIN,
                OutOfRangeBehavior::Ignore => return None,
            },
            Greater => match out_of_range_behavior {
                OutOfRangeBehavior::MinOrMax => UnitValue::MAX,
                OutOfRangeBehavior::Min => UnitValue::MIN,
                OutOfRangeBehavior::Ignore => return None,
            },
        }
    };
    // 3. Apply reverse
    v = if reverse { v.inverse() } else { v };
    // 2. Apply transformation
    v = transformation
        .as_ref()
        .and_then(|t| t.transform_continuous(v, v).ok())
        .unwrap_or(v);
    // 1. Apply source interval
    Some(v.denormalize(source_value_interval))
}
