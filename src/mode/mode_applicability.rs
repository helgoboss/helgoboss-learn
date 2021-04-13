use crate::{AbsoluteMode, OutOfRangeBehavior};
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
    #[display(fmt = "Press-only button (doesn't fire on release)")]
    PressOnlyButton,
    #[display(fmt = "Range control element (e.g. knob or fader)")]
    RangeControl,
    #[display(fmt = "Relative control element (e.g. encoder)")]
    Relative,
}

impl DetailedSourceCharacter {
    fn is_button(self) -> bool {
        use DetailedSourceCharacter::*;
        matches!(
            self,
            MomentaryOnOffButton | MomentaryVelocitySensitiveButton | PressOnlyButton
        )
    }
}

#[derive(Clone, Debug)]
pub struct ModeApplicabilityCheckInput {
    pub target_is_virtual: bool,
    pub is_feedback: bool,
    pub make_absolute: bool,
    pub source_character: DetailedSourceCharacter,
    pub absolute_mode: AbsoluteMode,
    pub mode_parameter: ModeParameter,
}

impl ModeApplicabilityCheckInput {
    pub fn source_is_button(&self) -> bool {
        self.source_character.is_button()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Display)]
pub enum ModeParameter {
    #[display(fmt = "Source min/max")]
    SourceMinMax,
    #[display(fmt = "Reverse")]
    Reverse,
    #[display(fmt = "Out-of-range behavior \"{}\"", _0)]
    OutOfRangeBehavior(OutOfRangeBehavior),
    #[display(fmt = "Jump min/max")]
    JumpMinMax,
    #[display(fmt = "Takeover mode")]
    TakeoverMode,
    #[display(fmt = "Control transformation")]
    ControlTransformation,
    #[display(fmt = "Target min/max")]
    TargetMinMax,
    #[display(fmt = "Feedback transformation")]
    FeedbackTransformation,
    #[display(fmt = "Step size min")]
    StepSizeMin,
    #[display(fmt = "Step size max")]
    StepSizeMax,
    #[display(fmt = "Speed min")]
    SpeedMin,
    #[display(fmt = "Speed max")]
    SpeedMax,
    #[display(fmt = "Relative filter")]
    RelativeFilter,
    #[display(fmt = "Rotate")]
    Rotate,
    #[display(fmt = "Fire mode")]
    FireMode,
    #[display(fmt = "Button filter")]
    ButtonFilter,
}

pub fn check_mode_applicability(input: ModeApplicabilityCheckInput) -> Option<&'static str> {
    use ModeParameter::*;
    match input.mode_parameter {
        SourceMinMax => {
            if input.is_feedback {
                if input.source_is_button() {
                    Some("Changes off/on LED colors")
                } else {
                    Some("Changes lowest/highest position of motorized fader or LED ring")
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | PressOnlyButton => None,
                    MomentaryVelocitySensitiveButton => {
                        Some("Defines the observed button press velocity range")
                    }
                    RangeControl | Relative => {
                        if input.source_character == RangeControl || input.make_absolute {
                            Some("Defines the observed fader/knob position range")
                        } else {
                            None
                        }
                    }
                }
            }
        }
        Reverse => {
            if input.is_feedback {
                if input.source_is_button() {
                    Some(
                        "If enabled, uses \"off\" LED color if target is on and \"on\" LED color if target is off",
                    )
                } else {
                    Some("If enabled, reverses the direction of motorized fader or LED ring")
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | MomentaryVelocitySensitiveButton | PressOnlyButton => {
                        match input.absolute_mode {
                            AbsoluteMode::Normal => Some(
                                "If enabled, switches the target off when pressed and on when released",
                            ),
                            AbsoluteMode::IncrementalButtons => Some(
                                "If enabled, decreases the target value on press instead of increasing it",
                            ),
                            AbsoluteMode::ToggleButtons => None,
                        }
                    }
                    RangeControl | Relative => {
                        if input.source_character == RangeControl || input.make_absolute {
                            Some("If enabled, reverses the direction of the target value change")
                        } else {
                            Some("If enabled, converts increments to decrements and vice versa")
                        }
                    }
                }
            }
        }
        OutOfRangeBehavior(b) => {
            use crate::OutOfRangeBehavior::*;
            if input.is_feedback {
                match b {
                    MinOrMax => Some("Uses target min/max if target value below/above range"),
                    Min => Some("Uses target min if target value out of range"),
                    Ignore => Some("Doesn't send feedback if target value out of range"),
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | PressOnlyButton => None,
                    MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == AbsoluteMode::Normal {
                            match b {
                                MinOrMax => Some(
                                    "Uses min/max velocity if button velocity below/above velocity range",
                                ),
                                Min => Some("Uses min velocity if button velocity out of range"),
                                Ignore => Some("Ignores button press if velocity out of range"),
                            }
                        } else {
                            None
                        }
                    }
                    RangeControl | Relative => {
                        if input.source_character == Relative && !input.make_absolute {
                            None
                        } else {
                            match b {
                                MinOrMax => Some(
                                    "Uses source min/max if source value below/above source range",
                                ),
                                Min => Some("Uses source min if source value out of range"),
                                Ignore => Some("Ignores event if source value out of range"),
                            }
                        }
                    }
                }
            }
        }
        JumpMinMax | TakeoverMode => {
            if input.target_is_virtual || input.is_feedback {
                None
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | PressOnlyButton => None,
                    MomentaryVelocitySensitiveButton | RangeControl | Relative => {
                        if (input.source_character == MomentaryVelocitySensitiveButton
                            && input.absolute_mode != AbsoluteMode::Normal)
                            || (input.source_character == Relative && !input.make_absolute)
                        {
                            None
                        } else if input.mode_parameter == JumpMinMax {
                            Some(
                                "Sets the min/max allowed target parameter jump (set max very low for takeover)",
                            )
                        } else {
                            // Takeover mode
                            Some("Defines how to deal with too long target parameter jumps")
                        }
                    }
                }
            }
        }
        ControlTransformation => {
            if input.is_feedback {
                None
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | PressOnlyButton => None,
                    MomentaryVelocitySensitiveButton | RangeControl | Relative => {
                        if (input.source_character == MomentaryVelocitySensitiveButton
                            && input.absolute_mode != AbsoluteMode::Normal)
                            || (input.source_character == Relative && !input.make_absolute)
                        {
                            None
                        } else {
                            Some(
                                "Defines via EEL how to transform the normalized source value x (0.0 <= x <= 1.0)",
                            )
                        }
                    }
                }
            }
        }
        TargetMinMax => {
            if input.target_is_virtual {
                None
            } else if input.is_feedback {
                Some("Defines the relevant target value range")
            } else {
                Some("Makes sure the target value will end up in the specified range")
            }
        }
        FeedbackTransformation => {
            if input.is_feedback {
                Some(
                    "Defines via EEL how to transform the normalized feedback value y (0.0 <= y <= 1.0)",
                )
            } else {
                None
            }
        }
        StepSizeMin | SpeedMin => {
            if input.is_feedback {
                None
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | PressOnlyButton | MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == AbsoluteMode::IncrementalButtons {
                            if input.source_character == MomentaryVelocitySensitiveButton {
                                if input.mode_parameter == StepSizeMin {
                                    Some(
                                        "Sets the target value change amount when button pressed with lowest velocity",
                                    )
                                } else {
                                    Some(
                                        "Sets the number of target increments when button pressed with lowest velocity",
                                    )
                                }
                            } else {
                                if input.mode_parameter == StepSizeMin {
                                    Some("Sets the target value change amount when button pressed")
                                } else {
                                    Some("Sets the number of target increments when button pressed")
                                }
                            }
                        } else {
                            None
                        }
                    }
                    RangeControl => None,
                    Relative => {
                        if input.make_absolute {
                            if input.mode_parameter == StepSizeMin {
                                Some(
                                    "Sets the amount added/subtracted to calculate the absolute value from an incoming non-accelerated increment/decrement",
                                )
                            } else {
                                None
                            }
                        } else {
                            if input.mode_parameter == StepSizeMin {
                                Some(
                                    "Sets the target value change amount for an incoming non-accelerated increment/decrement",
                                )
                            } else {
                                Some(
                                    "Sets the number of target increments for an incoming non-accelerated increment/decrement",
                                )
                            }
                        }
                    }
                }
            }
        }
        StepSizeMax | SpeedMax => {
            if input.is_feedback {
                None
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | PressOnlyButton | RangeControl => None,
                    MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == AbsoluteMode::IncrementalButtons {
                            if input.mode_parameter == StepSizeMax {
                                Some(
                                    "Sets the target value change amount when button pressed with highest velocity",
                                )
                            } else {
                                Some(
                                    "Sets the number of target increments when button pressed with highest velocity",
                                )
                            }
                        } else {
                            None
                        }
                    }
                    Relative => {
                        if input.make_absolute {
                            if input.mode_parameter == StepSizeMax {
                                Some(
                                    "Sets the amount added/subtracted to calculate the absolute value from an incoming most accelerated increment/decrement",
                                )
                            } else {
                                None
                            }
                        } else {
                            if input.mode_parameter == StepSizeMin {
                                Some(
                                    "Sets the target value change amount for an incoming most accelerated increment/decrement",
                                )
                            } else {
                                Some(
                                    "Sets the number of target increments for an incoming most accelerated increment/decrement",
                                )
                            }
                        }
                    }
                }
            }
        }
        RelativeFilter => {
            if input.is_feedback || input.source_character != DetailedSourceCharacter::Relative {
                None
            } else {
                Some("Defines whether to process increments only, decrements only or both")
            }
        }
        Rotate => {
            if input.is_feedback {
                None
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | MomentaryVelocitySensitiveButton | PressOnlyButton => {
                        if input.absolute_mode == AbsoluteMode::IncrementalButtons {
                            Some(
                                "If enabled, jumps from max target value to min target value (or opposite if reverse enabled)",
                            )
                        } else {
                            None
                        }
                    }
                    RangeControl => None,
                    Relative => {
                        if input.make_absolute {
                            Some(
                                "If enabled, jumps from absolute value 100% to 0% for increments (opposite for decrements)",
                            )
                        } else {
                            Some(
                                "If enabled, jumps from max target value to min target value for increments (opposite for decrements)",
                            )
                        }
                    }
                }
            }
        }
        FireMode => {
            if input.is_feedback || !input.source_is_button() {
                None
            } else {
                Some("Fire in different situations")
            }
        }
        ButtonFilter => {
            if input.is_feedback {
                None
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | MomentaryVelocitySensitiveButton
                        if input.absolute_mode == AbsoluteMode::Normal =>
                    {
                        Some(
                            "Defines whether to process button presses only, releases only or both",
                        )
                    }
                    _ => None,
                }
            }
        }
    }
}
