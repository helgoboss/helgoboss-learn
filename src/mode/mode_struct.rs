use crate::{
    create_discrete_increment_interval, create_unit_value_interval, full_unit_interval,
    negative_if, AbsoluteValue, AbstractTimestamp, ButtonUsage, ControlEvent, ControlType,
    ControlValue, DiscreteIncrement, DiscreteValue, EncoderUsage, FeedbackStyle, FeedbackValue,
    FireMode, Fraction, Increment, Interval, MinIsMaxBehavior, NumericFeedbackValue,
    OutOfRangeBehavior, PressDurationProcessor, TakeoverMode, Target, TextualFeedbackValue,
    Transformation, TransformationInputMetaData, TransformationOutput, UnitIncrement, UnitValue,
    ValueSequence, BASE_EPSILON,
};
use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use regex::Captures;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "serde_repr")]
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::time::Duration;

/// When interpreting target value, make only 4 fractional digits matter.
///
/// If we don't do this and target min == target max, even the slightest imprecision of the actual
/// target value (which in practice often occurs with FX parameters not taking exactly the desired
/// value) could result in a totally different feedback value. Maybe it would be better to determine
/// the epsilon dependent on the source precision (e.g. 1.0/128.0 in case of short MIDI messages)
/// but right now this should suffice to solve the immediate problem.  
pub const FEEDBACK_EPSILON: f64 = BASE_EPSILON;

/// 0.01 has been chosen as default minimum step size because it corresponds to 1%.
pub const DEFAULT_STEP_SIZE: f64 = 0.01;

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct ModeControlOptions {
    pub enforce_rotate: bool,
}

pub trait TransformationInputProvider<T> {
    fn additional_input(&self) -> T;
}

// It's quite practical and makes sense to let the unit control context (basically a control context
// that is empty) to always create the default transformation input. It also saves some plumbing
// because we couldn't implement TransformationInputProvider for () in other crates. We need to do
// it here. Otherwise we would have
impl<T: Default> TransformationInputProvider<T> for () {
    fn additional_input(&self) -> T {
        Default::default()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct ModeFeedbackOptions {
    pub source_is_virtual: bool,
    pub max_discrete_source_value: Option<u32>,
}

#[derive(Clone, Debug)]
pub enum FeedbackValueTable {
    FromTextToDiscrete(HashMap<String, u32>),
    FromTextToContinuous(HashMap<String, f64>),
}

impl FeedbackValueTable {
    pub fn transform_value<'a, 'b, 'c>(
        &'b self,
        value: Cow<'a, FeedbackValue<'c>>,
    ) -> Option<Cow<'a, FeedbackValue<'c>>> {
        match self {
            FeedbackValueTable::FromTextToDiscrete(map) => match value.as_ref() {
                FeedbackValue::Textual(v) => {
                    let discrete_value = map.get(v.text.as_ref())?;
                    let numeric_value = NumericFeedbackValue::new(
                        v.style,
                        AbsoluteValue::Discrete(Fraction::new_max(*discrete_value)),
                    );
                    Some(Cow::Owned(FeedbackValue::Numeric(numeric_value)))
                }
                _ => Some(value),
            },
            FeedbackValueTable::FromTextToContinuous(map) => match value.as_ref() {
                FeedbackValue::Textual(v) => {
                    let continuous_value = map.get(v.text.as_ref())?;
                    let numeric_value = NumericFeedbackValue::new(
                        v.style,
                        AbsoluteValue::Continuous(UnitValue::new_clamped(*continuous_value)),
                    );
                    Some(Cow::Owned(FeedbackValue::Numeric(numeric_value)))
                }
                _ => Some(value),
            },
        }
    }
}

impl Default for FeedbackValueTable {
    fn default() -> Self {
        Self::FromTextToDiscrete(HashMap::new())
    }
}

#[derive(Clone, Debug)]
pub struct ModeSettings<T: Transformation> {
    pub absolute_mode: AbsoluteMode,
    pub source_value_interval: Interval<UnitValue>,
    pub discrete_source_value_interval: Interval<u32>,
    pub target_value_interval: Interval<UnitValue>,
    pub discrete_target_value_interval: Interval<u32>,
    /// Negative increments represent fractions (throttling), e.g. -2 fires an increment every
    /// 2nd time only.
    pub step_factor_interval: Interval<DiscreteIncrement>,
    pub step_size_interval: Interval<UnitValue>,
    pub jump_interval: Interval<UnitValue>,
    pub discrete_jump_interval: Interval<u32>,
    pub takeover_mode: TakeoverMode,
    pub encoder_usage: EncoderUsage,
    pub button_usage: ButtonUsage,
    pub reverse: bool,
    pub rotate: bool,
    pub round_target_value: bool,
    pub out_of_range_behavior: OutOfRangeBehavior,
    pub control_transformation: Option<T>,
    pub feedback_transformation: Option<T>,
    pub feedback_value_table: Option<FeedbackValueTable>,
    /// Converts incoming relative messages to absolute ones.
    pub make_absolute: bool,
    /// Not in use at the moment, should always be `false`.
    pub use_discrete_processing: bool,
    pub fire_mode: FireMode,
    pub press_duration_interval: Interval<Duration>,
    pub turbo_rate: Duration,
    pub target_value_sequence: ValueSequence,
    pub feedback_type: FeedbackType,
    pub textual_feedback_expression: String,
    pub feedback_color: Option<VirtualColor>,
    pub feedback_background_color: Option<VirtualColor>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VirtualColor {
    Rgb(RgbColor),
    Prop {
        #[serde(rename = "prop")]
        prop: String,
    },
}

impl VirtualColor {
    fn resolve(&self, get_prop_value: impl Fn(&str) -> Option<PropValue>) -> Option<RgbColor> {
        use VirtualColor::*;
        match self {
            Rgb(color) => Some(*color),
            Prop { prop } => {
                if let PropValue::Color(color) = get_prop_value(prop)? {
                    Some(color)
                } else {
                    None
                }
            }
        }
    }
}

const ZERO_DURATION: Duration = Duration::from_millis(0);

impl<T: Transformation> Default for ModeSettings<T> {
    fn default() -> Self {
        ModeSettings {
            absolute_mode: AbsoluteMode::Normal,
            source_value_interval: full_unit_interval(),
            discrete_source_value_interval: full_discrete_interval(),
            target_value_interval: full_unit_interval(),
            discrete_target_value_interval: full_discrete_interval(),
            step_size_interval: default_step_size_interval(),
            step_factor_interval: default_step_count_interval(),
            jump_interval: full_unit_interval(),
            discrete_jump_interval: full_discrete_interval(),
            takeover_mode: Default::default(),
            button_usage: Default::default(),
            encoder_usage: Default::default(),
            reverse: false,
            round_target_value: false,
            out_of_range_behavior: OutOfRangeBehavior::MinOrMax,
            control_transformation: None,
            feedback_transformation: None,
            rotate: false,
            make_absolute: false,
            use_discrete_processing: false,
            fire_mode: FireMode::Normal,
            press_duration_interval: Interval::new(ZERO_DURATION, ZERO_DURATION),
            turbo_rate: ZERO_DURATION,
            target_value_sequence: Default::default(),
            feedback_type: Default::default(),
            textual_feedback_expression: Default::default(),
            feedback_color: None,
            feedback_background_color: None,
            feedback_value_table: None,
        }
    }
}

/// Settings for processing all kinds of control values.
///
/// ## How relative control values are processed (or button taps interpreted as increments).
///
/// Here's an overview in which cases step counts are used and in which step sizes.
/// This is the same, no matter if the source emits relative increments or absolute values
/// ("relative one-direction mode").
///   
/// - Target wants relative increments: __Step counts__
///     - Example: Action with invocation type "Relative"
///     - Displayed as: "{count} x"
/// - Target wants absolute values
///     - Target is continuous, optionally roundable: __Step sizes__
///         - Example: Track volume
///         - Displayed as: "{size} {unit}"
///     - Target is discrete: __Step counts__
///         - Example: FX preset, some FX params
///         - Displayed as: "{count} x" or "{count}" (former if source emits increments) TODO I
///           think now we have only the "x" variant
#[derive(Clone, Debug)]
pub struct Mode<T: Transformation, S: AbstractTimestamp> {
    settings: ModeSettings<T>,
    state: ModeState<S>,
}

#[derive(Clone, Debug)]
struct ModeState<S: AbstractTimestamp> {
    press_duration_processor: PressDurationProcessor,
    /// For relative-to-absolute mode
    current_absolute_value: UnitValue,
    #[allow(dead_code)]
    discrete_current_absolute_value: u32,
    /// Counter for implementing throttling.
    ///
    /// Throttling is implemented by spitting out control values only every nth time. The counter
    /// can take positive or negative values in order to detect direction changes. This is positive
    /// when the last change was a positive increment and negative when the last change was a
    /// negative increment.
    increment_counter: i32,
    /// Used in absolute control for "make relative" and certain takeover modes to calculate the
    /// next value based on the previous one.
    previous_absolute_value_event: Option<ControlEvent<AbsoluteValue, S>>,
    // For absolute control
    unpacked_target_value_sequence: Vec<UnitValue>,
    // For relative control
    unpacked_target_value_set: BTreeSet<UnitValue>,
    // For textual feedback
    feedback_props_in_use: HashSet<String>,
}

impl<S: AbstractTimestamp> Default for ModeState<S> {
    fn default() -> Self {
        Self {
            press_duration_processor: Default::default(),
            current_absolute_value: Default::default(),
            discrete_current_absolute_value: 0,
            increment_counter: 0,
            previous_absolute_value_event: None,
            unpacked_target_value_sequence: vec![],
            unpacked_target_value_set: Default::default(),
            feedback_props_in_use: Default::default(),
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, IntoEnumIterator, TryFromPrimitive, IntoPrimitive, Display,
)]
#[cfg_attr(feature = "serde_repr", derive(Serialize_repr, Deserialize_repr))]
#[repr(usize)]
pub enum AbsoluteMode {
    #[display(fmt = "Normal")]
    Normal = 0,
    #[display(fmt = "Incremental button")]
    IncrementalButton = 1,
    #[display(fmt = "Toggle button")]
    ToggleButton = 2,
    #[display(fmt = "Make relative")]
    MakeRelative = 3,
    #[display(fmt = "Performance control")]
    PerformanceControl = 4,
}

impl Default for AbsoluteMode {
    fn default() -> Self {
        AbsoluteMode::Normal
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, IntoEnumIterator, TryFromPrimitive, IntoPrimitive, Display,
)]
#[cfg_attr(feature = "serde_repr", derive(Serialize_repr, Deserialize_repr))]
#[repr(usize)]
pub enum FeedbackType {
    #[display(fmt = "Numeric feedback: Transformation (EEL)")]
    Numerical = 0,
    #[display(fmt = "Textual feedback: Text expression")]
    Textual = 1,
}

impl FeedbackType {
    pub fn is_textual(self) -> bool {
        self == FeedbackType::Textual
    }
}

impl Default for FeedbackType {
    fn default() -> Self {
        FeedbackType::Numerical
    }
}

pub struct ModeGarbage<T> {
    _control_transformation: Option<T>,
    _feedback_transformation: Option<T>,
    _target_value_sequence: ValueSequence,
    _unpacked_target_value_sequence: Vec<UnitValue>,
    _unpacked_target_value_set: BTreeSet<UnitValue>,
    _textual_feedback_expression: String,
    _feedback_color: Option<VirtualColor>,
    _feedback_background_color: Option<VirtualColor>,
    _feedback_props_in_use: HashSet<String>,
    _feedback_value_table: Option<FeedbackValueTable>,
}

/// Human-readable numeric value (not normalized, not zero-rooted).
///
/// The concrete type (decimal, discrete) just serves as a hint how to do the default formatting:
/// With or without decimal points. In general, all numeric values should be treatable the same
/// way, which is especially important if we want to add "value formatters" to the textual feedback
/// expressions in future. Numeric value formatters then should work on all numeric values in the
/// same way, the sub type shouldn't make a difference.
#[derive(Clone, PartialEq, Debug)]
pub enum NumericValue {
    Decimal(f64),
    /// Not zero-rooted if it's a number that represents a position.
    Discrete(i32),
}

#[derive(Clone, PartialEq, Debug)]
pub enum PropValue {
    /// Aka percentage.
    Normalized(UnitValue),
    /// Always a number that represents a position. Zero-rooted. So not human-friendly (which is
    /// the difference to `Numeric`)! Important for users to know that such a type is
    /// returned because then they know that they just need to add a *one* in order to obtain a
    /// human-friendly position. We don't want to provide each prop value as both 0-rooted index and
    /// 1-rooted position.
    Index(u32),
    /// Human-friendly numeric representation.
    Numeric(NumericValue),
    /// Textual representation.
    Text(Cow<'static, str>),
    /// Color.
    Color(RgbColor),
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct RgbColor(u8, u8, u8);

impl RgbColor {
    pub const BLACK: Self = Self::new(0x00, 0x00, 0x00);
    pub const WHITE: Self = Self::new(0xFF, 0xFF, 0xFF);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        RgbColor(r, g, b)
    }

    pub const fn r(&self) -> u8 {
        self.0
    }

    pub const fn g(&self) -> u8 {
        self.1
    }

    pub const fn b(&self) -> u8 {
        self.2
    }
}

impl Default for PropValue {
    fn default() -> Self {
        Self::Text(Cow::Owned(String::new()))
    }
}

impl PropValue {
    pub fn to_percentage(&self) -> Option<AbsoluteValue> {
        use PropValue::*;
        if let Normalized(v) = self {
            Some(AbsoluteValue::Continuous(*v))
        } else {
            None
        }
    }

    pub fn into_textual(self) -> Cow<'static, str> {
        use PropValue::*;
        match self {
            Normalized(v) => format!("{:.2}", v.get() * 100.0).into(),
            Numeric(v) => v.into_textual().into(),
            Index(i) => i.to_string().into(),
            Text(text) => text,
            Color(color) => format!("{:?}", color).into(),
        }
    }
}

impl NumericValue {
    pub fn into_textual(self) -> String {
        use NumericValue::*;
        match self {
            Decimal(v) => format!("{:.2}", v),
            Discrete(v) => v.to_string(),
        }
    }
}

impl<T: Transformation, S: AbstractTimestamp> Mode<T, S> {
    pub fn new(settings: ModeSettings<T>) -> Self {
        let state = ModeState {
            press_duration_processor: PressDurationProcessor::new(
                settings.fire_mode,
                settings.press_duration_interval,
                settings.turbo_rate,
            ),
            feedback_props_in_use: {
                let mut set = HashSet::new();
                if settings.feedback_type.is_textual() {
                    if settings.textual_feedback_expression.is_empty() {
                        set.insert(DEFAULT_TEXTUAL_FEEDBACK_PROP_KEY.to_string());
                    } else {
                        set.extend(
                            textual_feedback_expression_regex()
                                .captures_iter(&settings.textual_feedback_expression)
                                .map(|cap| cap[1].to_string()),
                        );
                    }
                }
                if let Some(VirtualColor::Prop { prop }) = settings.feedback_color.as_ref() {
                    set.insert(prop.to_string());
                }
                if let Some(VirtualColor::Prop { prop }) =
                    settings.feedback_background_color.as_ref()
                {
                    set.insert(prop.to_string());
                }
                set
            },
            ..Default::default()
        };
        Mode { settings, state }
    }

    pub fn settings(&self) -> &ModeSettings<T> {
        &self.settings
    }

    /// For deferring deallocation to non-real-time thread.
    pub fn recycle(self) -> ModeGarbage<T> {
        ModeGarbage {
            _control_transformation: self.settings.control_transformation,
            _feedback_transformation: self.settings.feedback_transformation,
            _target_value_sequence: self.settings.target_value_sequence,
            _unpacked_target_value_sequence: self.state.unpacked_target_value_sequence,
            _unpacked_target_value_set: self.state.unpacked_target_value_set,
            _textual_feedback_expression: self.settings.textual_feedback_expression,
            _feedback_color: self.settings.feedback_color,
            _feedback_background_color: self.settings.feedback_background_color,
            _feedback_props_in_use: self.state.feedback_props_in_use,
            _feedback_value_table: self.settings.feedback_value_table,
        }
    }

    /// Processes the given control value and maybe returns an appropriate target control value.
    ///
    /// `None` either means ignored or target value already has desired value.
    #[cfg(test)]
    fn control<'a, C: Copy + TransformationInputProvider<T::AdditionalInput> + Into<TC>, TC>(
        &mut self,
        control_event: ControlEvent<ControlValue, S>,
        target: &impl Target<'a, Context = TC>,
        context: C,
    ) -> Option<ControlValue> {
        self.control_with_options(
            control_event,
            target,
            context,
            ModeControlOptions::default(),
            None,
        )?
        .into()
    }

    /// Processes the given control value and maybe returns an appropriate target control value.
    ///
    /// `None` means the incoming source control value doesn't reach the target because it's
    /// filtered out (e.g. because of button filter "Press only").
    pub fn control_with_options<
        'a,
        C: Copy + TransformationInputProvider<T::AdditionalInput> + Into<TC>,
        TC,
    >(
        &mut self,
        control_event: ControlEvent<ControlValue, S>,
        target: &impl Target<'a, Context = TC>,
        context: C,
        options: ModeControlOptions,
        last_non_performance_target_value: Option<AbsoluteValue>,
    ) -> Option<ModeControlResult<ControlValue>> {
        match control_event.payload() {
            ControlValue::AbsoluteContinuous(v) => self.control_absolute(
                control_event.with_payload(AbsoluteValue::Continuous(v)),
                target,
                context,
                true,
                options,
                last_non_performance_target_value,
            ),
            ControlValue::AbsoluteDiscrete(v) => self.control_absolute(
                control_event.with_payload(AbsoluteValue::Discrete(v)),
                target,
                context,
                true,
                options,
                last_non_performance_target_value,
            ),
            ControlValue::RelativeDiscrete(i) => self.control_relative(
                control_event.with_payload(Increment::Discrete(i)),
                target,
                context,
                options,
            ),
            ControlValue::RelativeContinuous(i) => self.control_relative(
                control_event.with_payload(Increment::Continuous(i)),
                target,
                context,
                options,
            ),
        }
    }

    pub fn wants_textual_feedback(&self) -> bool {
        self.settings.feedback_type.is_textual()
    }

    pub fn feedback_props_in_use(&self) -> &HashSet<String> {
        &self.state.feedback_props_in_use
    }

    pub fn query_textual_feedback(
        &self,
        get_prop_value: &impl Fn(&str) -> Option<PropValue>,
    ) -> TextualFeedbackValue {
        let text = if self.settings.textual_feedback_expression.is_empty() {
            get_prop_value(DEFAULT_TEXTUAL_FEEDBACK_PROP_KEY)
                .unwrap_or_default()
                .into_textual()
        } else {
            textual_feedback_expression_regex().replace_all(
                &self.settings.textual_feedback_expression,
                |c: &Captures| get_prop_value(&c[1]).unwrap_or_default().into_textual(),
            )
        };
        TextualFeedbackValue::new(self.feedback_style(get_prop_value), text)
    }

    pub fn feedback_style(
        &self,
        get_prop_value: &impl Fn(&str) -> Option<PropValue>,
    ) -> FeedbackStyle {
        FeedbackStyle {
            color: self
                .settings
                .feedback_color
                .as_ref()
                .and_then(|c| c.resolve(get_prop_value)),
            background_color: self
                .settings
                .feedback_background_color
                .as_ref()
                .and_then(|c| c.resolve(get_prop_value)),
        }
    }

    #[cfg(test)]
    fn feedback(&self, target_value: AbsoluteValue) -> Option<AbsoluteValue> {
        self.feedback_with_options(target_value, ModeFeedbackOptions::default())
    }

    #[cfg(test)]
    fn feedback_with_options(
        &self,
        target_value: AbsoluteValue,
        options: ModeFeedbackOptions,
    ) -> Option<AbsoluteValue> {
        let in_cow = Cow::Owned(FeedbackValue::Numeric(NumericFeedbackValue::new(
            Default::default(),
            target_value,
        )));
        let out_cow = self.feedback_with_options_detail(in_cow, options, Default::default())?;
        Some(out_cow.to_numeric()?.value)
    }

    /// Takes a target value, interprets and transforms it conforming to mode rules and
    /// maybe returns an appropriate source value that should be sent to the source.
    pub fn feedback_with_options_detail<'a, 'b, 'c>(
        &'b self,
        target_value: Cow<'a, FeedbackValue<'c>>,
        options: ModeFeedbackOptions,
        additional_transformation_input: T::AdditionalInput,
    ) -> Option<Cow<'a, FeedbackValue<'c>>> {
        match target_value {
            Cow::Owned(FeedbackValue::Numeric(feedback_value)) => {
                let res = self.feedback_numerical_target_value(
                    feedback_value,
                    options,
                    additional_transformation_input,
                )?;
                Some(Cow::Owned(res))
            }
            Cow::Borrowed(FeedbackValue::Numeric(feedback_value)) => {
                let res = self.feedback_numerical_target_value(
                    *feedback_value,
                    options,
                    additional_transformation_input,
                )?;
                Some(Cow::Owned(res))
            }
            v => {
                if let Some(table) = self.settings.feedback_value_table.as_ref() {
                    table.transform_value(v)
                } else {
                    Some(v)
                }
            }
        }
    }

    fn feedback_numerical_target_value(
        &self,
        feedback_value: NumericFeedbackValue,
        options: ModeFeedbackOptions,
        additional_transformation_input: T::AdditionalInput,
    ) -> Option<FeedbackValue<'static>> {
        let v = feedback_value.value;
        // 4. Filter and Apply target interval (normalize)
        let interval_match_result = v.matches_tolerant(
            &self.settings.target_value_interval,
            &self.settings.discrete_target_value_interval,
            self.settings.use_discrete_processing,
            FEEDBACK_EPSILON,
        );
        let (mut v, min_is_max_behavior) = if interval_match_result.matches() {
            // Target value is within target value interval
            (v, MinIsMaxBehavior::PreferOne)
        } else {
            // Target value is outside target value interval
            self.settings.out_of_range_behavior.process(
                v,
                interval_match_result,
                &self.settings.target_value_interval,
                &self.settings.discrete_target_value_interval,
            )?
        };
        // Tolerant interval bounds test because of https://github.com/helgoboss/realearn/issues/263.
        // TODO-medium The most elaborate solution to deal with discrete values would be to actually
        //  know which interval of floating point values represents a specific discrete target value.
        //  However, is there a generic way to know that? Taking the target step size as epsilon in this
        //  case sounds good but we still don't know if the target respects approximate values, if it
        //  rounds them or uses more a ceil/floor approach ... I don't think this is standardized for
        //  VST parameters. We could solve it for our own parameters in future. Until then, having a
        //  fixed epsilon deals at least with most issues I guess.
        v = v.normalize(
            &self.settings.target_value_interval,
            &self.settings.discrete_target_value_interval,
            min_is_max_behavior,
            self.settings.use_discrete_processing,
            FEEDBACK_EPSILON,
        );
        // 3. Apply reverse
        if self.settings.reverse {
            let normalized_max_discrete_source_value = options.max_discrete_source_value.map(|m| {
                self.settings
                    .discrete_source_value_interval
                    .normalize_to_min(m)
            });
            v = v.inverse(normalized_max_discrete_source_value);
        };
        // 2. Apply transformation
        if let Some(transformation) = self.settings.feedback_transformation.as_ref() {
            if let Ok(output) = v.transform(
                transformation,
                Some(v),
                self.settings.use_discrete_processing,
                self.transformation_input_meta_data(),
                additional_transformation_input,
            ) {
                v = output.value()?;
            }
        };
        // 1. Apply source interval
        v = v.denormalize(
            &self.settings.source_value_interval,
            &self.settings.discrete_source_value_interval,
            self.settings.use_discrete_processing,
            options.max_discrete_source_value,
        );
        // Result
        if !self.settings.use_discrete_processing && !options.source_is_virtual {
            // If discrete processing is not explicitly enabled, we must NOT send discrete values to
            // a real (non-virtual) source! This is not just for backward compatibility. It would change
            // how discrete sources react in a surprising way (discrete behavior without having
            // discrete processing enabled).
            v = v.to_continuous_value();
        };
        Some(FeedbackValue::Numeric(NumericFeedbackValue::new(
            feedback_value.style,
            v,
        )))
    }

    fn process_control_transformation_output<O>(
        &mut self,
        output: TransformationOutput<O>,
    ) -> Option<O> {
        match output {
            TransformationOutput::Stop => {
                // Resetting the previous event will stop polling until the next mapping
                // invocation.
                self.state.previous_absolute_value_event = None;
                None
            }
            TransformationOutput::None => None,
            TransformationOutput::Control(v) => Some(v),
        }
    }

    fn transformation_input_meta_data(&self) -> TransformationInputMetaData {
        let rel_time = self
            .state
            .previous_absolute_value_event
            .map(|evt| S::now() - evt.timestamp())
            .unwrap_or_default();
        TransformationInputMetaData { rel_time }
    }

    /// If this returns `true`, the `poll` method should be called, on a regular basis.
    pub fn wants_to_be_polled(&self) -> bool {
        self.state.press_duration_processor.wants_to_be_polled()
            || self
                .settings
                .control_transformation
                .iter()
                .any(|t| t.wants_to_be_polled())
    }

    /// This function should be called regularly if the features are needed that are driven by a
    /// timer (fire on length min, turbo, etc.). Returns a target control value whenever it's time
    /// to fire.
    pub fn poll<'a, C: Copy + TransformationInputProvider<T::AdditionalInput> + Into<TC>, TC>(
        &mut self,
        target: &impl Target<'a, Context = TC>,
        context: C,
        timestamp: S,
    ) -> Option<ModeControlResult<ControlValue>> {
        // If we have a transformation which depends on the current timestamp, we poll this one.
        if let Some(transformation) = &self.settings.control_transformation {
            if transformation.wants_to_be_polled() {
                let evt = self.state.previous_absolute_value_event?;
                let output = evt
                    .payload()
                    .transform(
                        transformation,
                        target.current_value(context.into()),
                        self.settings.use_discrete_processing,
                        self.transformation_input_meta_data(),
                        context.additional_input(),
                    )
                    .ok()?;
                let v = self.process_control_transformation_output(output)?;
                let v = ControlValue::from_absolute(v);
                return Some(ModeControlResult::hit_target(v));
            }
        }
        // If not, we let the press duration processor do its job.
        let control_value = self.state.press_duration_processor.poll()?;
        self.control_absolute(
            ControlEvent::new(control_value, timestamp),
            target,
            context,
            false,
            ModeControlOptions::default(),
            // Polling is only for buttons. "Performance control" mode is only for range elements.
            None,
        )
    }

    /// This should be called when the containing mapping gets deactivated.
    ///
    /// Attention: At the moment it can be called even if the mapping was already inactive.
    /// So it should be idempotent!
    pub fn on_deactivate(&mut self) {
        // Clear the previous absolute value so we don't get jumps when using "Make relative"
        // and using this mapping next time it's active again.
        self.state.previous_absolute_value_event = None;
    }

    /// Gives the mode the opportunity to update internal state when it's being connected to a
    /// target (either initial target resolve or refreshing target resolve).  
    pub fn update_from_target<'a, C: Copy + Into<TC>, TC>(
        &mut self,
        target: &impl Target<'a, Context = TC>,
        context: C,
    ) {
        let default_step_size = target
            .control_type(context.into())
            .step_size()
            .unwrap_or_else(|| UnitValue::new(DEFAULT_STEP_SIZE));
        let unpacked_sequence = self
            .settings
            .target_value_sequence
            .unpack(default_step_size);
        self.state.unpacked_target_value_set = unpacked_sequence.iter().copied().collect();
        self.state.unpacked_target_value_sequence = unpacked_sequence;
    }

    fn control_relative<
        'a,
        C: Copy + TransformationInputProvider<T::AdditionalInput> + Into<TC>,
        TC,
    >(
        &mut self,
        control_event: ControlEvent<Increment, S>,
        target: &impl Target<'a, Context = TC>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        if !self.settings.encoder_usage.matches(control_event.payload()) {
            return None;
        }
        if self.settings.make_absolute {
            Some(
                self.control_relative_to_absolute(control_event, target, context, options)?
                    .map(|v| ControlValue::AbsoluteContinuous(v.to_unit_value())),
            )
        } else {
            self.control_relative_normal(control_event.payload(), target, context, options)
        }
    }

    fn control_absolute<
        'a,
        C: Copy + TransformationInputProvider<T::AdditionalInput> + Into<TC>,
        TC,
    >(
        &mut self,
        control_event: ControlEvent<AbsoluteValue, S>,
        target: &impl Target<'a, Context = TC>,
        context: C,
        consider_press_duration: bool,
        options: ModeControlOptions,
        last_non_performance_target_value: Option<AbsoluteValue>,
    ) -> Option<ModeControlResult<ControlValue>> {
        // Filter presses/releases. Makes sense only for absolute mode "Normal". If this is used
        // a filter is used with another absolute mode, it's considered a usage fault.
        let mut v = control_event.payload();
        match self.settings.button_usage {
            ButtonUsage::PressOnly if v.is_zero() => return None,
            ButtonUsage::ReleaseOnly if !v.is_zero() => return None,
            _ => {}
        };
        // Press duration
        if consider_press_duration {
            v = self
                .state
                .press_duration_processor
                .process_press_or_release(v)?;
        }
        // Dispatch
        let control_event = control_event.with_payload(v);
        use AbsoluteMode::*;
        match self.settings.absolute_mode {
            Normal => Some(
                self.control_absolute_normal(control_event, target, context, None)?
                    .map(ControlValue::from_absolute),
            ),
            IncrementalButton => self.control_absolute_incremental_buttons(
                control_event.with_payload(v.to_unit_value()),
                target,
                context,
                options,
            ),
            ToggleButton => Some(
                self.control_absolute_toggle_buttons(v, target, context)?
                    .map(|v| ControlValue::AbsoluteContinuous(v.to_unit_value())),
            ),
            MakeRelative => {
                self.control_absolute_to_relative(control_event, target, context, options)
            }
            PerformanceControl => Some(
                self.control_absolute_normal(
                    control_event,
                    target,
                    context,
                    last_non_performance_target_value,
                )?
                .map(ControlValue::from_absolute),
            ),
        }
    }

    /// Processes the given control value in absolute mode and maybe returns an appropriate target
    /// value.
    ///
    /// Provide `last_non_performance_target_value` only if you want "Performance control".
    fn control_absolute_normal<
        'a,
        C: Copy + TransformationInputProvider<T::AdditionalInput> + Into<TC>,
        TC,
    >(
        &mut self,
        control_event: ControlEvent<AbsoluteValue, S>,
        target: &impl Target<'a, Context = TC>,
        context: C,
        last_non_performance_target_value: Option<AbsoluteValue>,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        let res = self.pre_process_absolute_value(control_event)?;
        let current_target_value = target.current_value(context.into());
        let control_type = target.control_type(context.into());
        let pepped_up_control_value = self.pep_up_absolute_value(
            res.control_event.payload(),
            control_type,
            current_target_value,
            context.additional_input(),
            last_non_performance_target_value,
        )?;
        self.hitting_target_considering_max_jump(
            pepped_up_control_value,
            current_target_value,
            control_type,
            res.control_event,
            res.prev_control_event,
        )
    }

    fn pre_process_absolute_value(
        &mut self,
        control_event: ControlEvent<AbsoluteValue, S>,
    ) -> Option<AbsolutePreProcessingResult<S>> {
        let control_value = control_event.payload();
        let interval_match_result = control_value.matches_tolerant(
            &self.settings.source_value_interval,
            &self.settings.discrete_source_value_interval,
            self.settings.use_discrete_processing,
            BASE_EPSILON,
        );
        let (source_bound_value, min_is_max_behavior) = if interval_match_result.matches() {
            // Control value is within source value interval
            (control_value, MinIsMaxBehavior::PreferOne)
        } else {
            // Control value is outside source value interval
            // TODO-high Check if the lack of `use_discrete_processing` is a problem here (that
            //  we use the discrete interval although it's not currently set). It shouldn't
            //  cause an issue because it only has an effect if source min/max are non-default
            //  and then this will be normalized to 0.0 or 1.0 anyway in the next step.
            //  However, we should make this more clear.
            // TODO-high Having all the dead code for the discrete processing logic is not good.
            //  That code needs to grow with the rest. Idea: Unlock discrete processing at first
            //  with only a few very simple operators. Hide the rest.
            //  Unlock more complicated ones later if necessary.
            self.settings.out_of_range_behavior.process(
                control_value,
                interval_match_result,
                &self.settings.source_value_interval,
                &self.settings.discrete_source_value_interval,
            )?
        };
        // Control value is within source value interval
        // 1. Apply source interval
        let source_normalized_control_value = source_bound_value.normalize(
            &self.settings.source_value_interval,
            &self.settings.discrete_source_value_interval,
            min_is_max_behavior,
            self.settings.use_discrete_processing,
            BASE_EPSILON,
        );
        // Memorize as previous value for next control cycle.
        let prev_absolute_control_event = self
            .state
            .previous_absolute_value_event
            .replace(control_event.with_payload(source_normalized_control_value));
        let res = AbsolutePreProcessingResult {
            control_event: control_event.with_payload(source_normalized_control_value),
            prev_control_event: prev_absolute_control_event,
        };
        Some(res)
    }

    /// "Incremental button" mode: Convert absolute button presses to relative increments,
    /// taking the velocity into account.
    fn control_absolute_incremental_buttons<
        'a,
        C: Copy + TransformationInputProvider<T::AdditionalInput> + Into<TC>,
        TC,
    >(
        &mut self,
        control_event: ControlEvent<UnitValue, S>,
        target: &impl Target<'a, Context = TC>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        // TODO-high-discrete In discrete processing, don't interpret current target value as percentage!
        if control_event.payload().is_zero()
            || !self
                .settings
                .source_value_interval
                .value_matches_tolerant(control_event.payload(), BASE_EPSILON)
                .matches()
        {
            return None;
        }
        if self.settings.make_absolute {
            // Convert absolute button presses to increments/decrements of pseudo absolute value.
            // We could just as well convert to a continuous increment (we have one now).
            // However, I think it's not worth to change existing behavior at this point. First,
            // nobody expects 100% accuracy incrementing/decrementing something via button-press
            // velocity. Second, using "Incremental buttons" with "Make absolute" is probably
            // something people rarely use.
            let discrete_increment = self.convert_to_discrete_increment(control_event.payload())?;
            Some(
                self.control_relative_to_absolute(
                    control_event.with_payload(Increment::Discrete(discrete_increment)),
                    target,
                    context,
                    options,
                )?
                .map(|v| ControlValue::AbsoluteContinuous(v.to_unit_value())),
            )
        } else {
            // Initiate relative control via incremental buttons
            self.control_absolute_incremental_buttons_normal(
                control_event.payload(),
                target,
                context,
                options,
            )
        }
    }

    fn control_absolute_incremental_buttons_normal<'a, C: Copy + Into<TC>, TC>(
        &mut self,
        control_value: UnitValue,
        target: &impl Target<'a, Context = TC>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        if !self.state.unpacked_target_value_set.is_empty() {
            let discrete_increment = self.convert_to_discrete_increment(control_value)?;
            return self.control_relative_target_value_set(
                discrete_increment,
                target,
                context,
                options,
            );
        }
        use ControlType::*;
        let control_type = target.control_type(context.into());
        match control_type {
            AbsoluteContinuous
            | AbsoluteContinuousRoundable { .. }
            // TODO-low I think trigger and switch targets don't make sense at all here because
            //  instead of +/- n they need just "trigger!" or "on/off!". 
            | AbsoluteContinuousRetriggerable => {
                // Continuous target
                //
                // Settings:
                // - Source value interval (for setting the input interval of relevant source
                //   values)
                // - Minimum target step size (enables accurate minimum increment, atomic)
                // - Maximum target step size (enables accurate maximum increment, clamped)
                // - Target value interval (absolute, important for rotation only, clamped)
                let step_size_value = control_value
                    .normalize(
                        &self.settings.source_value_interval,
                        MinIsMaxBehavior::PreferOne,
                        BASE_EPSILON
                    )
                    .denormalize(&self.settings.step_size_interval);
                let step_size_increment =
                    step_size_value.to_increment(negative_if(self.settings.reverse))?;
                self.hit_target_absolutely_with_unit_increment(
                    step_size_increment,
                    // We don't want to adjust to a grid, target min/max are more important
                    // (see #577).
                    None,
                    target.current_value(context.into())?.to_unit_value(),
                    options,
                )
            }
            AbsoluteDiscrete { atomic_step_size, .. } => {
                // Discrete target
                //
                // Settings:
                // - Source value interval (for setting the input interval of relevant source
                //   values)
                // - Minimum target step count (enables accurate normal/minimum increment, atomic)
                // - Target value interval (absolute, important for rotation only, clamped)
                // - Maximum target step count (enables accurate maximum increment, clamped)
                let discrete_increment = self.convert_to_discrete_increment(control_value)?;
                self.hit_discrete_target_absolutely(Increment::Discrete(discrete_increment), atomic_step_size, options, control_type, || {
                    target.current_value(context.into())
                })
            }
            Relative
            // This is cool! With this, we can make controllers without encoders simulate them
            // by assigning one - button and one + button to the same virtual multi target.
            // Of course, all we can deliver is increments/decrements since virtual targets 
            // don't provide a current target value. But we also don't need it because all we
            // want to do is simulate an encoder.
            | VirtualMulti => {
                // Target wants increments so we just generate them e.g. depending on how hard the
                // button has been pressed
                //
                // - Source value interval (for setting the input interval of relevant source
                //   values)
                // - Minimum target step count (enables accurate normal/minimum increment, atomic)
                // - Maximum target step count (enables accurate maximum increment, mapped)
                let discrete_increment = self.convert_to_discrete_increment(control_value)?;
                Some(ModeControlResult::hit_target(ControlValue::RelativeDiscrete(discrete_increment)))
            }
            VirtualButton => {
                // This doesn't make sense at all. Buttons just need to be triggered, not fed with
                // +/- n.
                None
            }
        }
    }

    fn control_absolute_toggle_buttons<'a, C: Copy + Into<TC>, TC>(
        &mut self,
        control_value: AbsoluteValue,
        target: &impl Target<'a, Context = TC>,
        context: C,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        // TODO-high-discrete In discrete processing, don't interpret current target value as
        //  percentage!
        if control_value.is_zero() {
            return None;
        }
        // Nothing we can do if we can't get the current target value. This shouldn't happen
        // usually because virtual targets are not supposed to be used with toggle mode.
        let current_target_value = target.current_value(context.into())?;
        let desired_target_value = if self.settings.target_value_interval.min_is_max(BASE_EPSILON) {
            // Special case #452 (target min == target max).
            // Make it usable for exclusive toggle buttons.
            if current_target_value
                .matches_tolerant(
                    &self.settings.target_value_interval,
                    &self.settings.discrete_target_value_interval,
                    false,
                    BASE_EPSILON,
                )
                .matches()
            {
                UnitValue::MIN
            } else {
                self.settings.target_value_interval.max_val()
            }
        } else {
            // Normal case (target min != target max)
            let center_target_value = self.settings.target_value_interval.center();
            if current_target_value.to_unit_value() > center_target_value {
                // Target value is within the second half of the target range (considered as on).
                self.settings.target_value_interval.min_val()
            } else {
                // Target value is within the first half of the target range (considered as off).
                self.settings.target_value_interval.max_val()
            }
        };
        // If the settings make sense for toggling, the desired target value should *always*
        // be different than the current value. Therefore no need to check if the target value
        // already has that value.
        let final_absolute_value = self.get_final_absolute_value(
            AbsoluteValue::Continuous(desired_target_value),
            target.control_type(context.into()),
        );
        Some(ModeControlResult::hit_target(final_absolute_value))
    }

    /// Absolute-to-relative conversion mode.
    fn control_absolute_to_relative<
        'a,
        C: Copy + TransformationInputProvider<T::AdditionalInput> + Into<TC>,
        TC,
    >(
        &mut self,
        control_event: ControlEvent<AbsoluteValue, S>,
        target: &impl Target<'a, Context = TC>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        // If discrete processing not explicitly enabled, go continuous right here!
        // If not, discrete-relative things like step sizes or speed factors would be taken into
        // account, which we don't want for the "Make relative" mode (at least if not explicitly
        // enabled in future). It's important that an absolute swipe from 0% to 100% applied
        // relative to a target with initial value 0% results in a target value swipe from
        // 0% to 100%, no matter step sizes! That's what we expect from this mode.
        let control_event = if self.settings.use_discrete_processing {
            control_event
        } else {
            control_event.map_payload(|v| v.to_continuous_value())
        };
        let res = self.pre_process_absolute_value(control_event)?;
        // We can't do anything without having a previous value to relate to.
        let prev_control_value = res.prev_control_event?;
        let increment = match res.control_event.payload() {
            AbsoluteValue::Continuous(v) => {
                // This is kind of new: Continuous relative increments.
                let prev_control_value = prev_control_value.payload().continuous_value()?;
                let diff = v.get() - prev_control_value.get();
                let increment = UnitIncrement::try_from(diff).ok()?;
                Increment::Continuous(increment)
            }
            AbsoluteValue::Discrete(f) => {
                // This is easy and accurate! Conforms to our normal relative control stuff,
                // which has always been discrete in nature.
                // Be aware, even if the source emits discrete values, if source min/max is set,
                // we won't arrive in this match branch because discrete processing is not unlocked
                // yet!
                let prev_control_value = prev_control_value.payload().discrete_value()?;
                let diff = f.actual() as i32 - prev_control_value.actual() as i32;
                let increment = DiscreteIncrement::try_from(diff).ok()?;
                Increment::Discrete(increment)
            }
        };
        if !self.settings.encoder_usage.matches(increment) {
            return None;
        }
        // We ignore steps because the most important thing about this mode is that we can do
        // full sweeps, no matter the character of the target and potential discrete steps.
        self.control_relative_normal(increment, target, context, options)
    }

    /// Relative-to-absolute conversion mode.
    ///
    /// Takes care of:
    ///
    /// - Conversion to absolute value
    /// - Step size interval
    /// - Wrap (rotate)
    fn control_relative_to_absolute<
        'a,
        C: Copy + TransformationInputProvider<T::AdditionalInput> + Into<TC>,
        TC,
    >(
        &mut self,
        control_event: ControlEvent<Increment, S>,
        target: &impl Target<'a, Context = TC>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        // Convert to absolute value
        let mut inc = control_event
            .payload()
            .to_unit_increment(self.settings.step_size_interval.min_val())?;
        inc = inc.clamp_to_interval(&self.settings.step_size_interval)?;
        let full_unit_interval = full_unit_interval();
        let abs_input_value = if options.enforce_rotate || self.settings.rotate {
            self.state
                .current_absolute_value
                .add_rotating(inc, &full_unit_interval, BASE_EPSILON)
        } else {
            self.state
                .current_absolute_value
                .add_clamping(inc, &full_unit_interval, BASE_EPSILON)
        };
        self.state.current_absolute_value = abs_input_value;
        // Do the usual absolute processing
        self.control_absolute_normal(
            control_event.with_payload(AbsoluteValue::Continuous(abs_input_value)),
            target,
            context,
            None,
        )
    }

    // Classic relative mode: We are getting encoder increments from the source.
    // We don't need source min/max config in this case. At least I can't think of a use case
    // where one would like to totally ignore especially slow or especially fast encoder movements,
    // I guess that possibility would rather cause irritation.
    fn control_relative_normal<'a, C: Copy + Into<TC>, TC>(
        &mut self,
        increment: Increment,
        target: &impl Target<'a, Context = TC>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        if !self.state.unpacked_target_value_set.is_empty() {
            let pepped_up_increment = self.pep_up_increment(increment)?;
            // If the incoming increment is continuous, we ignore the amount and just consider
            // the direction.
            return self.control_relative_target_value_set(
                pepped_up_increment.to_discrete_increment(),
                target,
                context,
                options,
            );
        }
        use ControlType::*;
        let control_type = target.control_type(context.into());
        match control_type {
            AbsoluteContinuous
            | AbsoluteContinuousRoundable { .. }
            // TODO-low Controlling a switch/trigger target with +/- n doesn't make sense.
            | AbsoluteContinuousRetriggerable => {
                let current_target_value = match target.current_value(context.into()) {
                    None => {
                        // We couldn't obtain the current target value.
                        if control_type != ControlType::AbsoluteContinuousRetriggerable {
                            // Target is not trigger-like, there's nothing we can do.
                            return None;
                        }
                        // https://github.com/helgoboss/realearn/issues/613
                        // Target is like a trigger, so we don't actually need to know its
                        // value to trigger it. We consider a encoder movement as triggering!
                        // However, we also should support decreasing the encoder sensitivity, so
                        // we pep up the increment first to see if we need to fire.
                        self.pep_up_increment(increment)?;
                        return Some(ModeControlResult::HitTarget {
                            value: ControlValue::AbsoluteContinuous(UnitValue::MAX)
                        });
                    }
                    Some(t) => t,
                };
                // Continuous target
                //
                // Settings which are always necessary:
                // - Minimum target step size (enables accurate minimum increment, atomic)
                // - Target value interval (absolute, important for rotation only, clamped)
                //
                // Settings which are necessary in order to support >1-increments:
                // - Maximum target step size (enables accurate maximum increment, clamped)
                let potentially_reversed_increment = if self.settings.reverse {
                    increment.inverse()
                } else {
                    increment
                };
                let final_unit_increment = match potentially_reversed_increment {
                    Increment::Continuous(i) => {
                        let target_scale_factor = self.settings.target_value_interval.span();
                        UnitIncrement::try_from(i.get() * target_scale_factor).ok()?
                    },
                    Increment::Discrete(i) => {
                        let unit_increment = i
                            .to_unit_increment(self.settings.step_size_interval.min_val())?;
                        unit_increment.clamp_to_interval(&self.settings.step_size_interval)?
                    }
                };
                self.hit_target_absolutely_with_unit_increment(
                    final_unit_increment,
                    // We don't want to adjust to a grid, target min/max are more important
                    // (see #577).
                    None,
                    current_target_value.to_unit_value(),
                    options,
                )
            }
            AbsoluteDiscrete { atomic_step_size, .. } => {
                // Discrete target
                //
                // Settings which are always necessary:
                // - Minimum target step count (enables accurate normal/minimum increment, atomic)
                // - Target value interval (absolute, important for rotation only, clamped)
                //
                // Settings which are necessary in order to support >1-increments:
                // - Maximum target step count (enables accurate maximum increment, clamped)
                let pepped_up_increment = self.pep_up_increment(increment)?;
                self.hit_discrete_target_absolutely(pepped_up_increment, atomic_step_size, options, control_type, || {
                    target.current_value(context.into())
                })
            }
            Relative | VirtualMulti => {
                // Target wants increments so we just forward them after some preprocessing
                //
                // Settings which are always necessary:
                // - Minimum target step count (enables accurate normal/minimum increment, clamped)
                //
                // Settings which are necessary in order to support >1-increments:
                // - Maximum target step count (enables accurate maximum increment, clamped)
                let pepped_up_increment = self.pep_up_increment(increment)?;
                Some(ModeControlResult::hit_target(ControlValue::from_relative(pepped_up_increment)))
            }
            VirtualButton => {
                // Controlling a button target with +/- n doesn't make sense.
                None
            }
        }
    }

    /// Takes care of:
    ///
    /// - Target value set
    /// - Wrap (rotate)
    fn control_relative_target_value_set<'a, C: Copy + Into<TC>, TC>(
        &mut self,
        discrete_increment: DiscreteIncrement,
        target: &impl Target<'a, Context = TC>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        // Determine next value in target value set
        let current = target.current_value(context.into())?.to_unit_value();
        let target_value_set = &self.state.unpacked_target_value_set;
        use std::ops::Bound::*;
        let mut v = current;
        for _ in 0..discrete_increment.get().abs() {
            let next_value_in_direction = if discrete_increment.is_positive() {
                target_value_set
                    .range((
                        Excluded(UnitValue::new_clamped(v.get() + BASE_EPSILON)),
                        Unbounded,
                    ))
                    .next()
                    .copied()
            } else {
                target_value_set
                    .range((
                        Unbounded,
                        Excluded(UnitValue::new_clamped(v.get() - BASE_EPSILON)),
                    ))
                    .last()
                    .copied()
            };
            v = if let Some(v) = next_value_in_direction {
                v
            } else if options.enforce_rotate || self.settings.rotate {
                if discrete_increment.is_positive() {
                    *target_value_set.iter().next().unwrap()
                } else {
                    *target_value_set.iter().rev().next().unwrap()
                }
            } else {
                break;
            };
        }
        if v == current {
            return None;
        }
        Some(ModeControlResult::hit_target(
            ControlValue::AbsoluteContinuous(v),
        ))
    }

    fn pep_up_absolute_value(
        &mut self,
        source_normalized_control_value: AbsoluteValue,
        control_type: ControlType,
        current_target_value: Option<AbsoluteValue>,
        additional_transformation_input: T::AdditionalInput,
        last_non_performance_target_value: Option<AbsoluteValue>,
    ) -> Option<AbsoluteValue> {
        let mut v = source_normalized_control_value;
        // 1. Performance control (optional)
        let performance_control = if let Some(y_last) = last_non_performance_target_value {
            let x = v.to_unit_value().get();
            let y_last = y_last.to_unit_value().get();
            let target_min = self.settings.target_value_interval.min_val().get();
            let target_max = self.settings.target_value_interval.max_val().get();
            let y = if self.settings.reverse {
                let span = (y_last - target_min).max(0.0);
                y_last - x * span
            } else {
                let span = (target_max - y_last).max(0.0);
                y_last + x * span
            };
            v = AbsoluteValue::Continuous(UnitValue::new_clamped(y));
            true
        } else {
            false
        };
        // 2. Apply transformation
        if let Some(transformation) = self.settings.control_transformation.as_ref() {
            if let Ok(output) = v.transform(
                transformation,
                current_target_value,
                self.settings.use_discrete_processing,
                self.transformation_input_meta_data(),
                additional_transformation_input,
            ) {
                v = self.process_control_transformation_output(output)?;
            }
        };
        if performance_control {
            // Performance control. Just apply rounding.
            if self.settings.round_target_value {
                v = v.round(control_type);
            };
        } else {
            // No performance control
            // 3. Apply reverse
            if self.settings.reverse {
                // We must normalize the target value value and use it in the inversion operation.
                // As an alternative, we could BEFORE doing all that stuff homogenize the source and
                // target intervals to have the same (minimum) size?
                let normalized_max_discrete_target_value = control_type.discrete_max().map(|m| {
                    self.settings
                        .discrete_target_value_interval
                        .normalize_to_min(m)
                });
                // If this is a discrete target (which reports a discrete maximum) and discrete
                // processing is disabled, the reverse operation must use a "scaling reverse", not a
                // "subtraction reverse". Therefore we must turn a discrete control value into a
                // continuous value in this case before applying the reverse operation.
                if normalized_max_discrete_target_value.is_some()
                    && !self.settings.use_discrete_processing
                {
                    v = v.to_continuous_value();
                }
                v = v.inverse(normalized_max_discrete_target_value);
            };
            // 4. Apply target interval and rounding OR target value sequence
            if self.state.unpacked_target_value_sequence.is_empty() {
                // We don't have a target value sequence. Apply target interval and rounding.
                v = v.denormalize(
                    &self.settings.target_value_interval,
                    &self.settings.discrete_target_value_interval,
                    self.settings.use_discrete_processing,
                    control_type.discrete_max(),
                );
                if self.settings.round_target_value {
                    v = v.round(control_type);
                };
            } else {
                // We have a target value sequence. Apply it.
                let max_index = self.state.unpacked_target_value_sequence.len() - 1;
                let seq_index = (v.to_unit_value().get() * max_index as f64).round() as usize;
                let unit_value = self
                    .state
                    .unpacked_target_value_sequence
                    .get(seq_index)
                    .copied()
                    .unwrap_or_default();
                v = AbsoluteValue::Continuous(unit_value)
            }
        }
        // Return
        Some(v)
    }

    fn hitting_target_considering_max_jump(
        &mut self,
        pepped_up_control_value: AbsoluteValue,
        current_target_value: Option<AbsoluteValue>,
        control_type: ControlType,
        source_normalized_control_event: ControlEvent<AbsoluteValue, S>,
        prev_source_normalized_control_event: Option<ControlEvent<AbsoluteValue, S>>,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        let current_target_value = match current_target_value {
            // No target value available ... just deliver! Virtual targets take this shortcut.
            None => {
                let final_absolute_value =
                    self.get_final_absolute_value(pepped_up_control_value, control_type);
                return Some(ModeControlResult::hit_target(final_absolute_value));
            }
            Some(v) => v,
        };
        if (!self.settings.use_discrete_processing || pepped_up_control_value.is_continuous())
            && self.settings.jump_interval.is_full()
        {
            // No jump restrictions whatsoever
            return self.hit_if_changed(
                pepped_up_control_value,
                current_target_value,
                control_type,
            );
        }
        let distance = if self.settings.use_discrete_processing {
            pepped_up_control_value.calc_distance_from(current_target_value)
        } else {
            pepped_up_control_value.calc_distance_from(current_target_value.to_continuous_value())
        };
        if distance.is_greater_than(
            self.settings.jump_interval.max_val(),
            self.settings.discrete_jump_interval.max_val(),
        ) {
            // Distance is too large
            use TakeoverMode::*;
            return match self.settings.takeover_mode {
                Pickup => {
                    // Scaling not desired. Do nothing.
                    None
                }
                Parallel => {
                    // TODO-high-discrete Implement advanced takeover modes for discrete values, too
                    if let Some(prev) = prev_source_normalized_control_event {
                        let relative_increment =
                            source_normalized_control_event.payload().to_unit_value()
                                - prev.payload().to_unit_value();
                        if relative_increment == 0.0 {
                            None
                        } else {
                            let relative_increment = UnitIncrement::new_clamped(relative_increment);
                            let restrained_increment = relative_increment
                                .clamp_to_interval(&self.settings.jump_interval)?;
                            let final_target_value =
                                current_target_value.to_unit_value().add_clamping(
                                    restrained_increment,
                                    &self.settings.target_value_interval,
                                    BASE_EPSILON,
                                );
                            self.hit_if_changed(
                                AbsoluteValue::Continuous(final_target_value),
                                current_target_value,
                                control_type,
                            )
                        }
                    } else {
                        // We can't know the direction if we don't have a previous value.
                        // Wait for next incoming value.
                        None
                    }
                }
                LongTimeNoSee => {
                    let approach_distance = distance.denormalize(
                        &self.settings.jump_interval,
                        &self.settings.discrete_jump_interval,
                        self.settings.use_discrete_processing,
                        control_type.discrete_max(),
                    );
                    let approach_increment =
                        approach_distance.to_unit_value().to_increment(negative_if(
                            pepped_up_control_value.to_unit_value()
                                < current_target_value.to_unit_value(),
                        ))?;
                    let final_target_value = current_target_value.to_unit_value().add_clamping(
                        approach_increment,
                        &self.settings.target_value_interval,
                        BASE_EPSILON,
                    );
                    self.hit_if_changed(
                        AbsoluteValue::Continuous(final_target_value),
                        current_target_value,
                        control_type,
                    )
                }
                CatchUp => {
                    if let Some(prev) = prev_source_normalized_control_event {
                        let prev = prev.payload().to_unit_value();
                        let relative_increment =
                            source_normalized_control_event.payload().to_unit_value() - prev;
                        if relative_increment == 0.0 {
                            None
                        } else {
                            let goes_up = relative_increment.is_sign_positive();
                            // We already normalized the prev/current control values on the source
                            // interval, so we can use 0.0..=1.0 at this point.
                            let source_distance_from_bound = if goes_up {
                                1.0 - prev.get()
                            } else {
                                prev.get()
                            };
                            let current_target_value = current_target_value.to_unit_value();
                            let target_distance_from_bound = if goes_up {
                                self.settings.target_value_interval.max_val() - current_target_value
                            } else {
                                current_target_value - self.settings.target_value_interval.min_val()
                            }
                            .max(0.0);
                            if source_distance_from_bound == 0.0
                                || target_distance_from_bound == 0.0
                            {
                                None
                            } else {
                                // => -55484347409216.99
                                let scaled_increment = relative_increment
                                    * target_distance_from_bound
                                    / source_distance_from_bound;
                                let scaled_increment = UnitIncrement::new_clamped(scaled_increment);
                                let restrained_increment = scaled_increment
                                    .clamp_to_interval(&self.settings.jump_interval)?;
                                let final_target_value = current_target_value.add_clamping(
                                    restrained_increment,
                                    &self.settings.target_value_interval,
                                    BASE_EPSILON,
                                );
                                self.hit_if_changed(
                                    AbsoluteValue::Continuous(final_target_value),
                                    AbsoluteValue::Continuous(current_target_value),
                                    control_type,
                                )
                            }
                        }
                    } else {
                        // We can't know the direction if we don't have a previous value.
                        // Wait for next incoming value.
                        None
                    }
                }
            };
        }
        // Distance is not too large
        if distance.is_lower_than(
            self.settings.jump_interval.min_val(),
            self.settings.discrete_jump_interval.min_val(),
        ) {
            return None;
        }
        // Distance is also not too small
        self.hit_if_changed(pepped_up_control_value, current_target_value, control_type)
    }

    fn hit_if_changed(
        &self,
        desired_target_value: AbsoluteValue,
        current_target_value: AbsoluteValue,
        control_type: ControlType,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        if !control_type.is_retriggerable()
            && current_target_value.has_same_effect_as(desired_target_value)
        {
            return Some(ModeControlResult::LeaveTargetUntouched(
                desired_target_value,
            ));
        }
        let final_value = self.get_final_absolute_value(desired_target_value, control_type);
        Some(ModeControlResult::hit_target(final_value))
    }

    /// Use this only if the given desired target value could be a discrete value.
    fn get_final_absolute_value(
        &self,
        desired_target_value: AbsoluteValue,
        control_type: ControlType,
    ) -> AbsoluteValue {
        if self.settings.use_discrete_processing || control_type.is_virtual() {
            desired_target_value
        } else {
            // If discrete processing is not explicitly enabled, we must NOT send discrete values to
            // a real target! This is not just for backward compatibility. It would change how
            // discrete targets react in a surprising way (discrete behavior without having discrete
            // processing enabled). The reason why we don't "go continuous" right at the start of
            // the processing in this case is that we also have mappings with virtual targets. It's
            // important that they don't destroy the discreteness of a value, otherwise existing
            // controller presets which don't have "discrete processing" enabled would not be
            // compatible with main mappings that have discrete targets and want to use discrete
            // processing. There would have been other ways to deal with this (e.g. migration) but
            // the concept of letting a discrete value survive as long as possible (= not turning
            // it into a continuous one and thereby losing information) sounds like a good idea in
            // general.
            AbsoluteValue::Continuous(desired_target_value.to_unit_value())
        }
    }

    /// Takes care of:
    ///
    /// - Applying increment (the target step size is only looked at for discrete increments)
    /// - Wrap (rotate)
    fn hit_discrete_target_absolutely(
        &mut self,
        increment: Increment,
        target_step_size: UnitValue,
        options: ModeControlOptions,
        control_type: ControlType,
        current_value: impl Fn() -> Option<AbsoluteValue>,
    ) -> Option<ModeControlResult<ControlValue>> {
        if self.settings.use_discrete_processing {
            // Discrete processing for discrete target. Good!
            match current_value()? {
                AbsoluteValue::Continuous(_) => {
                    // But target reports continuous value!? Shouldn't happen. Whatever, fall back
                    // to continuous processing.
                    self.hit_target_absolutely_with_unit_increment(
                        increment.to_unit_increment(target_step_size)?,
                        // In order to not end up on "in-between" values, we should snap the target
                        // interval into the conceptual grid defined by the target step size.
                        Some(target_step_size),
                        current_value()?.to_unit_value(),
                        options,
                    )
                }
                AbsoluteValue::Discrete(f) => {
                    let discrete_increment = increment.to_discrete_increment();
                    self.hit_target_absolutely_with_discrete_increment(
                        discrete_increment,
                        f,
                        options,
                        control_type,
                    )
                }
            }
        } else {
            // Continuous processing although target is discrete. Kept for backward compatibility.
            self.hit_target_absolutely_with_unit_increment(
                increment.to_unit_increment(target_step_size)?,
                // In order to not end up on "in-between" values, we should snap the target
                // interval into the conceptual grid defined by the target step size.
                Some(target_step_size),
                current_value()?.to_unit_value(),
                options,
            )
        }
    }

    /// Takes care of:
    /// - Snap target interval to grid if desired (if `grid_interval_size` is given)
    /// - Applying increment
    /// - Wrap (rotate)
    fn hit_target_absolutely_with_unit_increment(
        &mut self,
        increment: UnitIncrement,
        grid_interval_size: Option<UnitValue>,
        current_target_value: UnitValue,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        let mut v = current_target_value;
        let mut target_value_interval = self.settings.target_value_interval;
        // If the target is discrete, it's important to make sure that the target interval
        // bounds correspond exactly to discrete target values. So we snap the target interval to
        // the target step size first.
        if let Some(grid_interval_size) = grid_interval_size {
            target_value_interval = Interval::new(
                self.settings
                    .target_value_interval
                    .min_val()
                    .snap_to_grid_by_interval_size(grid_interval_size),
                self.settings
                    .target_value_interval
                    .max_val()
                    .snap_to_grid_by_interval_size(grid_interval_size),
            );
            // The add functions don't add anything if the current target value is not within the target
            // interval in the first place. Instead they return one of the interval bounds. One issue
            // that might occur is that the current target value only *appears* out-of-range
            // because of numerical inaccuracies. That could lead to frustrating "it doesn't move"
            // experiences. Therefore we snap the current target value to grid first in that case.
            if !v.is_within_interval(&target_value_interval) {
                v = v.snap_to_grid_by_interval_size(grid_interval_size)
            };
        }
        v = if options.enforce_rotate || self.settings.rotate {
            v.add_rotating(increment, &target_value_interval, BASE_EPSILON)
        } else {
            v.add_clamping(increment, &target_value_interval, BASE_EPSILON)
        };
        if v == current_target_value {
            // Desired value is equal to current target value. No reason to hit the target.
            return Some(ModeControlResult::LeaveTargetUntouched(
                ControlValue::AbsoluteContinuous(v),
            ));
        }
        Some(ModeControlResult::HitTarget {
            value: ControlValue::AbsoluteContinuous(v),
        })
    }

    fn hit_target_absolutely_with_discrete_increment(
        &self,
        increment: DiscreteIncrement,
        current_target_value: Fraction,
        options: ModeControlOptions,
        control_type: ControlType,
    ) -> Option<ModeControlResult<ControlValue>> {
        let mut v = current_target_value;
        v = if options.enforce_rotate || self.settings.rotate {
            v.add_rotating(increment, &self.settings.discrete_target_value_interval)
        } else {
            v.add_clamping(increment, &self.settings.discrete_target_value_interval)
        };
        if let Some(target_max) = control_type.discrete_max() {
            v = v.with_max_clamped(target_max);
        }
        if v.actual() == current_target_value.actual() {
            return Some(ModeControlResult::LeaveTargetUntouched(
                ControlValue::AbsoluteDiscrete(v),
            ));
        }
        let final_absolute_value =
            self.get_final_absolute_value(AbsoluteValue::Discrete(v), control_type);
        Some(ModeControlResult::hit_target(ControlValue::from_absolute(
            final_absolute_value,
        )))
    }

    /// Takes care of:
    ///
    /// - Reverse
    /// - Speed (only for discrete increments)
    fn pep_up_increment(&mut self, increment: Increment) -> Option<Increment> {
        match increment {
            Increment::Continuous(i) => self
                .pep_up_continuous_increment(i)
                .map(Increment::Continuous),
            Increment::Discrete(i) => self.pep_up_discrete_increment(i).map(Increment::Discrete),
        }
    }

    /// Takes care of:
    ///
    /// - Reverse
    fn pep_up_continuous_increment(&mut self, increment: UnitIncrement) -> Option<UnitIncrement> {
        let result = if self.settings.reverse {
            increment.inverse()
        } else {
            increment
        };
        Some(result)
    }

    /// Takes care of:
    ///
    /// - Speed
    /// - Reverse
    fn pep_up_discrete_increment(
        &mut self,
        original_inc: DiscreteIncrement,
    ) -> Option<DiscreteIncrement> {
        let mut inc = original_inc;
        // Process speed (step count)
        let factor = inc.clamp_to_interval(&self.settings.step_factor_interval);
        inc = if factor.is_positive() {
            factor
        } else {
            let nth = factor.get().abs() as u32;
            let (fire, new_counter_value) = self.its_time_to_fire(nth, inc.signum());
            self.state.increment_counter = new_counter_value;
            if !fire {
                return None;
            }
            DiscreteIncrement::new(1)
        };
        inc = inc.with_direction(original_inc.signum());
        // Process reverse
        if self.settings.reverse {
            inc = inc.inverse();
        }
        Some(inc)
    }

    /// `nth` stands for "fire every nth time". `direction_signum` is either +1 or -1.
    fn its_time_to_fire(&self, nth: u32, direction_signum: i32) -> (bool, i32) {
        if self.state.increment_counter == 0 {
            // Initial fire
            return (true, direction_signum);
        }
        let positive_increment_counter = self.state.increment_counter.abs() as u32;
        if positive_increment_counter >= nth {
            // After having waited for a few increments, fire again.
            return (true, direction_signum);
        }
        (false, self.state.increment_counter + direction_signum)
    }

    /// Takes care of:
    ///
    /// - Source interval normalization
    /// - Speed (step count)
    /// - Reverse
    fn convert_to_discrete_increment(
        &mut self,
        control_value: UnitValue,
    ) -> Option<DiscreteIncrement> {
        let factor = control_value
            .normalize(
                &self.settings.source_value_interval,
                MinIsMaxBehavior::PreferOne,
                BASE_EPSILON,
            )
            .denormalize_discrete_increment(&self.settings.step_factor_interval);
        // This mode supports positive increment only.
        let discrete_value = if factor.is_positive() {
            factor.to_value()
        } else {
            let nth = factor.get().abs() as u32;
            let (fire, new_counter_value) = self.its_time_to_fire(nth, 1);
            self.state.increment_counter = new_counter_value;
            if !fire {
                return None;
            }
            DiscreteValue::new(1)
        };
        discrete_value.to_increment(negative_if(self.settings.reverse))
    }
}

struct AbsolutePreProcessingResult<S: AbstractTimestamp> {
    control_event: ControlEvent<AbsoluteValue, S>,
    prev_control_event: Option<ControlEvent<AbsoluteValue, S>>,
}

pub fn default_step_size_interval() -> Interval<UnitValue> {
    // 0.01 has also been chosen as default maximum step size because most users probably
    // want to start easy, that is without using the "press harder = more increments"
    // respectively "dial harder = more increments" features. Activating them right from
    // the start by choosing a higher step size maximum could lead to surprising results
    // such as ugly parameters jumps, especially if the source is not suited for that.
    create_unit_value_interval(DEFAULT_STEP_SIZE, DEFAULT_STEP_SIZE)
}

pub fn default_step_count_interval() -> Interval<DiscreteIncrement> {
    // Same reasoning as with step size interval
    create_discrete_increment_interval(1, 1)
}

/// If something like this is returned from the mode, it already means that the source value
/// was not filtered out (e.g. because of button filter).
#[derive(Debug)]
pub enum ModeControlResult<T> {
    /// Target should be hit with the given value.
    HitTarget { value: T },
    /// Target is reached but already has the given desired value and is not retriggerable.
    /// It shouldn't be hit.
    LeaveTargetUntouched(T),
}

impl<T> ModeControlResult<T> {
    pub fn hit_target(value: T) -> Self {
        Self::HitTarget { value }
    }

    pub fn map<R>(self, f: impl FnOnce(T) -> R) -> ModeControlResult<R> {
        use ModeControlResult::*;
        match self {
            HitTarget { value } => HitTarget { value: f(value) },
            LeaveTargetUntouched(v) => LeaveTargetUntouched(f(v)),
        }
    }
}

impl<T: Display> Display for ModeControlResult<T> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            ModeControlResult::HitTarget { value } => {
                write!(f, "Hit target with value {}", value)
            }
            ModeControlResult::LeaveTargetUntouched(v) => {
                write!(f, "Leave target untouched with value {}", v)
            }
        }
    }
}

impl<T> From<ModeControlResult<T>> for Option<T> {
    fn from(res: ModeControlResult<T>) -> Self {
        use ModeControlResult::*;
        match res {
            LeaveTargetUntouched(_) => None,
            HitTarget { value, .. } => Some(value),
        }
    }
}

fn full_discrete_interval() -> Interval<u32> {
    Interval::new(0, u32::MAX)
}

fn textual_feedback_expression_regex() -> &'static regex::Regex {
    regex!(r#"\{\{ *([A-Za-z0-9._]+) *\}\}"#)
}

const DEFAULT_TEXTUAL_FEEDBACK_PROP_KEY: &str = "target.text_value";

#[cfg(test)]
mod tests {
    use super::*;

    use crate::mode::test_util::{TestTarget, TestTransformation};
    use crate::{create_unit_value_interval, ControlType, Fraction, NoopTimestamp};
    use approx::*;

    mod absolute_normal {
        use super::*;

        mod continuous_processing {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.3779527559055118)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_eq!(
                    mode.control(abs_con_evt(0.3779527559055118), &target, ()),
                    None
                );
                assert_eq!(mode.control(abs_dis_evt(48, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(63, 127), &target, ()).unwrap(),
                    abs_con_val(0.49606299212598426)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn default_with_virtual_target() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.3779527559055118)),
                    control_type: ControlType::VirtualMulti,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 127)
                );
                assert_eq!(
                    mode.control(abs_con_evt(0.3779527559055118), &target, ()),
                    None
                );
                assert_eq!(mode.control(abs_dis_evt(48, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(63, 127), &target, ()).unwrap(),
                    abs_dis_val(63, 127)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_dis_val(127, 127)
                );
            }

            #[test]
            fn default_target_is_trigger() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuousRetriggerable,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.777), &target, ()).unwrap(),
                    abs_con_val(0.777)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn relative_target() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: None,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn source_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(13, 127), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.2), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(25, 127), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(51, 127), &target, ()).unwrap(),
                    abs_con_val(0.5039370078740157)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(76, 127), &target, ()).unwrap(),
                    abs_con_val(0.9960629921259844)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.8), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(80, 127), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn source_interval_out_of_range_ignore() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.2), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert!(mode.control(abs_con_evt(0.8), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn source_interval_out_of_range_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.2), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.8), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            #[test]
            fn source_interval_out_of_range_ignore_source_one_value() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.4), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert!(mode.control(abs_con_evt(0.6), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn source_interval_out_of_range_min_source_one_value() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            #[test]
            fn source_interval_out_of_range_min_max_source_one_value() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::MinOrMax,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn target_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.6),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.2), &target, ()).unwrap(),
                    abs_con_val(0.28)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.25), &target, ()).unwrap(),
                    abs_con_val(0.3)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.75), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.6)
                );
            }

            #[test]
            fn target_interval_reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.6, 1.0),
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.25), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.75), &target, ()).unwrap(),
                    abs_con_val(0.7)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.6)
                );
            }

            #[test]
            fn source_and_target_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    target_value_interval: create_unit_value_interval(0.2, 0.6),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.2), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.8), &target, ()).unwrap(),
                    abs_con_val(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.6)
                );
            }

            #[test]
            fn source_and_target_interval_shifted() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    target_value_interval: create_unit_value_interval(0.4, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.2), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.8), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            #[test]
            fn reverse_discrete_target() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(55, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Check that we use a "scaling reverse" instead of a "subtracting" reverse even
                // though control value and target is discrete. Discrete processing is disabled!
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 10), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(5, 10), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(10, 10), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            #[test]
            fn discrete_target() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(4, 5)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.2),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_eq!(mode.control(abs_con_evt(0.8), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.19), &target, ()).unwrap(),
                    abs_con_val(0.19)
                );

                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.49), &target, ()).unwrap(),
                    abs_con_val(0.49)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn round() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    round_target_value: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(4, 5)),
                    control_type: ControlType::AbsoluteContinuousRoundable {
                        rounding_step_size: UnitValue::new(0.2),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.11), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.19), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.2), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.35), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.49), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn jump_interval_max_pickup() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.2),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.5)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.3), &target, ()).unwrap(),
                    abs_con_val(0.3)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.7), &target, ()).unwrap(),
                    abs_con_val(0.7)
                );
                assert!(mode.control(abs_con_evt(0.8), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.9), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn jump_interval_max_pickup_with_target_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.1),
                    target_value_interval: create_unit_value_interval(0.0, 0.5),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.1)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |i, o| {
                    abs_con_test(&mut mode, &target, i, o);
                };
                test(0.0, Some(0.00));
                test(0.1, Some(0.05));
                test(0.2, None);
                test(0.3, Some(0.15));
                test(0.4, Some(0.20));
                test(0.5, None);
                test(0.7, None);
                test(1.0, None);
            }

            #[test]
            fn jump_interval_max_pickup_with_target_interval_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.1),
                    target_value_interval: create_unit_value_interval(0.0, 0.5),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |i, o| {
                    abs_con_test(&mut mode, &target, i, o);
                };
                // Pickup mode is strict. If we never reach the actual value, we can't pick it up.
                test(0.0, None);
                test(0.1, None);
                test(0.2, None);
                test(0.3, None);
                test(0.4, None);
                test(0.5, None);
                test(0.7, None);
                test(1.0, None);
            }

            #[test]
            fn jump_interval_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.1, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.5)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.1)
                );
                assert!(mode.control(abs_con_evt(0.41), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.59), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn jump_interval_max_long_time_no_see() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.2),
                    takeover_mode: TakeoverMode::LongTimeNoSee,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.5)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |i, o| {
                    // Takeover mode "Long time no see" works without having to maintain a previous
                    // control value. So each assertion is independent and therefore we can
                    // intuitively test it without actually adjusting the current target value
                    // between each assertion.
                    abs_con_test(&mut mode, &target, i, o);
                };
                test(0.0, Some(0.4));
                test(0.1, Some(0.42));
                test(0.4, Some(0.4));
                test(0.6, Some(0.6));
                test(0.7, Some(0.7));
                test(0.8, Some(0.56));
                test(1.0, Some(0.6));
            }

            #[test]
            fn jump_interval_max_long_time_no_see_with_target_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.1),
                    target_value_interval: create_unit_value_interval(0.0, 0.5),
                    takeover_mode: TakeoverMode::LongTimeNoSee,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.1)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |i, o| {
                    abs_con_test(&mut mode, &target, i, o);
                };
                test(0.0, Some(0.00));
                test(0.1, Some(0.05));
                test(0.2, None);
                test(0.3, Some(0.15));
                test(0.4, Some(0.20));
                test(0.5, Some(0.115));
                test(0.6, Some(0.12));
                test(0.7, Some(0.125));
                test(0.8, Some(0.13));
                test(0.9, Some(0.135));
                test(1.0, Some(0.14));
            }

            #[test]
            fn jump_interval_max_long_time_no_see_with_target_interval_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.1),
                    target_value_interval: create_unit_value_interval(0.0, 0.5),
                    takeover_mode: TakeoverMode::LongTimeNoSee,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |i, o| {
                    abs_con_test(&mut mode, &target, i, o);
                };
                // That's a jump, but one that we allow because target value being out of range
                // is an exceptional situation.
                test(0.0, Some(0.5));
                test(0.1, Some(0.5));
                test(0.2, Some(0.5));
                test(0.3, Some(0.5));
                test(0.4, Some(0.5));
                test(0.5, Some(0.5));
                test(0.6, Some(0.5));
                test(0.7, Some(0.5));
                test(0.8, Some(0.5));
                test(0.9, Some(0.5));
                test(1.0, Some(0.5));
            }

            #[test]
            fn jump_interval_max_parallel() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.1),
                    takeover_mode: TakeoverMode::Parallel,
                    ..Default::default()
                });
                let mut target = TestTarget {
                    current_value: Some(con_val(0.1)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |i, o| {
                    // In order to intuitively test this takeover mode, we need to also adjust
                    // the current target value after each assertion.
                    abs_con_test_cumulative(&mut mode, &mut target, i, o);
                };
                // First one indeterminate
                test(0.6, None);
                // Raising in parallel
                test(0.7, Some(0.2));
                test(0.8, Some(0.3));
                test(0.85, Some(0.35));
                test(0.9, Some(0.4));
                test(1.0, Some(0.5));
                // Falling in parallel
                test(0.9, Some(0.4));
                test(0.8, Some(0.3));
                test(0.75, Some(0.25));
                test(0.7, Some(0.2));
                test(0.6, Some(0.1));
                test(0.5, Some(0.0));
                // Saturating
                test(0.4, None);
                test(0.3, None);
                test(0.4, Some(0.1));
                // Raising in parallel without exceeding max jump
                test(0.6, Some(0.2));
                test(0.7, Some(0.3));
                test(1.0, Some(0.4));
                // Falling in parallel without exceeding max jump
                test(0.6, Some(0.3));
            }

            #[test]
            fn jump_interval_max_parallel_with_target_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.1),
                    target_value_interval: create_unit_value_interval(0.0, 0.5),
                    takeover_mode: TakeoverMode::Parallel,
                    ..Default::default()
                });
                let mut target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |i, o| {
                    abs_con_test_cumulative(&mut mode, &mut target, i, o);
                };
                // First one indeterminate
                test(0.6, None);
                // Raising in parallel
                test(0.7, Some(0.1));
                test(0.8, Some(0.2));
                test(0.85, Some(0.25));
                test(0.9, Some(0.30));
                test(1.0, Some(0.4));
                // No jump
                test(0.9, Some(0.45));
                // Falling in parallel
                test(0.3, Some(0.35));
                test(0.2, Some(0.25));
                test(0.1, Some(0.15));
                test(0.0, Some(0.05));
                // No jump
                test(0.0, Some(0.00));
                // Saturating
                test(0.0, None);
            }

            #[test]
            fn jump_interval_max_catch_up() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.1),
                    takeover_mode: TakeoverMode::CatchUp,
                    ..Default::default()
                });
                let mut target = TestTarget {
                    current_value: Some(con_val(0.1)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |input: f64, output: Option<f64>| {
                    if let Some(o) = output {
                        assert_abs_diff_eq!(
                            mode.control(abs_con_evt(input), &target, ()).unwrap(),
                            abs_con_val(o)
                        );
                        // In order to intuitively test this takeover mode, we need to also adjust
                        // the current target value after each assertion.
                        target.current_value = Some(con_val(o));
                    } else {
                        assert_eq!(mode.control(abs_con_evt(input), &target, ()), None);
                    }
                };
                // First one indeterminate
                test(0.6, None);
                // Raising as fast as possible (= catching up) without exceeding max jump
                test(0.7, Some(0.2));
                test(0.8, Some(0.3));
                test(0.85, Some(0.4));
                test(0.9, Some(0.5));
                test(1.0, Some(0.6));
                // Falling slower than usually (= seeking convergence)
                test(0.9, Some(0.54));
                test(0.8, Some(0.48));
                test(0.75, Some(0.45));
                test(0.7, Some(0.42));
                test(0.6, Some(0.36));
                test(0.5, Some(0.30));
                // Mmh?
                test(0.0, Some(0.2));
                test(0.0, None);
                // Raising as fast as possible (= catching up) without exceeding max jump
                test(0.1, Some(0.1));
                test(0.3, Some(0.2));
                test(0.5, Some(0.3));
                // Falling and seeing that we already are at this value.
                test(0.3, None);
            }

            #[test]
            fn jump_interval_max_catch_up_corner_case() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.47, 0.53),
                    jump_interval: create_unit_value_interval(0.0, 0.02),
                    target_value_interval: create_unit_value_interval(0.0, 0.716),
                    takeover_mode: TakeoverMode::CatchUp,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.6897)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |input: f64| {
                    mode.control(abs_con_evt(input), &target, ());
                };
                // In older versions this panicked because of invalid unit values/increments
                test(0.0);
                test(0.1);
                test(0.2);
                test(0.3);
                test(0.4);
                test(0.5);
                test(0.6);
                test(0.7);
                test(0.8);
                test(0.9);
                test(1.0);
                test(0.9);
                test(0.8);
                test(0.7);
                test(0.6);
                test(0.5);
                test(0.4);
                test(0.3);
                test(0.2);
                test(0.1);
                test(0.0);
            }

            #[test]
            fn jump_interval_max_catch_up_with_target_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    jump_interval: create_unit_value_interval(0.0, 0.1),
                    target_value_interval: create_unit_value_interval(0.5, 1.0),
                    takeover_mode: TakeoverMode::CatchUp,
                    ..Default::default()
                });
                let mut target = TestTarget {
                    current_value: Some(con_val(0.1)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                let mut test = |input: f64, output: Option<f64>| {
                    if let Some(o) = output {
                        assert_abs_diff_eq!(
                            mode.control(abs_con_evt(input), &target, ()).unwrap(),
                            abs_con_val(o)
                        );
                        // In order to intuitively test this takeover mode, we need to also adjust
                        // the current target value after each assertion.
                        target.current_value = Some(con_val(o));
                    } else {
                        assert_eq!(mode.control(abs_con_evt(input), &target, ()), None);
                    }
                };
                // First one indeterminate
                test(0.6, None);
                // Raising as fast as possible (= catching up) without exceeding max jump
                test(0.7, Some(0.5));
                test(0.8, Some(0.6));
                test(0.85, Some(0.7));
                test(0.9, Some(0.8));
                test(1.0, Some(0.9));
                // No jump detected. Interesting case. TODO-medium This is debatable. Value raise
                //  when fader turned down is something we wouldn't expect with this takeover
                //  mode. It's only the moment though when fader and value get in sync again. It
                //  snaps back, so it probably doesn't hurt and is barely noticeable.
                test(0.9, Some(0.95));
                // Falling as fast as possible (= catching up)
                test(0.5, Some(0.85));
                // Converging
                test(0.4, Some(0.78));
            }

            #[test]
            fn transformation_ok() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    control_transformation: Some(TestTransformation::new(|input| Ok(1.0 - input))),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            #[test]
            fn transformation_err() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    control_transformation: Some(TestTransformation::new(|_| Err("oh no!"))),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            // TODO-medium-discrete Add tests for discrete processing
            #[test]
            fn target_value_sequence_continuous_target() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_sequence: "0.2, 0.4, 0.4, 0.5, 0.0, 0.9".parse().unwrap(),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.6)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                mode.update_from_target(&target, ());
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 20), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.09), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(1, 20), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(3, 20), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.2), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(4, 20), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(8, 20), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(12, 20), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.65), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(13, 20), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.8), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(16, 20), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(20, 20), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
            }

            /// Discrete steps dictated by target itself should be ignored when a sequence is given.
            #[test]
            fn target_value_sequence_discrete_target() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_sequence: "0.2, 0.4, 0.4, 0.5, 0.0, 0.9".parse().unwrap(),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.6)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.5),
                        is_retriggerable: false,
                    },
                };
                mode.update_from_target(&target, ());
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 20), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.09), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(1, 20), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(3, 20), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.2), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(4, 20), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.4), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(8, 20), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.6), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(12, 20), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.65), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(13, 20), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.8), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(16, 20), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(20, 20), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
            }

            #[test]
            fn target_value_sequence_continuous_target_range() {
                // Given
                let target_value_sequence: ValueSequence =
                    "0.25 - 0.50 (0.01), 0.75, 0.50, 0.10".parse().unwrap();
                let count = target_value_sequence.unpack(UnitValue::new(0.01)).len();
                let max = count - 1;
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_sequence,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.6)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                mode.update_from_target(&target, ());
                // When
                // Then
                assert_eq!(29, count);
                assert_eq!(28, max);
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.0), &target, ()).unwrap(),
                    abs_con_val(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(25.0 / max as f64), &target, ())
                        .unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.10)
                );
            }

            #[test]
            fn feedback() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(dis_val(0, 127)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(
                    mode.feedback(dis_val(60, 127)).unwrap(),
                    con_val(0.47244094488188976)
                );
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(dis_val(127, 127)).unwrap(), con_val(1.0));
            }

            #[test]
            fn feedback_with_virtual_source() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                let options = ModeFeedbackOptions {
                    source_is_virtual: true,
                    max_discrete_source_value: None,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.feedback_with_options(con_val(0.0), options).unwrap(),
                    con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(dis_val(0, 127), options)
                        .unwrap(),
                    dis_val(0, 127)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(con_val(0.5), options).unwrap(),
                    con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(dis_val(60, 127), options)
                        .unwrap(),
                    dis_val(60, 127)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(con_val(1.0), options).unwrap(),
                    con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(dis_val(127, 127), options)
                        .unwrap(),
                    dis_val(127, 127)
                );
            }

            #[test]
            fn feedback_reverse() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    reverse: true,
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_target_interval() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.2)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.25));
                assert_abs_diff_eq!(mode.feedback(con_val(0.6)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(0.8)).unwrap(), con_val(0.75));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
            }

            #[test]
            fn feedback_target_interval_reverse() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 1.0),
                    reverse: true,
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.2)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.75));
                assert_abs_diff_eq!(mode.feedback(con_val(0.6)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(0.8)).unwrap(), con_val(0.25));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_source_and_target_interval() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    source_value_interval: create_unit_value_interval(0.2, 0.8),
                    target_value_interval: create_unit_value_interval(0.4, 1.0),
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.2));
                assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.2));
                assert_abs_diff_eq!(mode.feedback(con_val(0.7)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.8));
            }

            #[test]
            fn feedback_out_of_range_ignore() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                });
                // When
                // Then
                assert!(mode.feedback(con_val(0.0)).is_none());
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert!(mode.feedback(con_val(1.0)).is_none());
            }

            #[test]
            fn feedback_out_of_range_min() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.1)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(0.9)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_max_okay() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.02, 0.02),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.01)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.02)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.03)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_max_issue_263() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.03, 0.03),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.01)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.03)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.04)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_max_issue_263_more() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.03, 0.03),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.01)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(
                    mode.feedback(con_val(0.029999999329447746)).unwrap(),
                    con_val(1.0)
                );
                assert_abs_diff_eq!(mode.feedback(con_val(0.0300000001)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.04)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_target_one_value() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.1)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.9)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_max_target_one_value() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.5, 0.5),
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.1)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.9)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
            }

            #[test]
            fn feedback_out_of_range_ignore_target_one_value() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                });
                // When
                // Then
                assert!(mode.feedback(con_val(0.0)).is_none());
                assert!(mode.feedback(con_val(0.1)).is_none());
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(1.0));
                assert!(mode.feedback(con_val(0.9)).is_none());
                assert!(mode.feedback(con_val(1.0)).is_none());
            }

            #[test]
            fn feedback_transformation() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    feedback_transformation: Some(TestTransformation::new(|input| Ok(1.0 - input))),
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }
        }

        mod discrete_processing {
            use super::*;

            #[test]
            fn case_1_no_interval_restriction() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(mode.control(abs_dis_evt(48, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(48, 200), FB_OPTS),
                    Some(dis_val(48, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(50, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn case_2_target_interval_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(mode.control(abs_dis_evt(48, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(98, 200), FB_OPTS),
                    Some(dis_val(48, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(50, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(150, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn case_3_source_interval_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(50, 100), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(mode.control(abs_dis_evt(98, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(48, 200), FB_OPTS),
                    Some(dis_val(98, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn case_4_source_and_target_interval_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(50, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(mode.control(abs_dis_evt(98, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(98, 200), FB_OPTS),
                    Some(dis_val(98, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn case_5_source_and_target_interval_min_max() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(0, 50),
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(mode.control(abs_dis_evt(48, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(98, 200), FB_OPTS),
                    Some(dis_val(48, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(50, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(mode.control(abs_dis_evt(48, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(98, 200), FB_OPTS),
                    Some(dis_val(48, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
            }

            #[test]
            fn case_6_source_and_target_interval_disjoint() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    discrete_target_value_interval: Interval::new(200, u32::MAX),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(248, 350)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 350.0),
                        is_retriggerable: false,
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(50, 150), &target, ()).unwrap(),
                    abs_dis_val(200, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(mode.control(abs_dis_evt(98, 150), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(248, 350), fb_opts),
                    Some(dis_val(98, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 150), &target, ()).unwrap(),
                    abs_dis_val(250, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(250, 350), fb_opts),
                    Some(dis_val(100, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(150, 150), &target, ()).unwrap(),
                    abs_dis_val(300, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(300, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 150), &target, ()).unwrap(),
                    abs_dis_val(200, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(350, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
            }

            #[test]
            fn case_7_interval_max_greater_than_target_max() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 200),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(98, 150)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 150.0),
                        is_retriggerable: false,
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 150), &target, ()).unwrap(),
                    abs_dis_val(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(50, 150), &target, ()).unwrap(),
                    abs_dis_val(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 150), &target, ()).unwrap(),
                    abs_dis_val(150, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 150), fb_opts),
                    Some(dis_val(100, 150))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(150, 150), &target, ()).unwrap(),
                    abs_dis_val(150, 150)
                );
            }

            #[test]
            fn case_8_target_subset_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(98, 150)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 150.0),
                        is_retriggerable: false,
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 150), &target, ()).unwrap(),
                    abs_dis_val(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(50, 150), &target, ()).unwrap(),
                    abs_dis_val(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 150), &target, ()).unwrap(),
                    abs_dis_val(100, 150)
                );
                assert_eq!(
                    mode.control(abs_dis_evt(150, 150), &target, ()).unwrap(),
                    abs_dis_val(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
            }

            #[test]
            fn case_9_no_interval_restriction_reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(50, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
            }

            #[test]
            fn case_10_target_interval_min_reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(150, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(50, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
            }

            #[test]
            fn case_11_source_interval_min_reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(50, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
            }

            #[test]
            fn case_12_source_and_target_interval_min_reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(50, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
            }

            #[test]
            fn case_13_source_and_target_interval_disjoint_reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    discrete_target_value_interval: Interval::new(200, u32::MAX),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(248, 350)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 350.0),
                        is_retriggerable: false,
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(50, 150), &target, ()).unwrap(),
                    abs_dis_val(300, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(300, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 150), &target, ()).unwrap(),
                    abs_dis_val(250, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(250, 350), fb_opts),
                    Some(dis_val(100, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(150, 150), &target, ()).unwrap(),
                    abs_dis_val(200, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 150), &target, ()).unwrap(),
                    abs_dis_val(300, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(350, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
            }

            #[test]
            fn case_14_interval_max_greater_than_target_max_reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_target_value_interval: Interval::new(50, 200),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(98, 150)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 150.0),
                        is_retriggerable: false,
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 150), &target, ()).unwrap(),
                    abs_dis_val(150, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(50, 150), &target, ()).unwrap(),
                    abs_dis_val(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 150), &target, ()).unwrap(),
                    abs_dis_val(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 150), fb_opts),
                    Some(dis_val(100, 150))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 150), fb_opts),
                    Some(dis_val(100, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(150, 150), &target, ()).unwrap(),
                    abs_dis_val(50, 150)
                );
            }

            #[test]
            fn case_15_target_subset_interval_reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_target_value_interval: Interval::new(50, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(98, 150)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 150.0),
                        is_retriggerable: false,
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis_evt(0, 150), &target, ()).unwrap(),
                    abs_dis_val(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(50, 150), &target, ()).unwrap(),
                    abs_dis_val(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 150), &target, ()).unwrap(),
                    abs_dis_val(50, 150)
                );
                assert_eq!(
                    mode.control(abs_dis_evt(150, 150), &target, ()).unwrap(),
                    abs_dis_val(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
            }

            #[test]
            fn default_with_virtual_target() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: None,
                    control_type: ControlType::VirtualMulti,
                };
                // When
                // Then
                assert_eq!(
                    mode.control(abs_dis_evt(0, 100), &target, ()).unwrap(),
                    abs_dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(63, 100), &target, ()).unwrap(),
                    abs_dis_val(63, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(63, 100), FB_OPTS),
                    Some(dis_val(63, 100))
                );
                assert_eq!(
                    mode.control(abs_dis_evt(100, 100), &target, ()).unwrap(),
                    abs_dis_val(100, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn relative_target() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: None,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 127)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(63, 127), &target, ()).unwrap(),
                    abs_dis_val(63, 127)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_dis_val(127, 127)
                );
            }

            #[test]
            fn source_interval_out_of_range_ignore() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(20, 60),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_dis_evt(0, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis_evt(19, 127), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(20, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(40, 127), &target, ()).unwrap(),
                    abs_dis_val(20, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(60, 127), &target, ()).unwrap(),
                    abs_dis_val(40, 200)
                );
                assert!(mode.control(abs_dis_evt(70, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis_evt(100, 127), &target, ()).is_none());
            }

            #[test]
            fn source_interval_out_of_range_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(20, 60),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(1, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(20, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(40, 127), &target, ()).unwrap(),
                    abs_dis_val(20, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(60, 127), &target, ()).unwrap(),
                    abs_dis_val(40, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(90, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
            }

            #[test]
            fn source_interval_out_of_range_ignore_source_one_value() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(60, 60),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_dis_evt(0, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis_evt(59, 127), &target, ()).is_none());
                // TODO-high-discrete Not sure if this abs_dis(1, 1) is serving the actual use case
                //  here and in following tests...
                assert_eq!(
                    mode.control(abs_dis_evt(60, 127), &target, ()).unwrap(),
                    abs_dis_val(1, 200)
                );
                assert!(mode.control(abs_dis_evt(61, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis_evt(127, 127), &target, ()).is_none());
            }

            #[test]
            fn source_interval_out_of_range_min_source_one_value() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(60, 60),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(59, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(60, 127), &target, ()).unwrap(),
                    abs_dis_val(1, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(61, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(61, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
            }

            #[test]
            fn source_interval_out_of_range_min_max_source_one_value() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(60, 60),
                    out_of_range_behavior: OutOfRangeBehavior::MinOrMax,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(59, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(60, 127), &target, ()).unwrap(),
                    abs_dis_val(1, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(61, 127), &target, ()).unwrap(),
                    abs_dis_val(1, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_dis_val(1, 200)
                );
            }

            #[test]
            fn target_interval_reverse() {
                // TODO-high-discrete Add other reverse tests with source and target interval and
                //  also intervals with max values! First for continuous!
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(70, 100),
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(1, 127), &target, ()).unwrap(),
                    abs_dis_val(99, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(15, 127), &target, ()).unwrap(),
                    abs_dis_val(85, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(30, 100), &target, ()).unwrap(),
                    abs_dis_val(70, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(40, 100), &target, ()).unwrap(),
                    abs_dis_val(70, 200)
                );
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(127, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(63, 127), &target, ()).unwrap(),
                    abs_dis_val(127 - 63, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
            }

            #[test]
            fn round() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    round_target_value: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(2, 5)),
                    control_type: ControlType::AbsoluteContinuousRoundable {
                        rounding_step_size: UnitValue::new(0.2),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 5)
                );
                assert_eq!(mode.control(abs_dis_evt(2, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(3, 127), &target, ()).unwrap(),
                    abs_dis_val(3, 5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_dis_val(5, 5)
                );
            }

            #[test]
            fn jump_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_jump_interval: Interval::new(0, 2),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(60, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_eq!(mode.control(abs_dis_evt(0, 127), &target, ()), None);
                assert_eq!(mode.control(abs_dis_evt(57, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(58, 127), &target, ()).unwrap(),
                    abs_dis_val(58, 200)
                );
                assert_eq!(mode.control(abs_dis_evt(60, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(61, 127), &target, ()).unwrap(),
                    abs_dis_val(61, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(62, 127), &target, ()).unwrap(),
                    abs_dis_val(62, 200)
                );
                assert!(mode.control(abs_dis_evt(63, 200), &target, ()).is_none());
                assert!(mode.control(abs_dis_evt(127, 200), &target, ()).is_none());
            }

            #[test]
            fn jump_interval_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_jump_interval: Interval::new(10, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(60, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(1, 127), &target, ()).unwrap(),
                    abs_dis_val(1, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(50, 127), &target, ()).unwrap(),
                    abs_dis_val(50, 200)
                );
                assert!(mode.control(abs_dis_evt(55, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis_evt(65, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis_evt(69, 127), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(70, 127), &target, ()).unwrap(),
                    abs_dis_val(70, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_dis_val(127, 200)
                );
            }

            // #[test]
            // fn jump_interval_approach() {
            //     // Given
            //     let mut mode: TestMode = Mode::new(ModeSettings {
            //         jump_interval: create_unit_value_interval(0.0, 0.2),
            //         takeover_mode: TakeoverMode::LongTimeNoSee,
            //         ..Default::default()
            //     });
            //     let target = TestTarget {
            //         current_value: Some(continuous_value(0.5)),
            //         control_type: ControlType::AbsoluteContinuous,
            //     };
            //     // When
            //     // Then
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.0), &target, ()).unwrap(),
            //         abs_con(0.4)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.1), &target, ()).unwrap(),
            //         abs_con(0.42)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.4), &target, ()).unwrap(),
            //         abs_con(0.4)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.6), &target, ()).unwrap(),
            //         abs_con(0.6)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.7), &target, ()).unwrap(),
            //         abs_con(0.7)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.8), &target, ()).unwrap(),
            //         abs_con(0.56)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(1.0), &target, ()).unwrap(),
            //         abs_con(0.6)
            //     );
            // }

            #[test]
            fn transformation_ok() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    control_transformation: Some(TestTransformation::new(|input| Ok(input + 20.0))),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(20, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(60, 127), &target, ()).unwrap(),
                    abs_dis_val(80, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(120, 127), &target, ()).unwrap(),
                    abs_dis_val(140, 200)
                );
            }

            #[test]
            fn transformation_negative() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    control_transformation: Some(TestTransformation::new(|input| Ok(input - 20.0))),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(20, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(120, 127), &target, ()).unwrap(),
                    abs_dis_val(100, 200)
                );
            }

            #[test]
            fn transformation_err() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    control_transformation: Some(TestTransformation::new(|_| Err("oh no!"))),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(0, 127), &target, ()).unwrap(),
                    abs_dis_val(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(60, 127), &target, ()).unwrap(),
                    abs_dis_val(60, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis_evt(127, 127), &target, ()).unwrap(),
                    abs_dis_val(127, 200)
                );
            }

            #[test]
            fn feedback() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 10), FB_OPTS).unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(5, 10), FB_OPTS).unwrap(),
                    dis_val(5, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 10), FB_OPTS)
                        .unwrap(),
                    dis_val(10, 100)
                );
            }

            #[test]
            fn feedback_with_virtual_source() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    ..Default::default()
                });
                let options = ModeFeedbackOptions {
                    source_is_virtual: true,
                    max_discrete_source_value: None,
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 10), options).unwrap(),
                    dis_val(0, 10)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(5, 10), options).unwrap(),
                    dis_val(5, 10)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 10), options)
                        .unwrap(),
                    dis_val(10, 10)
                );
            }

            #[test]
            fn feedback_reverse() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    reverse: true,
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(100, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(5, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 5, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 10, 100)
                );
            }

            #[test]
            fn feedback_target_interval() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(20, u32::MAX),
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(20, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(40, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(20, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(60, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(40, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(80, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(60, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(80, 100)
                );
            }

            #[test]
            fn feedback_target_interval_reverse() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(20, u32::MAX),
                    reverse: true,
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(20, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(40, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 20, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(60, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 40, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(80, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 60, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 80, 100)
                );
            }

            #[test]
            fn feedback_source_and_target_interval() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(20, 80),
                    discrete_target_value_interval: Interval::new(40, 100),
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(20, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(40, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(20, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(70, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(50, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(80, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_ignore() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(20, 80),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(mode.feedback_with_options(dis_val(0, 100), FB_OPTS), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(30, 100)
                );
                assert_eq!(mode.feedback_with_options(dis_val(100, 100), FB_OPTS), None);
            }

            #[test]
            fn feedback_out_of_range_min() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(20, 80),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(30, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(90, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_min_max_okay() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(2, 2),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(1, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                // TODO-high-discrete Would make more sense to use SOURCE MAX instead of 1. So this
                //  (1, 1) thing is not that useful! We should have a way to express MAX - which is
                //  u32:MAX. It should then be clamped to
                //  min(source_interval_max, discrete_source_max).
                assert_eq!(
                    mode.feedback_with_options(dis_val(2, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(3, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_min_max_issue_263() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(3, 3),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(1, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(2, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(3, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(4, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_min_target_one_value() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 50),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(90, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_min_max_target_one_value() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 50),
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(90, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_ignore_target_one_value() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 50),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(mode.feedback_with_options(dis_val(0, 100), FB_OPTS), None);
                assert_eq!(mode.feedback_with_options(dis_val(10, 100), FB_OPTS), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(mode.feedback_with_options(dis_val(90, 100), FB_OPTS), None);
                assert_eq!(mode.feedback_with_options(dis_val(100, 100), FB_OPTS), None);
            }

            #[test]
            fn feedback_transformation() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    use_discrete_processing: true,
                    feedback_transformation: Some(TestTransformation::new(
                        |input| Ok(input - 10.0),
                    )),
                    ..Default::default()
                });
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(00, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(40, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(90, 100)
                );
            }
        }
    }

    mod absolute_toggle {
        use super::*;

        #[test]
        fn absolute_value_target_off() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(1.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(1.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(1.0)
            );
        }

        #[test]
        fn absolute_value_target_on() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(1.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(0.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(0.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(0.0)
            );
        }

        #[test]
        fn absolute_value_target_rather_off() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.333)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(1.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(1.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(1.0)
            );
        }

        #[test]
        fn absolute_value_target_rather_on() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.777)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(0.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(0.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(0.0)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_off() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.3)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(0.7)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_on() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.7)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(0.3)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_rather_off() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.4)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(0.7)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_rather_on() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.6)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(0.3)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_too_off() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(0.7)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_too_on() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(1.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                abs_con_val(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                abs_con_val(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                abs_con_val(0.3)
            );
        }

        #[test]
        fn feedback() {
            // Given
            let mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                ..Default::default()
            });
            // When
            // Then
            assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
            assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
            assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
        }

        #[test]
        fn feedback_target_interval() {
            // Given
            let mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::ToggleButton,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            });
            // When
            // Then
            assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
            assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.25));
            assert_abs_diff_eq!(mode.feedback(con_val(0.7)).unwrap(), con_val(1.0));
            assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
        }
    }

    mod make_relative {
        use super::*;

        #[test]
        fn continuous_to_continuous_equal() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::MakeRelative,
                ..Default::default()
            });
            let mut target = TestTarget {
                current_value: Some(con_val(0.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            let mut test = |i, o| {
                abs_con_test_cumulative(&mut mode, &mut target, i, o);
            };
            test(0.0, None);
            test(0.1, Some(0.1));
            test(0.4, Some(0.4));
            test(0.5, Some(0.5));
            test(1.0, Some(1.0));
        }

        #[test]
        fn continuous_to_continuous_shifted() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::MakeRelative,
                ..Default::default()
            });
            let mut target = TestTarget {
                current_value: Some(con_val(0.4)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            let mut test = |i, o| {
                abs_con_test_cumulative(&mut mode, &mut target, i, o);
            };
            test(0.0, None);
            test(0.1, Some(0.5));
            test(0.2, Some(0.6));
            test(0.4, Some(0.8));
            test(0.5, Some(0.9));
            test(0.3, Some(0.7));
            test(1.0, Some(1.0));
            test(0.9, Some(0.9));
            test(0.8, Some(0.8));
            test(0.0, Some(0.0));
        }

        /// Yes, we want to ignore the target's atomic step size! We want a full control sweep to
        /// always result in a full target sweep!
        #[test]
        fn continuous_to_discrete_shifted() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::MakeRelative,
                ..Default::default()
            });
            let mut target = TestTarget {
                current_value: Some(con_val(0.4)),
                control_type: ControlType::AbsoluteDiscrete {
                    atomic_step_size: UnitValue::new(0.5),
                    is_retriggerable: false,
                },
            };
            // When
            // Then
            let mut test = |i, o| {
                abs_con_test_cumulative(&mut mode, &mut target, i, o);
            };
            test(0.0, None);
            test(0.1, Some(0.5));
            test(0.2, Some(0.6));
            test(0.4, Some(0.8));
            test(0.5, Some(0.9));
            test(0.3, Some(0.7));
            test(1.0, Some(1.0));
            test(0.9, Some(0.9));
            test(0.8, Some(0.8));
            test(0.0, Some(0.0));
        }

        /// Absolute discrete becomes relative continuous when not using discrete processing.
        #[test]
        fn discrete_to_continuous_shifted() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::MakeRelative,
                ..Default::default()
            });
            let mut target = TestTarget {
                current_value: Some(con_val(0.4)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            let mut test = |i, o| {
                abs_test_cumulative(&mut mode, &mut target, ControlValue::AbsoluteDiscrete(i), o);
            };
            test(Fraction::new(0, 10), None);
            test(Fraction::new(1, 10), Some(0.5));
            test(Fraction::new(2, 10), Some(0.6));
            test(Fraction::new(4, 10), Some(0.8));
            test(Fraction::new(5, 10), Some(0.9));
            test(Fraction::new(3, 10), Some(0.7));
            test(Fraction::new(10, 10), Some(1.0));
            test(Fraction::new(9, 10), Some(0.9));
            test(Fraction::new(8, 10), Some(0.8));
            test(Fraction::new(0, 10), Some(0.0));
        }

        /// Absolute discrete becomes relative continuous when not using discrete processing.
        #[test]
        fn discrete_to_discrete_shifted() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::MakeRelative,
                ..Default::default()
            });
            let mut target = TestTarget {
                current_value: Some(con_val(0.4)),
                control_type: ControlType::AbsoluteDiscrete {
                    atomic_step_size: UnitValue::new(0.5),
                    is_retriggerable: false,
                },
            };
            // When
            // Then
            let mut test = |i, o| {
                abs_test_cumulative(&mut mode, &mut target, ControlValue::AbsoluteDiscrete(i), o);
            };
            test(Fraction::new(0, 10), None);
            test(Fraction::new(0, 10), None);
            test(Fraction::new(1, 10), Some(0.5));
            test(Fraction::new(2, 10), Some(0.6));
            test(Fraction::new(4, 10), Some(0.8));
            test(Fraction::new(5, 10), Some(0.9));
            test(Fraction::new(3, 10), Some(0.7));
            test(Fraction::new(10, 10), Some(1.0));
            test(Fraction::new(9, 10), Some(0.9));
            test(Fraction::new(8, 10), Some(0.8));
            test(Fraction::new(0, 10), Some(0.0));
        }

        /// We can even navigate relatively in target value sequences, but step counts are ignored.
        #[test]
        fn target_value_sequence() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                // Should be translated to set of 0.0, 0.2, 0.4, 0.5, 0.9!
                target_value_sequence: "0.2, 0.4, 0.4, 0.5, 0.0, 0.9".parse().unwrap(),
                step_factor_interval: create_discrete_increment_interval(10, 10),
                absolute_mode: AbsoluteMode::MakeRelative,
                ..Default::default()
            });
            let mut target = TestTarget {
                current_value: Some(con_val(0.6)),
                control_type: ControlType::AbsoluteContinuous,
            };
            mode.update_from_target(&target, ());
            // When
            // Then
            let mut test = |i, o| {
                abs_con_test_cumulative(&mut mode, &mut target, i, o);
            };
            test(0.5, None);
            test(0.55, Some(0.9));
            test(0.6, None);
            test(0.5, Some(0.5));
            test(0.4, Some(0.4));
            test(0.3, Some(0.2));
            test(0.2, Some(0.0));
            test(0.1, None);
        }
    }

    mod performance_control {
        use super::*;

        #[test]
        fn performance_control() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::PerformanceControl,
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            let last_non_performance_target_value = Some(con_val(0.5));
            // When
            // Then
            let mut test = |i, o| {
                perf_test(
                    &mut mode,
                    &target,
                    abs_con_val(i),
                    o,
                    last_non_performance_target_value,
                );
            };
            test(0.0, Some(0.5));
            test(0.5, Some(0.75));
            test(1.0, Some(1.0));
        }

        #[test]
        fn performance_control_max() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::PerformanceControl,
                target_value_interval: create_unit_value_interval(0.0, 0.6),
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            let last_non_performance_target_value = Some(con_val(0.1));
            // When
            // Then
            let mut test = |i, o| {
                perf_test(
                    &mut mode,
                    &target,
                    abs_con_val(i),
                    o,
                    last_non_performance_target_value,
                );
            };
            test(0.0, Some(0.1));
            test(0.5, Some(0.35));
            test(1.0, Some(0.6));
        }

        #[test]
        fn performance_control_reverse() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::PerformanceControl,
                reverse: true,
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(1.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            let last_non_performance_target_value = Some(con_val(0.5));
            // When
            // Then
            let mut test = |i, o| {
                perf_test(
                    &mut mode,
                    &target,
                    abs_con_val(i),
                    o,
                    last_non_performance_target_value,
                );
            };
            test(0.0, Some(0.5));
            test(0.5, Some(0.25));
            test(1.0, Some(0.0));
        }

        #[test]
        fn performance_control_reverse_min() {
            // Given
            let mut mode: TestMode = Mode::new(ModeSettings {
                absolute_mode: AbsoluteMode::PerformanceControl,
                target_value_interval: create_unit_value_interval(0.4, 1.0),
                reverse: true,
                ..Default::default()
            });
            let target = TestTarget {
                current_value: Some(con_val(0.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            let last_non_performance_target_value = Some(con_val(0.9));
            // When
            // Then
            let mut test = |i, o| {
                perf_test(
                    &mut mode,
                    &target,
                    abs_con_val(i),
                    o,
                    last_non_performance_target_value,
                );
            };
            test(0.0, Some(0.9));
            test(0.5, Some(0.65));
            test(1.0, Some(0.4));
        }
    }

    mod relative {
        use super::*;

        mod absolute_continuous_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(-2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
            }

            #[test]
            fn default_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
                assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
            }

            #[test]
            fn trigger_target() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    step_factor_interval: create_discrete_increment_interval(-2, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: None,
                    control_type: ControlType::AbsoluteContinuousRetriggerable,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn min_step_size_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(-2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn min_step_size_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
            }

            #[test]
            fn max_step_size_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(-2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.02)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.09)
                );
            }

            #[test]
            fn max_step_size_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.91)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.98)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
                assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
            }

            #[test]
            fn rotate_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
            }

            #[test]
            fn rotate_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(-2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.79)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.79)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.79)
                );
                assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
            }

            /// See https://github.com/helgoboss/realearn/issues/577.
            #[test]
            fn target_interval_max_not_exceedable() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.0, 0.716),
                    step_size_interval: create_unit_value_interval(0.01, 0.01),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.71)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.716)
                );
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            #[test]
            fn target_interval_current_target_value_just_appearing_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.199999999999)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                // That's the point: If the current value of 0.199999999999 would be considered as
                // out-of-range, we would end up on 0.2 (target min). But we should consider it as
                // in-range and therefore use the usual incrementation logic.
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.20999999999900001)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.20999999999900001)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.20999999999900001)
                );
            }

            /// See https://github.com/helgoboss/realearn/issues/100.
            #[test]
            fn not_get_stuck() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: full_unit_interval(),
                    step_size_interval: create_unit_value_interval(0.01, 0.01),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.875)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.865)
                );
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.79)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.79)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.79)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            // TODO-medium-discrete Add tests for discrete processing
            #[test]
            fn target_value_sequence() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    // Should be translated to set of 0.0, 0.2, 0.4, 0.5, 0.9!
                    target_value_sequence: "0.2, 0.4, 0.4, 0.5, 0.0, 0.9".parse().unwrap(),
                    step_factor_interval: create_discrete_increment_interval(1, 5),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.6)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                mode.update_from_target(&target, ());
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-3), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-4), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-5), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            // TODO-medium-discrete Add tests for discrete processing
            #[test]
            fn target_value_sequence_rotate() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    // Should be translated to set of 0.0, 0.2, 0.4, 0.5, 0.9!
                    target_value_sequence: "0.2, 0.4, 0.4, 0.5, 0.0, 0.9".parse().unwrap(),
                    step_factor_interval: create_discrete_increment_interval(1, 5),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.6)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                mode.update_from_target(&target, ());
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(3), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-3), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-4), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-5), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
            }

            #[test]
            fn make_absolute_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    make_absolute: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_eq!(mode.control(rel_dis_evt(-10), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(-2), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(-1), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.02)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.03)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-5), &target, ()).unwrap(),
                    abs_con_val(0.02)
                );
            }

            #[test]
            fn make_absolute_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    make_absolute: true,
                    step_size_interval: create_unit_value_interval(0.01, 0.05),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.5)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                // TODO-medium This behavior is debatable! Normal absolute control elements don't
                //  send the same absolute value multiple times when hitting knob/fader boundary.
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                    abs_con_val(0.03)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                    abs_con_val(0.08)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-5), &target, ()).unwrap(),
                    abs_con_val(0.03)
                );
            }
        }

        mod absolute_discrete_target {
            use super::*;

            mod continuous_processing {
                use super::*;

                #[test]
                fn default_1() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                }

                #[test]
                fn default_2() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.95)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.95)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.95)
                    );
                    assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
                }

                #[test]
                fn min_step_count_1() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        step_factor_interval: create_discrete_increment_interval(4, 100),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.20)
                    );
                    // 4x
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.25)
                    );
                    // 5x
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(4), &target, ()).unwrap(),
                        abs_con_val(0.35)
                    );
                    // 7x
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.65)
                    );
                    // 13x
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(100), &target, ()).unwrap(),
                        abs_con_val(1.00)
                    );
                    // 100x
                }

                #[test]
                fn min_step_count_2() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        step_factor_interval: create_discrete_increment_interval(4, 100),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.35)
                    );
                    // 13x
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.75)
                    );
                    // 5x
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.8)
                    ); // 4x
                    assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
                }

                #[test]
                fn max_step_count_1() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        step_factor_interval: create_discrete_increment_interval(1, 2),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.10)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.10)
                    );
                }

                #[test]
                fn max_step_count_throttle() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        step_factor_interval: create_discrete_increment_interval(-2, -2),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    // No effect because already min
                    assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                    // Every 2nd time
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                }

                #[test]
                fn max_step_count_2() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        step_factor_interval: create_discrete_increment_interval(1, 2),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.90)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.90)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.95)
                    );
                    assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
                }

                #[test]
                fn reverse() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        reverse: true,
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
                }

                #[test]
                fn rotate_1() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        rotate: true,
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(1.0)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(1.0)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(1.0)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.05)
                    );
                }

                #[test]
                fn rotate_2() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        rotate: true,
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.95)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.95)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.95)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.0)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.0)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.0)
                    );
                }

                #[test]
                fn target_interval_min() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(4, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.25)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.25)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.25)
                    );
                }

                #[test]
                fn target_interval_max() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(16, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.75)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.75)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.75)
                    );
                    assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
                }

                #[test]
                fn target_interval_current_target_value_out_of_range() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                }

                #[test]
                fn target_interval_step_interval_current_target_value_out_of_range() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        step_factor_interval: create_discrete_increment_interval(1, 100),
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                }

                #[test]
                fn target_interval_snaps_to_target_step_size() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        step_factor_interval: create_discrete_increment_interval(1, 1),
                        target_value_interval: create_unit_value_interval(0.2, 0.71),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(con_val(0.6)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.2),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    // Target interval should implicitly change to 0.2 - 0.8.
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.8),
                    );
                }

                #[test]
                fn target_interval_min_rotate() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        rotate: true,
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(4, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.8)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.8)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.8)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.25)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.25)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.25)
                    );
                }

                #[test]
                fn target_interval_max_rotate() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        rotate: true,
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(16, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.75)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.75)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.75)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                }

                #[test]
                fn target_interval_rotate_current_target_value_out_of_range() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        rotate: true,
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.8)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.8)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.8)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.2)
                    );
                }

                #[test]
                fn make_absolute_1() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        make_absolute: true,
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_eq!(mode.control(rel_dis_evt(-10), &target, ()), None);
                    assert_eq!(mode.control(rel_dis_evt(-2), &target, ()), None);
                    assert_eq!(mode.control(rel_dis_evt(-1), &target, ()), None);
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.01)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.02)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.03)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-5), &target, ()).unwrap(),
                        abs_con_val(0.02)
                    );
                }

                #[test]
                fn make_absolute_2() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        make_absolute: true,
                        step_size_interval: create_unit_value_interval(0.01, 0.05),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(10, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.0)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.0)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.0)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_con_val(0.01)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_con_val(0.03)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_con_val(0.08)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-5), &target, ()).unwrap(),
                        abs_con_val(0.03)
                    );
                }
            }

            mod discrete_processing {
                use super::*;

                #[test]
                fn default_1() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        use_discrete_processing: true,
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(1.0 / 20.0),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_eq!(mode.control(rel_dis_evt(-10), &target, ()), None);
                    assert_eq!(mode.control(rel_dis_evt(-2), &target, ()), None);
                    assert_eq!(mode.control(rel_dis_evt(-1), &target, ()), None);
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_dis_val(1, 20)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_dis_val(1, 20)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_dis_val(1, 20)
                    );
                }

                #[test]
                fn default_2() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        use_discrete_processing: true,
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(1.0 / 20.0),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_dis_val(19, 20)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_dis_val(19, 20)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_dis_val(19, 20)
                    );
                    assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                    assert_eq!(mode.control(rel_dis_evt(2), &target, ()), None);
                    assert_eq!(mode.control(rel_dis_evt(10), &target, ()), None);
                }

                #[test]
                fn min_step_count_1() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        use_discrete_processing: true,
                        step_factor_interval: create_discrete_increment_interval(4, 100),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 200)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(1.0 / 200.0),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel_dis_evt(-10), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(1), &target, ()).unwrap(),
                        abs_dis_val(4, 200)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(2), &target, ()).unwrap(),
                        abs_dis_val(5, 200)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(4), &target, ()).unwrap(),
                        abs_dis_val(7, 200)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(10), &target, ()).unwrap(),
                        abs_dis_val(13, 200)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(100), &target, ()).unwrap(),
                        abs_dis_val(100, 200)
                    );
                }

                #[test]
                fn min_step_count_2() {
                    // Given
                    let mut mode: TestMode = Mode::new(ModeSettings {
                        step_factor_interval: create_discrete_increment_interval(4, 100),
                        ..Default::default()
                    });
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(1.0 / 20.0),
                            is_retriggerable: false,
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                        abs_con_val(0.35)
                    );
                    // 13x
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                        abs_con_val(0.75)
                    );
                    // 5x
                    assert_abs_diff_eq!(
                        mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                        abs_con_val(0.8)
                    ); // 4x
                    assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                    assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
                }
                //
                // #[test]
                // fn max_step_count_1() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         step_count_interval: create_discrete_increment_interval(1, 2),
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-2), &target, ()).is_none());
                //     assert!(mode.control(rel(-1), &target, ()).is_none());
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.10));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.10));
                // }
                //
                // #[test]
                // fn max_step_count_throttle() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         step_count_interval: create_discrete_increment_interval(-2, -2),
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     // No effect because already min
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     // Every 2nd time
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                //     assert!(mode.control(rel(1), &target, ()).is_none());
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                //     assert!(mode.control(rel(2), &target, ()).is_none());
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.05));
                // }
                //
                // #[test]
                // fn max_step_count_2() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         step_count_interval: create_discrete_increment_interval(1, 2),
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(20, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.90)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.90));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.95));
                //     assert!(mode.control(rel(1), &target, ()).is_none());
                //     assert!(mode.control(rel(2), &target, ()).is_none());
                //     assert!(mode.control(rel(10), &target, ()).is_none());
                // }
                //
                // #[test]
                // fn reverse() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         reverse: true,
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.05)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.05));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.05));
                //     assert!(mode.control(rel(1), &target, ()).is_none());
                //     assert!(mode.control(rel(2), &target, ()).is_none());
                //     assert!(mode.control(rel(10), &target, ()).is_none());
                // }
                //
                // #[test]
                // fn rotate_1() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         rotate: true,
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(1.0));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(1.0));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(1.0));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.05));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.05));
                // }
                //
                // #[test]
                // fn rotate_2() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         rotate: true,
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(20, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.95)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.95));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.95));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.0));
                // }
                //
                // #[test]
                // fn target_interval_min() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(4, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-2), &target, ()).is_none());
                //     assert!(mode.control(rel(-1), &target, ()).is_none());
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.25));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.25));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.25));
                // }
                //
                // #[test]
                // fn target_interval_max() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(16, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.75)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.75));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.75));
                //     assert!(mode.control(rel(1), &target, ()).is_none());
                //     assert!(mode.control(rel(2), &target, ()).is_none());
                //     assert!(mode.control(rel(10), &target, ()).is_none());
                // }
                //
                // #[test]
                // fn target_interval_current_target_value_out_of_range() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                // }
                //
                // #[test]
                // fn target_interval_step_interval_current_target_value_out_of_range() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         step_count_interval: create_discrete_increment_interval(1, 100),
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                // }
                //
                // #[test]
                // fn target_interval_min_rotate() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         rotate: true,
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(4, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.25));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.25));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.25));
                // }
                //
                // #[test]
                // fn target_interval_max_rotate() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         rotate: true,
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(16, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.75)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.75));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.75));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                // }
                //
                // #[test]
                // fn target_interval_rotate_current_target_value_out_of_range() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         rotate: true,
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                // }

                // #[test]
                // fn make_absolute_1() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         convert_relative_to_absolute: true,
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_eq!(mode.control(rel(-10), &target, ()), None);
                //     assert_eq!(mode.control(rel(-2), &target, ()), None);
                //     assert_eq!(mode.control(rel(-1), &target, ()), None);
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.02));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.03));
                //     assert_abs_diff_eq!(mode.control(rel(-5), &target, ()).unwrap(), abs_con(0.02));
                // }
                //
                // #[test]
                // fn make_absolute_2() {
                //     // Given
                //     let mut mode: TestMode = Mode::new(ModeSettings {
                //         convert_relative_to_absolute: true,
                //         step_size_interval: create_unit_value_interval(0.01, 0.05),
                //         ..Default::default()
                //     });
                //     let target = TestTarget {
                //         current_value: Some(dis_val(10, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.03));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.08));
                //     assert_abs_diff_eq!(mode.control(rel(-5), &target, ()).unwrap(), abs_con(0.03));
                // }
            }
        }

        mod relative_target {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()),
                    Some(rel_dis_val(-1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()),
                    Some(rel_dis_val(-1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()),
                    Some(rel_dis_val(-1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(1), &target, ()),
                    Some(rel_dis_val(1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(2), &target, ()),
                    Some(rel_dis_val(1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(10), &target, ()),
                    Some(rel_dis_val(1))
                );
            }

            #[test]
            fn min_step_count() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    step_factor_interval: create_discrete_increment_interval(2, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()),
                    Some(rel_dis_val(-11))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()),
                    Some(rel_dis_val(-3))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()),
                    Some(rel_dis_val(-2))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(1), &target, ()),
                    Some(rel_dis_val(2))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(2), &target, ()),
                    Some(rel_dis_val(3))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(10), &target, ()),
                    Some(rel_dis_val(11))
                );
            }

            #[test]
            fn min_step_count_throttle() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    step_factor_interval: create_discrete_increment_interval(-4, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                // So intense that reaching speedup area
                assert_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()),
                    Some(rel_dis_val(-6))
                );
                // Every 3rd time
                assert_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()),
                    Some(rel_dis_val(-1))
                );
                assert_eq!(mode.control(rel_dis_evt(-2), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(-2), &target, ()), None);
                assert_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()),
                    Some(rel_dis_val(-1))
                );
                // Every 4th time (but fired before)
                assert_eq!(mode.control(rel_dis_evt(-1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(-1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(-1), &target, ()), None);
                assert_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()),
                    Some(rel_dis_val(-1))
                );
                // Direction change
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                // Every 3rd time (but fired before)
                assert_eq!(
                    mode.control(rel_dis_evt(2), &target, ()),
                    Some(rel_dis_val(1))
                );
                assert_eq!(mode.control(rel_dis_evt(2), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(2), &target, ()), None);
                assert_eq!(
                    mode.control(rel_dis_evt(2), &target, ()),
                    Some(rel_dis_val(1))
                );
                // So intense that reaching speedup area
                assert_eq!(
                    mode.control(rel_dis_evt(10), &target, ()),
                    Some(rel_dis_val(6))
                );
            }

            #[test]
            fn max_step_count() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    step_factor_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()),
                    Some(rel_dis_val(-2))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()),
                    Some(rel_dis_val(-2))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()),
                    Some(rel_dis_val(-1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(1), &target, ()),
                    Some(rel_dis_val(1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(2), &target, ()),
                    Some(rel_dis_val(2))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(10), &target, ()),
                    Some(rel_dis_val(2))
                );
            }

            #[test]
            fn max_step_count_throttle() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    step_factor_interval: create_discrete_increment_interval(-10, -4),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                // Every 4th time
                assert_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()),
                    Some(rel_dis_val(-1))
                );
                assert_eq!(mode.control(rel_dis_evt(-10), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(-10), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(-10), &target, ()), None);
                assert_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()),
                    Some(rel_dis_val(-1))
                );
                assert_eq!(mode.control(rel_dis_evt(-10), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(-10), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(-10), &target, ()), None);
                // Every 10th time
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(
                    mode.control(rel_dis_evt(1), &target, ()),
                    Some(rel_dis_val(1))
                );
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(mode.control(rel_dis_evt(1), &target, ()), None);
                assert_eq!(
                    mode.control(rel_dis_evt(1), &target, ()),
                    Some(rel_dis_val(1))
                );
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()),
                    Some(rel_dis_val(1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()),
                    Some(rel_dis_val(1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()),
                    Some(rel_dis_val(1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(1), &target, ()),
                    Some(rel_dis_val(-1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(2), &target, ()),
                    Some(rel_dis_val(-1))
                );
                assert_eq!(
                    mode.control(rel_dis_evt(10), &target, ()),
                    Some(rel_dis_val(-1))
                );
            }
        }
    }

    mod incremental_buttons {
        use super::*;

        mod absolute_continuous_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
            }

            #[test]
            fn default_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn min_step_size_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.28)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn min_step_size_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn max_step_size_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.018)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.75), &target, ()).unwrap(),
                    abs_con_val(0.07)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.09)
                );
            }

            #[test]
            fn max_step_size_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn source_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.75), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
            }

            #[test]
            fn source_interval_step_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_size_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.75), &target, ()).unwrap(),
                    abs_con_val(0.75)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn reverse_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn reverse_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.99)
                );
            }

            #[test]
            fn rotate_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
            }

            #[test]
            fn rotate_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            #[test]
            fn rotate_3_almost_max() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.990000000001)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn reverse_and_rotate_almost_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    rotate: true,
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.00999999999999)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            #[test]
            fn reverse_and_rotate_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    rotate: true,
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.21)
                );
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            #[test]
            fn target_interval_rotate_reverse_current_target_value_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    reverse: true,
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
            }

            #[test]
            fn make_absolute_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    make_absolute: true,
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.02)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.03)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.04)
                );
            }

            // TODO-medium-discrete Add tests for discrete processing
            #[test]
            fn target_value_sequence() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    // Should be translated to set of 0.0, 0.2, 0.4, 0.5, 0.9!
                    target_value_sequence: "0.2, 0.4, 0.4, 0.5, 0.0, 0.9".parse().unwrap(),
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.6)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                mode.update_from_target(&target, ());
                // When
                // Then
                assert_eq!(mode.control(abs_con_evt(0.0), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.9)
                );
            }
        }

        mod absolute_discrete_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
            }

            #[test]
            fn default_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn min_step_count_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_factor_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.3)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
            }

            #[test]
            fn min_step_count_throttle() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_factor_interval: create_discrete_increment_interval(-4, -4),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                // Every 4th time
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
            }

            #[test]
            fn min_step_count_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_factor_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn max_step_count_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_factor_interval: create_discrete_increment_interval(1, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.1)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
            }

            #[test]
            fn max_step_count_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_factor_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-10), &target, ()).unwrap(),
                    abs_con_val(0.90)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-2), &target, ()).unwrap(),
                    abs_con_val(0.90)
                );
                assert_abs_diff_eq!(
                    mode.control(rel_dis_evt(-1), &target, ()).unwrap(),
                    abs_con_val(0.95)
                );
                assert!(mode.control(rel_dis_evt(1), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(2), &target, ()).is_none());
                assert!(mode.control(rel_dis_evt(10), &target, ()).is_none());
            }

            #[test]
            fn source_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.75), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
            }

            #[test]
            fn source_interval_step_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_factor_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.75), &target, ()).unwrap(),
                    abs_con_val(0.3)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.4)
                );
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn rotate_1() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.05)
                );
            }

            #[test]
            fn rotate_2() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.0)
                );
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.25)
                );
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(1.0), &target, ()).is_none());
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            #[test]
            fn step_count_interval_exceeded() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_factor_interval: create_discrete_increment_interval(1, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.55)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(1.0)
                );
            }

            #[test]
            fn target_interval_step_interval_current_target_value_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_factor_interval: create_discrete_increment_interval(1, 100),
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.25)
                );
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.2)
                );
            }

            #[test]
            fn target_interval_rotate_reverse_current_target_value_out_of_range() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    reverse: true,
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                        is_retriggerable: false,
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    abs_con_val(0.8)
                );
            }
        }

        mod relative_target {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    rel_dis_val(1)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    rel_dis_val(1)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    rel_dis_val(1)
                );
            }

            #[test]
            fn min_step_count() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_factor_interval: create_discrete_increment_interval(2, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    rel_dis_val(3)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    rel_dis_val(5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    rel_dis_val(8)
                );
            }

            #[test]
            fn max_step_count() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    step_factor_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    rel_dis_val(1)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    rel_dis_val(2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    rel_dis_val(2)
                );
            }

            #[test]
            fn source_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    rel_dis_val(1)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    rel_dis_val(1)
                );
            }

            #[test]
            fn source_interval_step_interval() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_factor_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con_evt(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    rel_dis_val(4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    rel_dis_val(8)
                );
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con_evt(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.1), &target, ()).unwrap(),
                    rel_dis_val(-1)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(0.5), &target, ()).unwrap(),
                    rel_dis_val(-1)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con_evt(1.0), &target, ()).unwrap(),
                    rel_dis_val(-1)
                );
            }
        }

        mod feedback {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
            }

            #[test]
            fn reverse() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    reverse: true,
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn source_and_target_interval() {
                // Given
                let mode: TestMode = Mode::new(ModeSettings {
                    absolute_mode: AbsoluteMode::IncrementalButton,
                    source_value_interval: create_unit_value_interval(0.2, 0.8),
                    target_value_interval: create_unit_value_interval(0.4, 1.0),
                    ..Default::default()
                });
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.2));
                assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.2));
                assert_abs_diff_eq!(mode.feedback(con_val(0.7)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.8));
            }
        }
    }

    mod text_feedback {
        use crate::mode::mode_struct::tests::TestMode;
        use crate::{
            AbsoluteValue, FeedbackStyle, FeedbackValue, FeedbackValueTable, Fraction, Mode,
            ModeFeedbackOptions, ModeSettings, NumericFeedbackValue, RgbColor,
            TextualFeedbackValue,
        };
        use std::borrow::Cow;
        use std::collections::HashMap;

        #[test]
        fn pass_through() {
            // Given
            let mode: TestMode = Mode::new(ModeSettings {
                ..Default::default()
            });
            // When
            let style = FeedbackStyle {
                color: Some(RgbColor::new(10, 10, 10)),
                background_color: None,
            };
            let result = mode.feedback_with_options_detail(
                Cow::Owned(FeedbackValue::Textual(TextualFeedbackValue::new(
                    style,
                    "playing".into(),
                ))),
                ModeFeedbackOptions::default(),
                (),
            );
            // Then
            assert_eq!(
                result,
                Some(Cow::Owned(FeedbackValue::Textual(
                    TextualFeedbackValue::new(style, "playing".into(),)
                )))
            );
        }

        #[test]
        fn feedback_value_table() {
            // Given
            let map: HashMap<String, u32> = [("playing", 5), ("paused", 6)]
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect();
            let mode: TestMode = Mode::new(ModeSettings {
                feedback_value_table: Some(FeedbackValueTable::FromTextToDiscrete(map)),
                ..Default::default()
            });
            // When
            let style = FeedbackStyle {
                color: Some(RgbColor::new(10, 10, 10)),
                background_color: None,
            };
            let matched_result = mode.feedback_with_options_detail(
                Cow::Owned(FeedbackValue::Textual(TextualFeedbackValue::new(
                    style,
                    "playing".into(),
                ))),
                ModeFeedbackOptions::default(),
                (),
            );
            let unmatched_result = mode.feedback_with_options_detail(
                Cow::Owned(FeedbackValue::Textual(TextualFeedbackValue::new(
                    style,
                    "bla".into(),
                ))),
                ModeFeedbackOptions::default(),
                (),
            );
            // Then
            assert_eq!(
                matched_result,
                Some(Cow::Owned(FeedbackValue::Numeric(
                    NumericFeedbackValue::new(style, AbsoluteValue::Discrete(Fraction::new_max(5)))
                )))
            );
            assert_eq!(unmatched_result, None);
        }
    }

    /// Absolute continuous control event.
    fn abs_con_evt(number: f64) -> TimelessControlEvent<ControlValue> {
        create_timeless_control_event(abs_con_val(number))
    }

    /// Absolute continuous control value.
    fn abs_con_val(number: f64) -> ControlValue {
        ControlValue::absolute_continuous(number)
    }

    /// Absolute discrete control event.
    fn abs_dis_evt(actual: u32, max: u32) -> TimelessControlEvent<ControlValue> {
        create_timeless_control_event(abs_dis_val(actual, max))
    }

    /// Absolute discrete control value.
    fn abs_dis_val(actual: u32, max: u32) -> ControlValue {
        ControlValue::absolute_discrete(actual, max)
    }

    /// Relative discrete control event.
    fn rel_dis_evt(increment: i32) -> TimelessControlEvent<ControlValue> {
        create_timeless_control_event(rel_dis_val(increment))
    }

    /// Relative discrete control value.
    fn rel_dis_val(increment: i32) -> ControlValue {
        ControlValue::relative(increment)
    }

    fn con_val(v: f64) -> AbsoluteValue {
        AbsoluteValue::Continuous(UnitValue::new(v))
    }

    fn dis_val(actual: u32, max: u32) -> AbsoluteValue {
        AbsoluteValue::Discrete(Fraction::new(actual, max))
    }

    fn abs_con_test(mode: &mut TestMode, target: &TestTarget, input: f64, output: Option<f64>) {
        abs_test(
            mode,
            target,
            ControlValue::absolute_continuous(input),
            output,
        )
    }

    fn abs_con_test_cumulative(
        mode: &mut TestMode,
        target: &mut TestTarget,
        input: f64,
        output: Option<f64>,
    ) {
        abs_test_cumulative(
            mode,
            target,
            ControlValue::absolute_continuous(input),
            output,
        )
    }

    fn abs_test(
        mode: &mut TestMode,
        target: &TestTarget,
        input: ControlValue,
        output: Option<f64>,
    ) {
        let result = mode.control(create_timeless_control_event(input), target, ());
        assert_equals(result, output)
    }

    fn assert_equals(result: Option<ControlValue>, expected: Option<f64>) {
        if let Some(e) = expected {
            assert_abs_diff_eq!(result.unwrap(), abs_con_val(e));
        } else {
            assert_eq!(result, None);
        }
    }

    fn abs_test_cumulative(
        mode: &mut TestMode,
        target: &mut TestTarget,
        input: ControlValue,
        output: Option<f64>,
    ) {
        abs_test(mode, target, input, output);
        if let Some(o) = output {
            target.current_value = Some(con_val(o));
        }
    }

    fn perf_test(
        mode: &mut TestMode,
        target: &TestTarget,
        input: ControlValue,
        output: Option<f64>,
        last_non_performance_target_value: Option<AbsoluteValue>,
    ) {
        let result = mode
            .control_with_options(
                create_timeless_control_event(input),
                target,
                (),
                ModeControlOptions::default(),
                last_non_performance_target_value,
            )
            .and_then(|r| r.into());
        assert_equals(result, output);
    }

    type TestMode = Mode<TestTransformation, NoopTimestamp>;

    type TimelessControlEvent<P> = ControlEvent<P, NoopTimestamp>;

    fn create_timeless_control_event<P>(payload: P) -> TimelessControlEvent<P> {
        TimelessControlEvent::new(payload, NoopTimestamp)
    }

    const FB_OPTS: ModeFeedbackOptions = ModeFeedbackOptions {
        source_is_virtual: false,
        // Count: 101
        max_discrete_source_value: Some(100),
    };
}
