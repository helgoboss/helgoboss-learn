use crate::RgbColor;

// Initially taken from here:
// https://github.com/dozius/TwisterSister/blob/main/src/main/java/io/github/dozius/twister/TwisterColors.java
pub const COLOR_PALETTE: [RgbColor; 128] = [
    // 0..64
    RgbColor::new(0, 0, 0),       // 0
    RgbColor::new(0, 0, 255),     // 1 - Blue
    RgbColor::new(0, 21, 255),    // 2 - Blue (Green Rising)
    RgbColor::new(0, 34, 255),    //
    RgbColor::new(0, 46, 255),    //
    RgbColor::new(0, 59, 255),    //
    RgbColor::new(0, 68, 255),    //
    RgbColor::new(0, 80, 255),    //
    RgbColor::new(0, 93, 255),    //
    RgbColor::new(0, 106, 255),   //
    RgbColor::new(0, 119, 255),   //
    RgbColor::new(0, 127, 255),   //
    RgbColor::new(0, 140, 255),   //
    RgbColor::new(0, 153, 255),   //
    RgbColor::new(0, 165, 255),   //
    RgbColor::new(0, 178, 255),   //
    RgbColor::new(0, 191, 255),   //
    RgbColor::new(0, 199, 255),   //
    RgbColor::new(0, 212, 255),   //
    RgbColor::new(0, 225, 255),   //
    RgbColor::new(0, 238, 255),   //
    RgbColor::new(0, 250, 255),   // 21 - End of Blue's Reign
    RgbColor::new(0, 255, 250),   // 22 - Green (Blue Fading)
    RgbColor::new(0, 255, 237),   //
    RgbColor::new(0, 255, 225),   //
    RgbColor::new(0, 255, 212),   //
    RgbColor::new(0, 255, 199),   //
    RgbColor::new(0, 255, 191),   //
    RgbColor::new(0, 255, 178),   //
    RgbColor::new(0, 255, 165),   //
    RgbColor::new(0, 255, 153),   //
    RgbColor::new(0, 255, 140),   //
    RgbColor::new(0, 255, 127),   //
    RgbColor::new(0, 255, 119),   //
    RgbColor::new(0, 255, 106),   //
    RgbColor::new(0, 255, 93),    //
    RgbColor::new(0, 255, 80),    //
    RgbColor::new(0, 255, 67),    //
    RgbColor::new(0, 255, 59),    //
    RgbColor::new(0, 255, 46),    //
    RgbColor::new(0, 255, 33),    //
    RgbColor::new(0, 255, 21),    //
    RgbColor::new(0, 255, 8),     //
    RgbColor::new(0, 255, 0),     // 43 - Green
    RgbColor::new(12, 255, 0),    // 44 - Green/Red Rising
    RgbColor::new(25, 255, 0),    //
    RgbColor::new(38, 255, 0),    //
    RgbColor::new(51, 255, 0),    //
    RgbColor::new(63, 255, 0),    //
    RgbColor::new(72, 255, 0),    //
    RgbColor::new(84, 255, 0),    //
    RgbColor::new(97, 255, 0),    //
    RgbColor::new(110, 255, 0),   //
    RgbColor::new(123, 255, 0),   //
    RgbColor::new(131, 255, 0),   //
    RgbColor::new(144, 255, 0),   //
    RgbColor::new(157, 255, 0),   //
    RgbColor::new(170, 255, 0),   //
    RgbColor::new(182, 255, 0),   //
    RgbColor::new(191, 255, 0),   //
    RgbColor::new(203, 255, 0),   //
    RgbColor::new(216, 255, 0),   //
    RgbColor::new(229, 255, 0),   //
    RgbColor::new(242, 255, 0),   //
    RgbColor::new(255, 255, 0),   // 64 - Green + Red (Yellow)
    RgbColor::new(255, 246, 0),   // 65 - Red, Green Fading
    RgbColor::new(255, 233, 0),   //
    RgbColor::new(255, 220, 0),   //
    RgbColor::new(255, 208, 0),   //
    RgbColor::new(255, 195, 0),   //
    RgbColor::new(255, 187, 0),   //
    RgbColor::new(255, 174, 0),   //
    RgbColor::new(255, 161, 0),   //
    RgbColor::new(255, 148, 0),   //
    RgbColor::new(255, 135, 0),   //
    RgbColor::new(255, 127, 0),   //
    RgbColor::new(255, 114, 0),   //
    RgbColor::new(255, 102, 0),   //
    RgbColor::new(255, 89, 0),    //
    RgbColor::new(255, 76, 0),    //
    RgbColor::new(255, 63, 0),    //
    RgbColor::new(255, 55, 0),    //
    RgbColor::new(255, 42, 0),    //
    RgbColor::new(255, 29, 0),    //
    RgbColor::new(255, 16, 0),    //
    RgbColor::new(255, 4, 0),     // 85 - End Red/Green Fading
    RgbColor::new(255, 0, 4),     // 86 - Red/ Blue Rising
    RgbColor::new(255, 0, 16),    //
    RgbColor::new(255, 0, 29),    //
    RgbColor::new(255, 0, 42),    //
    RgbColor::new(255, 0, 55),    //
    RgbColor::new(255, 0, 63),    //
    RgbColor::new(255, 0, 76),    //
    RgbColor::new(255, 0, 89),    //
    RgbColor::new(255, 0, 102),   //
    RgbColor::new(255, 0, 114),   //
    RgbColor::new(255, 0, 127),   //
    RgbColor::new(255, 0, 135),   //
    RgbColor::new(255, 0, 148),   //
    RgbColor::new(255, 0, 161),   //
    RgbColor::new(255, 0, 174),   //
    RgbColor::new(255, 0, 186),   //
    RgbColor::new(255, 0, 195),   //
    RgbColor::new(255, 0, 208),   //
    RgbColor::new(255, 0, 221),   //
    RgbColor::new(255, 0, 233),   //
    RgbColor::new(255, 0, 246),   //
    RgbColor::new(255, 0, 255),   // 107 - Blue + Red
    RgbColor::new(242, 0, 255),   // 108 - Blue/ Red Fading
    RgbColor::new(229, 0, 255),   //
    RgbColor::new(216, 0, 255),   //
    RgbColor::new(204, 0, 255),   //
    RgbColor::new(191, 0, 255),   //
    RgbColor::new(182, 0, 255),   //
    RgbColor::new(169, 0, 255),   //
    RgbColor::new(157, 0, 255),   //
    RgbColor::new(144, 0, 255),   //
    RgbColor::new(131, 0, 255),   //
    RgbColor::new(123, 0, 255),   //
    RgbColor::new(110, 0, 255),   //
    RgbColor::new(97, 0, 255),    //
    RgbColor::new(85, 0, 255),    //
    RgbColor::new(72, 0, 255),    //
    RgbColor::new(63, 0, 255),    //
    RgbColor::new(50, 0, 255),    //
    RgbColor::new(38, 0, 255),    //
    RgbColor::new(25, 0, 255),    // 126 - Blue-ish
    RgbColor::new(240, 240, 225), // 127 - White ?
];
