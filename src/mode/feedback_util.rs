use crate::{Interval, Transformation, UnitValue};

pub fn feedback<T: Transformation>(
    target_value: UnitValue,
    reverse: bool,
    transformation: &Option<T>,
    source_value_interval: &Interval<UnitValue>,
    target_value_interval: &Interval<UnitValue>,
) -> UnitValue {
    let potentially_inversed_value = if reverse {
        target_value.inverse()
    } else {
        target_value
    };
    let transformed_value = transformation
        .as_ref()
        .and_then(|t| t.transform(potentially_inversed_value).ok())
        .unwrap_or(potentially_inversed_value);
    transformed_value
        .map_to_unit_interval_from(target_value_interval)
        .map_from_unit_interval_to(source_value_interval)
}
