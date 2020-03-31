use crate::UnitValue;

pub trait Target {
    fn get_current_value(&self) -> UnitValue;

    // Returns None if no minimum step size. Usually if the target character is not discrete. But
    // some targets are continuous in nature but it still makes sense to have discrete steps,
    // for example tempo in bpm.
    // This value should be something from 0 to 1. Although 1 doesn't really make sense because that
    // would mean the step size covers the whole interval, so the target has just one possible
    // value.
    fn get_step_size(&self) -> Option<UnitValue>;

    fn wants_to_be_hit_with_increments(&self) -> bool;

    // Should be a rather high value like e.g. 63 (meaning one target has 63 different discrete
    // values)
    fn get_discrete_values_count(&self) -> Option<u32> {
        let step_size = self.get_step_size()?;
        Some((1.0 / step_size.get_number()).floor() as u32)
    }

    fn round_to_nearest_discrete_value(&self, approximate_control_value: UnitValue) -> UnitValue {
        match self.get_step_size() {
            None => approximate_control_value,
            Some(step_size) => approximate_control_value.round_by_grid_interval_size(step_size),
        }
    }
}
