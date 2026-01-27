use crate::AbsoluteMode::PerformanceControl;
use crate::{AbsoluteMode, FireMode, GroupInteraction, OutOfRangeBehavior};
use derive_more::Display;
use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(Copy, Clone, Eq, PartialEq, Debug, Display, TryFromPrimitive, IntoPrimitive)]
#[repr(isize)]
pub enum DetailedSourceCharacter {
    /// Feature-wise a superset of `MomentaryOnOffButton` and `PressOnlyButton`.
    #[display(fmt = "Momentary velocity-sensitive button")]
    MomentaryVelocitySensitiveButton,
    /// Feature-wise a superset of `PressOnlyButton`.
    #[display(fmt = "Momentary on/off button")]
    MomentaryOnOffButton,
    /// Doesn't send message on release ("Toggle-only button").
    #[display(fmt = "Trigger (doesn't fire on release)")]
    Trigger,
    #[display(fmt = "Range control element (e.g. knob or fader)")]
    RangeControl,
    #[display(fmt = "Relative control element (e.g. encoder)")]
    Relative,
}

impl DetailedSourceCharacter {
    fn is_button(self) -> bool {
        use DetailedSourceCharacter as C;
        matches!(
            self,
            C::MomentaryOnOffButton | C::MomentaryVelocitySensitiveButton | C::Trigger
        )
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ModeApplicabilityCheckInput {
    pub target_is_virtual: bool,
    pub target_supports_discrete_values: bool,
    pub control_transformation_uses_time: bool,
    pub control_transformation_produces_relative_values: bool,
    pub is_feedback: bool,
    pub make_absolute: bool,
    pub use_textual_feedback: bool,
    pub source_character: DetailedSourceCharacter,
    pub absolute_mode: AbsoluteMode,
    pub target_value_sequence_is_set: bool,
    pub fire_mode: FireMode,
}

impl ModeApplicabilityCheckInput {
    pub fn source_is_button(&self) -> bool {
        self.source_character.is_button()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Display)]
pub enum ModeParameter {
    #[display(fmt = "Use discrete processing (prevents scaling)")]
    UseDiscreteProcessing,
    #[display(fmt = "Source min/max")]
    SourceMinMax,
    #[display(fmt = "Reverse")]
    Reverse,
    #[display(fmt = "Out-of-range behavior")]
    OutOfRangeBehavior,
    #[display(fmt = "Out-of-range behavior \"{_0}\"")]
    SpecificOutOfRangeBehavior(OutOfRangeBehavior),
    #[display(fmt = "Jump min/max")]
    JumpMinMax,
    #[display(fmt = "Takeover mode")]
    TakeoverMode,
    #[display(fmt = "Control transformation")]
    ControlTransformation,
    #[display(fmt = "Target value sequence")]
    TargetValueSequence,
    #[display(fmt = "Target min/max")]
    TargetMinMax,
    #[display(fmt = "Feedback transformation")]
    FeedbackTransformation,
    #[display(fmt = "Textual feedback expression")]
    TextualFeedbackExpression,
    #[display(fmt = "Step size min")]
    StepSizeMin,
    #[display(fmt = "Step size max")]
    StepSizeMax,
    #[display(fmt = "Speed min")]
    StepFactorMin,
    #[display(fmt = "Speed max")]
    StepFactorMax,
    #[display(fmt = "Relative filter")]
    RelativeFilter,
    #[display(fmt = "Wrap")]
    Rotate,
    #[display(fmt = "Fire mode")]
    FireMode,
    #[display(fmt = "Fire mode \"{_0}\"")]
    SpecificFireMode(FireMode),
    #[display(fmt = "Button filter")]
    ButtonFilter,
    #[display(fmt = "Make absolute")]
    MakeAbsolute,
    #[display(fmt = "Feedback type")]
    FeedbackType,
    #[display(fmt = "Round target value")]
    RoundTargetValue,
    #[display(fmt = "Absolute mode")]
    AbsoluteMode,
    #[display(fmt = "Absolute mode \"{_0}\"")]
    SpecificAbsoluteMode(AbsoluteMode),
    #[display(fmt = "Group interaction")]
    GroupInteraction,
    #[display(fmt = "Group interaction \"{_0}\"")]
    SpecificGroupInteraction(GroupInteraction),
}

#[derive(Copy, Clone, Debug)]
pub enum ModeApplicability {
    /// Parameter is completely ignored.
    HasNoEffect,
    /// Makes no sense but not ignored. A sensible "no-op" default should be used.
    MakesNoSenseUseDefault,
    /// Doesn't make sense. Used for variants of an enum (e.g. AbsoluteMode "Toggle button") to
    /// document that the applicability check of the enum itself (e.g. AbsoluteMode) will take care
    /// of choosing the correct default.
    MakesNoSenseParentTakesCareOfDefault,
    /// Parameter is relevant. Contains description of what it does.
    ///
    /// The contained text is not in use anymore (is now part of the ReaLearn Reference)!
    MakesSense(&'static str),
    /// Has an effect but a rather undesired one. The description should suggest alternatives if
    /// possible.
    ///
    /// The contained text is not in use anymore (is now part of the ReaLearn Reference)!
    Awkward(&'static str),
}

impl ModeApplicability {
    pub fn hint(self) -> Option<&'static str> {
        use ModeApplicability as A;
        match self {
            A::HasNoEffect
            | A::MakesNoSenseUseDefault
            | A::MakesNoSenseParentTakesCareOfDefault => None,
            A::MakesSense(h) | A::Awkward(h) => Some(h),
        }
    }

    pub fn is_relevant(self) -> bool {
        use ModeApplicability as A;
        matches!(self, A::MakesSense(_) | A::Awkward(_))
    }
}

const STEP_SIZE_MIN_FOR_RANGE_DESC: &str =
    "Sets the target value change amount for an incoming non-accelerated increment/decrement.";
const SPEED_MIN_FOR_RANGE_DESC: &str =
    "Sets the number of target increments for an incoming non-accelerated increment/decrement.";
const STEP_SIZE_MAX_FOR_RANGE_DESC: &str =
    "Sets the target value change amount for an incoming most accelerated increment/decrement.";
const SPEED_MAX_FOR_RANGE_DESC: &str =
    "Sets the number of target increments for an incoming most accelerated increment/decrement.";

const ROTATE_FOR_RANGE_DESC: &str = "If enabled, jumps from max target value to min target value for increments (opposite for decrements). Was called \"Rotate\" before.";

const NORMAL_ABSOLUTE_MODE_FOR_RANGE_DESC: &str = "Sets target to the value that corresponds to the knob/fader position. Proportionally maps from source to target range.";

pub fn check_mode_applicability(
    mode_parameter: ModeParameter,
    input: ModeApplicabilityCheckInput,
) -> ModeApplicability {
    use ModeApplicability::*;
    use ModeParameter::*;
    match mode_parameter {
        UseDiscreteProcessing => {
            if input.target_supports_discrete_values {
                MakesSense("By default, ReaLearn uses continuous processing logic. That means it considers all values as percentages and scales (stretches/squeezes) them as needed. If your target is discrete, you can enable discrete processing, which will prevent scaling and deliver your control value to the target as discrete integer (and vice versa in the feedback direction).")
            } else {
                MakesNoSenseUseDefault
            }
        }
        SourceMinMax => {
            if input.is_feedback {
                if input.use_textual_feedback {
                    HasNoEffect
                } else {
                    use DetailedSourceCharacter::*;
                    match input.source_character {
                        MomentaryVelocitySensitiveButton | MomentaryOnOffButton => {
                            MakesSense("Changes off/on LED colors.")
                        }
                        Trigger => MakesNoSenseUseDefault,
                        RangeControl | Relative => MakesSense(
                            "Changes lowest/highest position of motorized fader or LED ring.",
                        ),
                    }
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    Trigger => MakesNoSenseUseDefault,
                    MomentaryOnOffButton => {
                        if input.absolute_mode == crate::AbsoluteMode::Normal {
                            Awkward(
                                "If min > 0 and out-of-range behavior is \"Ignore\", button releases are ignored. Also affects feedback. It's usually better to use the dedicated button filter (e.g. \"Press only\").",
                            )
                        } else {
                            // Releases don't have an effect anyway with incremental and toggle
                            // mode.
                            HasNoEffect
                        }
                    }
                    MomentaryVelocitySensitiveButton => {
                        MakesSense("Defines the observed button press velocity range.")
                    }
                    RangeControl | Relative => {
                        if input.source_character == RangeControl || input.make_absolute {
                            MakesSense("Defines the observed fader/knob position range.")
                        } else {
                            HasNoEffect
                        }
                    }
                }
            }
        }
        Reverse => {
            if input.is_feedback {
                if input.use_textual_feedback {
                    HasNoEffect
                } else if input.source_is_button() {
                    MakesSense(
                        "If enabled, uses \"off\" LED color if target is on and \"on\" LED color if target is off.",
                    )
                } else {
                    MakesSense("If enabled, reverses the direction of motorized fader or LED ring.")
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | MomentaryVelocitySensitiveButton | Trigger => {
                        match input.absolute_mode {
                            crate::AbsoluteMode::Normal => MakesSense(
                                "If enabled, switches the target off when pressed and on when released.",
                            ),
                            crate::AbsoluteMode::IncrementalButton => MakesSense(
                                "If enabled, decreases the target value on press instead of increasing it.",
                            ),
                            crate::AbsoluteMode::ToggleButton => MakesNoSenseUseDefault,
                            crate::AbsoluteMode::MakeRelative => MakesNoSenseUseDefault,
                            crate::AbsoluteMode::PerformanceControl => MakesNoSenseUseDefault,
                        }
                    }
                    RangeControl | Relative => {
                        if input.source_character == RangeControl || input.make_absolute {
                            MakesSense(
                                "If enabled, reverses the direction of the target value change.",
                            )
                        } else {
                            MakesSense(
                                "If enabled, converts increments to decrements and vice versa.",
                            )
                        }
                    }
                }
            }
        }
        OutOfRangeBehavior => {
            if input.is_feedback {
                if input.use_textual_feedback {
                    HasNoEffect
                } else {
                    MakesSense("-")
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    Trigger => HasNoEffect,
                    MomentaryOnOffButton | MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == crate::AbsoluteMode::Normal {
                            MakesSense("-")
                        } else {
                            HasNoEffect
                        }
                    }
                    RangeControl | Relative => {
                        if input.source_character == Relative && !input.make_absolute {
                            HasNoEffect
                        } else {
                            MakesSense("-")
                        }
                    }
                }
            }
        }
        SpecificOutOfRangeBehavior(b) => {
            use crate::OutOfRangeBehavior::*;
            if input.is_feedback {
                match b {
                    MinOrMax => {
                        MakesSense("Uses target min/max if target value below/above range.")
                    }
                    Min => MakesSense("Uses target min if target value out of range."),
                    Ignore => MakesSense("Doesn't send feedback if target value out of range."),
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    // Doesn't have an effect if source max is at 100% (which is a basic requirement
                    // and mentioned in the source min/max description).
                    Trigger => HasNoEffect,
                    MomentaryOnOffButton => {
                        if input.absolute_mode == crate::AbsoluteMode::Normal {
                            match b {
                                // Doesn't really have an effect so I guess this is
                                // backward-compatible.
                                MinOrMax | Min => HasNoEffect,
                                Ignore => {
                                    Awkward("Ignores button press if \"on\" value out of range.")
                                }
                            }
                        } else {
                            HasNoEffect
                        }
                    }
                    MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == crate::AbsoluteMode::Normal {
                            match b {
                                MinOrMax => MakesSense(
                                    "Uses min/max velocity if button velocity below/above velocity range.",
                                ),
                                Min => {
                                    MakesSense("Uses min velocity if button velocity out of range.")
                                }
                                Ignore => {
                                    MakesSense("Ignores button press if velocity out of range.")
                                }
                            }
                        } else {
                            HasNoEffect
                        }
                    }
                    RangeControl | Relative => {
                        if input.source_character == Relative && !input.make_absolute {
                            HasNoEffect
                        } else {
                            match b {
                                MinOrMax => MakesSense(
                                    "Uses source min/max if source value below/above source range.",
                                ),
                                Min => MakesSense("Uses source min if source value out of range."),
                                Ignore => MakesSense("Ignores event if source value out of range."),
                            }
                        }
                    }
                }
            }
        }
        JumpMinMax | TakeoverMode => {
            if input.target_is_virtual
                || input.is_feedback
                || input.absolute_mode == crate::AbsoluteMode::MakeRelative
            {
                HasNoEffect
            } else if input.control_transformation_uses_time {
                MakesNoSenseUseDefault
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | Trigger => MakesNoSenseUseDefault,
                    MomentaryVelocitySensitiveButton | RangeControl | Relative => {
                        if (input.source_character == MomentaryVelocitySensitiveButton
                            && input.absolute_mode != crate::AbsoluteMode::Normal)
                            || (input.source_character == Relative && !input.make_absolute)
                        {
                            HasNoEffect
                        } else if mode_parameter == JumpMinMax {
                            MakesSense(
                                "Sets the min/max allowed target parameter jump (set max very low for takeover).",
                            )
                        } else {
                            // Takeover mode
                            MakesSense("Defines how to deal with too long target parameter jumps.")
                        }
                    }
                }
            }
        }
        ControlTransformation => {
            if input.is_feedback || input.absolute_mode == crate::AbsoluteMode::MakeRelative {
                HasNoEffect
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | Trigger => {
                        if input.absolute_mode == crate::AbsoluteMode::Normal {
                            MakesSense(
                                "Defines via EEL how to transform incoming button presses or releases. Interesting use case for buttons: Stepping through a list of predefined target values. You can access the current target value as normalized value y (where 0.0 <= y <= 1.0). Example: a = 0.0; b = 0.2; c = 0.6; y = y == a ? b : (y == b ? c : a);",
                            )
                        } else {
                            MakesNoSenseUseDefault
                        }
                    }
                    MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == crate::AbsoluteMode::Normal {
                            MakesSense(
                                "Defines via EEL how to transform the button velocity (represented as normalized source value x where 0.0 <= x <= 1.0). See other button types for additional use cases. Example that creates a curve: y = x^8",
                            )
                        } else {
                            MakesNoSenseUseDefault
                        }
                    }
                    RangeControl | Relative => {
                        if input.source_character == Relative && !input.make_absolute {
                            HasNoEffect
                        } else {
                            MakesSense(
                                "Defines via EEL how to transform the knob/fader position (represented as normalized source value x where 0.0 <= x <= 1.0). Example that creates a curve: y = x^8",
                            )
                        }
                    }
                }
            }
        }
        TargetMinMax => {
            if input.target_is_virtual {
                HasNoEffect
            } else if input.is_feedback {
                if input.use_textual_feedback {
                    HasNoEffect
                } else {
                    MakesSense("Defines the relevant target value range.")
                }
            } else if input.target_value_sequence_is_set
                && check_mode_applicability(TargetValueSequence, input).is_relevant()
            {
                HasNoEffect
            } else {
                MakesSense("Makes sure the target value will end up in the specified range.")
            }
        }
        TargetValueSequence => {
            if input.target_is_virtual || input.is_feedback {
                HasNoEffect
            } else {
                use crate::AbsoluteMode::*;
                match input.absolute_mode {
                    Normal | IncrementalButton | MakeRelative => {
                        MakesSense("Allows you to step through a sequence of comma-separated user-defined target values and value ranges. When using relative control, duplicate values and direction changes are ignored. Example: 25 - 50 (2), 75, 50, 100 %")
                    }
                    ToggleButton | PerformanceControl => {
                        MakesNoSenseUseDefault
                    }
                }
            }
        }
        FeedbackTransformation => {
            if input.is_feedback && !input.use_textual_feedback {
                MakesSense(
                    "Defines via EEL how to transform the normalized feedback value y (where 0.0 <= y <= 1.0). Example: x = 1 - y",
                )
            } else {
                HasNoEffect
            }
        }
        TextualFeedbackExpression => {
            if input.is_feedback && input.use_textual_feedback && !input.target_is_virtual {
                MakesSense("Text that you write here will appear on your hardware display. You can access lots of mapping and target properties using double braces. Example: \"{{ target.normalized_value }} %\".")
            } else {
                HasNoEffect
            }
        }
        StepSizeMin | StepFactorMin => {
            if input.is_feedback {
                HasNoEffect
            } else if input.control_transformation_produces_relative_values {
                MakesSense("-")
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | Trigger | MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == crate::AbsoluteMode::IncrementalButton {
                            if input.source_character == MomentaryVelocitySensitiveButton {
                                if mode_parameter == StepSizeMin {
                                    MakesSense(
                                        "Sets the target value change amount when button pressed with lowest velocity.",
                                    )
                                } else {
                                    MakesSense(
                                        "Sets the number of target increments when button pressed with lowest velocity.",
                                    )
                                }
                            } else if mode_parameter == StepSizeMin {
                                MakesSense(
                                    "Sets the target value change amount when button pressed.",
                                )
                            } else {
                                MakesSense(
                                    "Sets the number of target increments when button pressed.",
                                )
                            }
                        } else {
                            HasNoEffect
                        }
                    }
                    RangeControl => HasNoEffect,
                    Relative => {
                        if input.make_absolute {
                            if mode_parameter == StepSizeMin {
                                MakesSense(
                                    "Sets the amount added/subtracted to calculate the absolute value from an incoming non-accelerated increment/decrement.",
                                )
                            } else {
                                HasNoEffect
                            }
                        } else if mode_parameter == StepSizeMin {
                            MakesSense(STEP_SIZE_MIN_FOR_RANGE_DESC)
                        } else {
                            MakesSense(SPEED_MIN_FOR_RANGE_DESC)
                        }
                    }
                }
            }
        }
        StepSizeMax | StepFactorMax => {
            if input.is_feedback {
                HasNoEffect
            } else if input.control_transformation_produces_relative_values {
                MakesSense("-")
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | Trigger => MakesNoSenseUseDefault,
                    MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == crate::AbsoluteMode::IncrementalButton {
                            if mode_parameter == StepSizeMax {
                                MakesSense(
                                    "Sets the target value change amount when button pressed with highest velocity.",
                                )
                            } else {
                                MakesSense(
                                    "Sets the number of target increments when button pressed with highest velocity.",
                                )
                            }
                        } else {
                            HasNoEffect
                        }
                    }
                    RangeControl => HasNoEffect,
                    Relative => {
                        if input.make_absolute {
                            if mode_parameter == StepSizeMax {
                                MakesSense(
                                    "Sets the amount added/subtracted to calculate the absolute value from an incoming most accelerated increment/decrement.",
                                )
                            } else {
                                HasNoEffect
                            }
                        } else if mode_parameter == StepSizeMin {
                            MakesSense(STEP_SIZE_MAX_FOR_RANGE_DESC)
                        } else {
                            MakesSense(SPEED_MAX_FOR_RANGE_DESC)
                        }
                    }
                }
            }
        }
        RelativeFilter => {
            if !input.is_feedback
                && (input.source_character == DetailedSourceCharacter::Relative
                    || input.absolute_mode == crate::AbsoluteMode::MakeRelative)
            {
                MakesSense("Defines whether to process increments only, decrements only or both.")
            } else {
                HasNoEffect
            }
        }
        Rotate => {
            if input.is_feedback {
                HasNoEffect
            } else if input.control_transformation_produces_relative_values {
                MakesSense("-")
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | MomentaryVelocitySensitiveButton | Trigger => {
                        if input.absolute_mode == crate::AbsoluteMode::IncrementalButton {
                            MakesSense(
                                "If enabled, jumps from max target value to min target value (or opposite if reverse enabled). Was called \"Rotate\" before.",
                            )
                        } else {
                            HasNoEffect
                        }
                    }
                    RangeControl => {
                        if input.absolute_mode == crate::AbsoluteMode::MakeRelative {
                            MakesSense(ROTATE_FOR_RANGE_DESC)
                        } else {
                            HasNoEffect
                        }
                    }
                    Relative => {
                        if input.make_absolute {
                            MakesSense(
                                "If enabled, jumps from absolute value 100% to 0% for increments (opposite for decrements). Was called \"Rotate\" before.",
                            )
                        } else {
                            MakesSense(ROTATE_FOR_RANGE_DESC)
                        }
                    }
                }
            }
        }
        FireMode => {
            if input.is_feedback {
                HasNoEffect
            } else if input.source_is_button()
                && !input.target_is_virtual
                && input.absolute_mode != crate::AbsoluteMode::MakeRelative
            {
                // Description not interesting, will be queried for specific fire mode only.
                MakesSense("-")
            } else {
                MakesNoSenseUseDefault
            }
        }
        SpecificFireMode(m) => {
            use crate::FireMode::*;
            match m {
                Normal => {
                    if input.source_character == DetailedSourceCharacter::Trigger {
                        MakesNoSenseParentTakesCareOfDefault
                    } else {
                        MakesSense(
                                "If min and max is 0 ms, fires immediately on button press. If one of them is > 0 ms, fires on release if the button press duration was in range.",
                            )
                    }
                }
                AfterTimeout => {
                    if input.source_character == DetailedSourceCharacter::Trigger {
                        MakesSense("Fires after the specified timeout instead of immediately.")
                    } else {
                        MakesSense(
                            "Fires as soon as button pressed as long as the specified timeout.",
                        )
                    }
                }
                AfterTimeoutKeepFiring => {
                    if input.source_character == DetailedSourceCharacter::Trigger {
                        // What sense does it make if we can't turn the turbo off again ...
                        MakesNoSenseParentTakesCareOfDefault
                    } else {
                        MakesSense(
                                "When button pressed, waits until specified timeout and then fires continuously with the specified rate until button released.",
                            )
                    }
                }
                OnSinglePress => MakesSense("Reacts to single button presses only."),
                OnDoublePress => {
                    MakesSense("Reacts to double button presses only (like a mouse double-click).")
                }
            }
        }
        ButtonFilter => {
            if input.is_feedback {
                HasNoEffect
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | MomentaryVelocitySensitiveButton
                        if input.absolute_mode == crate::AbsoluteMode::Normal =>
                    {
                        match input.fire_mode {
                            crate::FireMode::Normal | crate::FireMode::AfterTimeout | crate::FireMode::AfterTimeoutKeepFiring => {
                                MakesSense(
                                    "Defines whether to process button presses only, releases only or both.",
                                )
                            }
                            crate::FireMode::OnSinglePress |
                            crate::FireMode::OnDoublePress => {
                                // In this case, we need both press and release as input for implementing the fire mode.
                                // And the output is only press.
                                MakesNoSenseUseDefault
                            }
                        }
                    }
                    RangeControl | Trigger => MakesNoSenseUseDefault,
                    _ => HasNoEffect,
                }
            }
        }
        MakeAbsolute => {
            if input.is_feedback {
                HasNoEffect
            } else if input.source_character == DetailedSourceCharacter::Relative
                || input.absolute_mode == crate::AbsoluteMode::IncrementalButton
            {
                MakesSense(
                        "Converts relative increments/decrements into an absolute value. This allows you to use control transformation and discontinuous target value sequences but comes with the disadvantage of parameter jumps (which can be mitigated using the jump settings).",
                    )
            } else {
                HasNoEffect
            }
        }
        FeedbackType => {
            if input.is_feedback {
                MakesSense(
                    "Allows you to switch to textual feedback (to be used with textual sources such as LCDs).",
                )
            } else {
                HasNoEffect
            }
        }
        RoundTargetValue => {
            if input.target_is_virtual || input.is_feedback {
                HasNoEffect
            } else {
                use DetailedSourceCharacter::*;
                let makes_sense = match input.source_character {
                    MomentaryOnOffButton | MomentaryVelocitySensitiveButton | Trigger => {
                        input.absolute_mode == crate::AbsoluteMode::Normal
                    }
                    RangeControl => input.absolute_mode != crate::AbsoluteMode::MakeRelative,
                    Relative => input.make_absolute,
                };
                if makes_sense {
                    MakesSense(
                        "If enabled and target supports it, makes sure the target value is always rounded to discrete values without decimals (e.g. tempo in BPM).",
                    )
                } else {
                    HasNoEffect
                }
            }
        }
        AbsoluteMode => {
            if input.is_feedback {
                HasNoEffect
            } else if input.source_is_button()
                || input.source_character == DetailedSourceCharacter::RangeControl
            {
                // Description not interesting, will be queried for specific absolute mode only.
                MakesSense("-")
            } else {
                MakesNoSenseUseDefault
            }
        }
        SpecificAbsoluteMode(m) => {
            if input.is_feedback {
                HasNoEffect
            } else if input.control_transformation_uses_time && m == PerformanceControl {
                MakesNoSenseUseDefault
            } else {
                use crate::AbsoluteMode::*;
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | Trigger | MomentaryVelocitySensitiveButton => {
                        match m {
                            Normal => {
                                if input.source_character == MomentaryVelocitySensitiveButton {
                                    MakesSense(
                                        "When pressing the button, sets the target value to a velocity-dependent value. Sets it back to minimum when releasing it.",
                                    )
                                } else {
                                    MakesSense(
                                        "Sets target value to its maximum when pressing the button and back to its minimum when releasing it.",
                                    )
                                }
                            }
                            IncrementalButton => {
                                if input.source_character == MomentaryVelocitySensitiveButton {
                                    MakesSense(
                                        "Increases the target value with each button press with the defined step size range, taking the velocity of the button press into account.",
                                    )
                                } else {
                                    MakesSense(
                                        "Increases the target value with each button press with the defined min step size.",
                                    )
                                }
                            }
                            ToggleButton => MakesSense(
                                "Switches the target value between its minimum and maximum on each button press.",
                            ),
                            MakeRelative | PerformanceControl => MakesNoSenseUseDefault,
                        }
                    }
                    RangeControl => {
                        match m {
                            Normal => {
                                MakesSense(
                                    NORMAL_ABSOLUTE_MODE_FOR_RANGE_DESC,
                                )

                            }
                            MakeRelative => {
                                MakesSense(
                                    "Attempts to convert incoming absolute control values to relative increments, making it possible to control targets relatively with absolute controls."
                                )
                            }
                            PerformanceControl => {
                                MakesSense(
                                    "Changes the target value starting from its last position set within REAPER."
                                )
                            }
                            IncrementalButton | ToggleButton => MakesNoSenseParentTakesCareOfDefault
                        }
                    }
                    Relative => {
                        if input.make_absolute {
                            match m {
                                Normal => {
                                    MakesSense(
                                        NORMAL_ABSOLUTE_MODE_FOR_RANGE_DESC,
                                    )
                                }
                                MakeRelative | IncrementalButton | ToggleButton | PerformanceControl => MakesNoSenseParentTakesCareOfDefault
                            }
                        } else {
                            HasNoEffect
                        }
                    }
                }
            }
        }
        GroupInteraction => {
            if input.is_feedback || input.target_is_virtual {
                HasNoEffect
            } else {
                // Description not interesting, will be queried for specific interaction only.
                MakesSense("-")
            }
        }
        SpecificGroupInteraction(i) => {
            if input.is_feedback || input.target_is_virtual {
                HasNoEffect
            } else {
                use crate::GroupInteraction::*;
                match i {
                    None => MakesSense("Other mappings in the same group will not be touched."),
                    SameControl => {
                        MakesSense("Other non-virtual mappings in this group will receive the same control value. Unlike \"Same target value\", this will run the complete glue section of the other mapping.")
                    }
                    SameTargetValue => {
                        MakesSense(
                            "Other non-virtual mappings in this group will receive the same target value as this one with respect to their corresponding target range. This can lead to jumps. If you don't like this, use \"Same control\".",
                        )
                    }
                    InverseControl => {
                        MakesSense("Other non-virtual mappings in this group will receive the opposite control value. Unlike \"Inverse target value\", this will run the complete glue section of the other mapping.")
                    }
                    InverseTargetValue => {
                        use DetailedSourceCharacter::*;
                        match input.source_character {
                            MomentaryOnOffButton | Trigger => {
                                MakesSense("Other non-virtual mappings in this group will receive the opposite target value, e.g. their targets will be switched off when this target is switched on. Great for making something exclusive within a group!")
                            }
                            RangeControl | Relative | MomentaryVelocitySensitiveButton => {
                                MakesSense(
                                    "Other non-virtual mappings in this group will receive the inverse target value with respect to their corresponding target range. This can lead to jumps. If you don't like this, use \"Inverse control\".",
                                )
                            }
                        }
                    }
                    InverseTargetValueOnOnly => {
                        MakesSense(
                            "Like \"Inverse target value\" but doesn't apply the inverse to other mappings if the target value is zero. Useful for exclusive toggle buttons.",
                        )
                    }
                    InverseTargetValueOffOnly => {
                        MakesSense(
                            "Like \"Inverse target value\" but doesn't apply the inverse to other mappings if the target value is not zero. Useful for exclusive toggle buttons.",
                        )
                    }
                }
            }
        }
    }
}
