/// Context for mode-related functions.
#[derive(Copy, Clone, Debug, Default)]
pub struct ModeContext<A> {
    pub additional_script_input: A,
}
