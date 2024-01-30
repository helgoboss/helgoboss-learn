/// Context for source-related functions.
#[derive(Copy, Clone, Debug, Default)]
pub struct SourceContext<A> {
    pub additional_script_input: A,
}
