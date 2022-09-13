use crate::RgbColor;

// Initially taken from https://github.com/jamesmunns/launch-rs/blob/master/lib/src/color.rs
pub fn find_closest_color_in_palette(color: RgbColor, palette: &[RgbColor]) -> u8 {
    let (red, green, blue) = (color.r(), color.g(), color.b());
    let mut ifurthest = 0usize;
    let mut furthest = 3 * 255_i32.pow(2) + 1;
    for (i, c) in palette.iter().enumerate() {
        if red == c.r() && green == c.g() && blue == c.b() {
            // Exact match
            return i as u8;
        }
        let distance = (red as i32 - c.r() as i32).pow(2)
            + (green as i32 - c.g() as i32).pow(2)
            + (blue as i32 - c.b() as i32).pow(2);
        if distance < furthest {
            furthest = distance;
            ifurthest = i;
        }
    }
    ifurthest as u8
}
