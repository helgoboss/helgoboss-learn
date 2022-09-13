use crate::source::color_util::find_closest_color_in_palette;
use crate::{MackieLcdScope, RgbColor};
use std::collections::HashMap;

// TODO-high CONTINUE
const COLOR_PALETTE: [RgbColor; 8] = [
    // 0..64
    RgbColor::new(0, 0, 0),    // 0
    RgbColor::new(0, 0, 255),  // 1 - Blue
    RgbColor::new(0, 21, 255), // 2 - Blue (Green Rising)
    RgbColor::new(0, 34, 255), //
    RgbColor::new(0, 46, 255), //
    RgbColor::new(0, 59, 255), //
    RgbColor::new(0, 68, 255), //
    RgbColor::new(0, 80, 255),
];

/// Global state for a particular Behringer X-Touch device.
///
/// It's used when choosing the X-Touch Mackie display MIDI source in order to determine if a
/// sys-ex message needs to be sent to change the display color, and if yes, which one. We need
/// global state here because, unfortunately, the color can only be changed for all displays
/// (channels) at once. However, ReaLearn's color feedback design allows for defining the color
/// in a very fine-granular way - as part of the feedback value (its "style"), and thus resides
/// within the scope of a mapping.
///
/// We need to make sure that when changing the color for one display, that the colors of the other
/// displays remain unchanged. This is impossible without having access to the current state of the
/// other displays because there's no sys-ex to change the color of just one display.
///
/// One alternative would have been to somehow restructure ReaLearn's feedback design so that
/// we always transfer batches of texts and colors ... but that wouldn't go well with the
/// concept where one mapping can change something very small and specific (which makes ReaLearn so
/// flexible and composable).
///
/// Another alternative would have been to make the feedback source value something more
/// abstract than concrete MIDI messages and then creating the concrete MIDI message at a later
/// stage when all information is available (probably in the struct that has access to the global
/// source context state).
#[derive(Debug, Default)]
pub struct XTouchMackieLcdState {
    state_by_extender: HashMap<u8, XTouchMackieExtenderLcdState>,
}

#[derive(Debug, Default)]
struct XTouchMackieExtenderLcdState {
    color_index_by_channel: [Option<u8>; MackieLcdScope::CHANNEL_COUNT as usize],
}

const EMPTY_COLOR_INDEX_BY_CHANNEL: XTouchMackieExtenderLcdState = XTouchMackieExtenderLcdState {
    color_index_by_channel: [None; MackieLcdScope::CHANNEL_COUNT as usize],
};

impl XTouchMackieLcdState {
    /// Returns `true` if something has changed for the given extender.
    ///
    /// In that case, the sys-ex should be sent again.
    pub fn notify_color_requested(
        &mut self,
        extender_index: u8,
        channel: u8,
        color_index: Option<u8>,
    ) -> bool {
        let extender_state = self.state_by_extender.entry(extender_index).or_default();
        let previous_color_index = extender_state.color_index_by_channel[channel as usize];
        extender_state.color_index_by_channel[channel as usize] = color_index;
        color_index != previous_color_index
    }

    /// Returns the sys-ex bytes for setting the colors for the given extender.
    pub fn sysex(&self, extender_index: u8) -> impl Iterator<Item = u8> + '_ {
        let start = [0xF0, 0x00, 0x00, 0x66, 0x14 + extender_index, 0x72];
        let extender_state = self
            .state_by_extender
            .get(&extender_index)
            .unwrap_or(&EMPTY_COLOR_INDEX_BY_CHANNEL);
        let color_indexes = extender_state
            .color_index_by_channel
            .iter()
            .map(|color_index| color_index.unwrap_or(X_TOUCH_DEFAULT_COLOR_INDEX));
        start
            .into_iter()
            .chain(color_indexes)
            .chain(std::iter::once(0xF7))
    }
}

pub fn get_x_touch_color_index_for_color(color: RgbColor) -> u8 {
    find_closest_color_in_palette(color, &COLOR_PALETTE)
}

const X_TOUCH_DEFAULT_COLOR_INDEX: u8 = 0;
