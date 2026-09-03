/// Returns an appropriate signum depending on the given condition.
pub(crate) fn negative_if(condition: bool) -> i32 {
    if condition { -1 } else { 1 }
}
