use crate::UnitValue;

pub trait Target {
    /// Should return the current value of the target.
    fn get_current_value(&self) -> UnitValue;

    /// Should return the atomic (minimum) step size if any. Usually there is some if the target
    /// character is discrete. But some targets are continuous in nature and it still makes sense to
    /// offer discrete steps. Imagine a "tempo" target: Musical tempo is continuous in nature and
    /// still you might want to offer the possibility to round on fraction-less bpm values.
    ///
    /// The returned value must be part of the unit interval (something from 0.0 to 1.0). Although 1
    /// doesn't really make sense because that would mean the step size covers the whole interval.
    fn get_step_size(&self) -> Option<UnitValue>;

    /// Should return `true` if this target doesn't want to be hit with absolute values but with
    /// relative increments.
    fn wants_increments(&self) -> bool;
}
