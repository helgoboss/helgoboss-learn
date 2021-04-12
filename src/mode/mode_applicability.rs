use crate::{AbsoluteMode, OutOfRangeBehavior};
use derive_more::Display;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DetailedSourceCharacter {
    MomentaryOnOffButton,
    MomentaryVelocitySensitiveButton,
    ToggleOnlyOnOffButton,
    RangeControl,
    Encoder,
}

impl DetailedSourceCharacter {
    fn is_button(self) -> bool {
        use DetailedSourceCharacter::*;
        matches!(
            self,
            MomentaryOnOffButton | MomentaryVelocitySensitiveButton | ToggleOnlyOnOffButton
        )
    }
}

#[derive(Debug)]
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
    #[display(fmt = "Out-of-range behavior")]
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
                    Some("off/on LED colors")
                } else {
                    Some("lowest/highest position of motorized fader or LED ring")
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | ToggleOnlyOnOffButton => None,
                    MomentaryVelocitySensitiveButton => Some(
                        "consider button presses with velocity between min and max as full range",
                    ),
                    RangeControl | Encoder => {
                        if input.source_character == RangeControl || input.make_absolute {
                            Some("consider fader/knob positions between min and max as full range")
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
                        "use \"off\" LED color if target is on and \"on\" LED color if target is off",
                    )
                } else {
                    Some("reverse direction of motorized fader or LED ring")
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton
                    | MomentaryVelocitySensitiveButton
                    | ToggleOnlyOnOffButton => match input.absolute_mode {
                        AbsoluteMode::Normal => {
                            Some("switch target off when pressed and on when released")
                        }
                        AbsoluteMode::IncrementalButtons => {
                            Some("decrease target value on press instead of increasing it")
                        }
                        AbsoluteMode::ToggleButtons => None,
                    },
                    RangeControl | Encoder => {
                        if input.source_character == RangeControl || input.make_absolute {
                            Some("reverse direction of target value change")
                        } else {
                            Some("convert increments to decrements and vice versa")
                        }
                    }
                }
            }
        }
        OutOfRangeBehavior(b) => {
            use crate::OutOfRangeBehavior::*;
            if input.is_feedback {
                match b {
                    MinOrMax => Some("use target min/max if target value below/above range"),
                    Min => Some("use target min if target value out of range"),
                    Ignore => Some("don't send feedback if target value out of range"),
                }
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton | ToggleOnlyOnOffButton => None,
                    MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == AbsoluteMode::Normal {
                            match b {
                                MinOrMax => Some(
                                    "use min/max velocity if button velocity below/above velocity range",
                                ),
                                Min => Some("use min velocity if button velocity out of range"),
                                Ignore => Some("ignore if button velocity out of range"),
                            }
                        } else {
                            None
                        }
                    }
                    RangeControl | Encoder => {
                        if input.source_character == Encoder && !input.make_absolute {
                            None
                        } else {
                            match b {
                                MinOrMax => Some(
                                    "use source min/max if source value below/above source range",
                                ),
                                Min => Some("use source min if source value out of range"),
                                Ignore => Some("ignore if source value out of range"),
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
                    MomentaryOnOffButton | ToggleOnlyOnOffButton => None,
                    MomentaryVelocitySensitiveButton | RangeControl | Encoder => {
                        if (input.source_character == MomentaryVelocitySensitiveButton
                            && input.absolute_mode != AbsoluteMode::Normal)
                            || (input.source_character == Encoder && !input.make_absolute)
                        {
                            None
                        } else if input.mode_parameter == JumpMinMax {
                            Some(
                                "min/max allowed target parameter jump (set max very low for takeover)",
                            )
                        } else {
                            // Takeover mode
                            Some("how to deal with too long target parameter jumps")
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
                    MomentaryOnOffButton | ToggleOnlyOnOffButton => None,
                    MomentaryVelocitySensitiveButton | RangeControl | Encoder => {
                        if (input.source_character == MomentaryVelocitySensitiveButton
                            && input.absolute_mode != AbsoluteMode::Normal)
                            || (input.source_character == Encoder && !input.make_absolute)
                        {
                            None
                        } else {
                            Some(
                                "EEL formula which transforms the normalized source value x (0.0 <= x <= 1.0)",
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
                Some("consider target values between min and max as full range")
            } else {
                Some("target value will end up somewhere between min and max")
            }
        }
        FeedbackTransformation => {
            if input.is_feedback {
                Some(
                    "EEL formula which transforms the normalized feedback value y (0.0 <= y <= 1.0)",
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
                    MomentaryOnOffButton
                    | ToggleOnlyOnOffButton
                    | MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == AbsoluteMode::IncrementalButtons {
                            if input.source_character == MomentaryVelocitySensitiveButton {
                                if input.mode_parameter == StepSizeMin {
                                    Some(
                                        "amount of target value change when button pressed with lowest velocity",
                                    )
                                } else {
                                    Some(
                                        "number of target increments when button pressed with lowest velocity",
                                    )
                                }
                            } else {
                                if input.mode_parameter == StepSizeMin {
                                    Some("amount of target value change when button pressed")
                                } else {
                                    Some("number of target increments when button pressed")
                                }
                            }
                        } else {
                            None
                        }
                    }
                    RangeControl => None,
                    Encoder => {
                        if input.make_absolute {
                            if input.mode_parameter == StepSizeMin {
                                Some(
                                    "amount added/subtracted to calculate absolute value from incoming non-accelerated increment/decrement",
                                )
                            } else {
                                None
                            }
                        } else {
                            if input.mode_parameter == StepSizeMin {
                                Some(
                                    "amount of target value change on incoming non-accelerated increment/decrement",
                                )
                            } else {
                                Some(
                                    "number of target increments on incoming non-accelerated increment/decrement",
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
                    MomentaryOnOffButton | ToggleOnlyOnOffButton | RangeControl => None,
                    MomentaryVelocitySensitiveButton => {
                        if input.absolute_mode == AbsoluteMode::IncrementalButtons {
                            if input.mode_parameter == StepSizeMax {
                                Some(
                                    "amount of target value change when button pressed with highest velocity",
                                )
                            } else {
                                Some(
                                    "number of target increments when button pressed with highest velocity",
                                )
                            }
                        } else {
                            None
                        }
                    }
                    Encoder => {
                        if input.make_absolute {
                            if input.mode_parameter == StepSizeMax {
                                Some(
                                    "amount added/subtracted to calculate absolute value from incoming most accelerated increment/decrement",
                                )
                            } else {
                                None
                            }
                        } else {
                            if input.mode_parameter == StepSizeMin {
                                Some(
                                    "amount of target value change on incoming most accelerated increment/decrement",
                                )
                            } else {
                                Some(
                                    "number of target increments on incoming most accelerated increment/decrement",
                                )
                            }
                        }
                    }
                }
            }
        }
        RelativeFilter => {
            if input.is_feedback || input.source_character != DetailedSourceCharacter::Encoder {
                None
            } else {
                Some("process increments only, decrements only or both")
            }
        }
        Rotate => {
            if input.is_feedback {
                None
            } else {
                use DetailedSourceCharacter::*;
                match input.source_character {
                    MomentaryOnOffButton
                    | MomentaryVelocitySensitiveButton
                    | ToggleOnlyOnOffButton => {
                        if input.absolute_mode == AbsoluteMode::IncrementalButtons {
                            Some(
                                "jump from max target value to min target value (or opposite if reverse enabled)",
                            )
                        } else {
                            None
                        }
                    }
                    RangeControl => None,
                    Encoder => {
                        if input.make_absolute {
                            Some(
                                "jump from absolute value 100% to 0% for increments (opposite for decrements)",
                            )
                        } else {
                            Some(
                                "jump from max target value to min target value for increments (opposite for decrements)",
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
                Some("react to certain button interactions only")
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
                        Some("process button presses only, releases only or both")
                    }
                    _ => None,
                }
            }
        }
    }
}
