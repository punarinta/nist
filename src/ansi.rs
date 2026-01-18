use sdl3::pixels::Color;
use std::collections::HashMap;

// Default colors
pub const DEFAULT_FG_COLOR: Color = Color::RGB(255, 255, 255);
pub const DEFAULT_BG_COLOR: Color = Color::RGB(20, 20, 20);

#[derive(Default, Clone, Copy, Debug)]
pub struct TextAttributes {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub blink: bool,
    pub reverse: bool,
    pub invisible: bool,
}

// Standard 16-color palette (indexed 0-7)
pub const COLOR_MAP_16: [(u32, Color); 8] = [
    (0, DEFAULT_BG_COLOR),         // Black
    (1, Color::RGB(255, 80, 80)),  // Red
    (2, Color::RGB(80, 255, 80)),  // Green
    (3, Color::RGB(255, 255, 80)), // Yellow
    (4, Color::RGB(80, 80, 255)),  // Blue
    (5, Color::RGB(255, 80, 255)), // Magenta
    (6, Color::RGB(80, 255, 255)), // Cyan
    (7, DEFAULT_FG_COLOR),         // White
];

// Bright color palette (indexed 0-7)
pub const COLOR_MAP_BRIGHT: [(u32, Color); 8] = [
    (0, Color::RGB(128, 128, 128)), // Bright Black (Gray)
    (1, Color::RGB(255, 128, 128)), // Bright Red
    (2, Color::RGB(128, 255, 128)), // Bright Green
    (3, Color::RGB(255, 255, 128)), // Bright Yellow
    (4, Color::RGB(128, 128, 255)), // Bright Blue
    (5, Color::RGB(255, 128, 255)), // Bright Magenta
    (6, Color::RGB(128, 255, 255)), // Bright Cyan
    (7, Color::RGB(255, 255, 255)), // Bright White
];

pub fn parse_m(ansi_code: &str) -> ([Option<Color>; 2], Option<TextAttributes>) {
    // TODO: can we make it a constant?
    let color_map_16: HashMap<u32, Color> = COLOR_MAP_16.iter().map(|&(code, color)| (code, color)).collect();

    let color_map_bright: HashMap<u32, Color> = COLOR_MAP_BRIGHT.iter().map(|&(code, color)| (code, color)).collect();

    let mut fg_color = None;
    let mut bg_color = None;
    let mut attrs = TextAttributes::default();
    let mut attrs_modified = false;

    let ansi_code_parts: Vec<&str> = ansi_code.trim_start_matches("\x1b[").trim_end_matches("m").split(';').collect();

    // Handle \x1b[m (empty/no parameters) as reset (same as \x1b[0m)
    if ansi_code_parts.len() == 1 && ansi_code_parts[0].is_empty() {
        fg_color = Some(DEFAULT_FG_COLOR);
        bg_color = Some(DEFAULT_BG_COLOR);
        attrs = TextAttributes::default();
        attrs_modified = true;
        return ([fg_color, bg_color], Some(attrs));
    }

    let mut i = 0;
    while i < ansi_code_parts.len() {
        let command = match ansi_code_parts[i].parse::<i32>() {
            Ok(value) => num::abs(value) as u32,
            Err(_) => {
                i += 1;
                continue;
            }
        };

        match command {
            0 => {
                fg_color = Some(DEFAULT_FG_COLOR);
                bg_color = Some(DEFAULT_BG_COLOR);
                attrs = TextAttributes::default();
                attrs_modified = true;
                i += 1;
            }
            1 => {
                /* Bold or increased intensity */
                attrs.bold = true;
                attrs_modified = true;
                i += 1;
            }
            2 => {
                /* Faint, decreased intensity */
                // We treat faint as non-bold for simplicity
                attrs.bold = false;
                attrs_modified = true;
                i += 1;
            }
            3 => {
                /* Italic */
                attrs.italic = true;
                attrs_modified = true;
                i += 1;
            }
            4 => {
                /* Underline */
                attrs.underline = true;
                attrs_modified = true;
                i += 1;
            }
            5 => {
                /* Slow blink */
                attrs.blink = true;
                attrs_modified = true;
                i += 1;
            }
            7 => {
                /* Reverse video */
                attrs.reverse = true;
                attrs_modified = true;
                i += 1;
            }
            8 => {
                /* Invisible/hidden */
                attrs.invisible = true;
                attrs_modified = true;
                i += 1;
            }
            9 => {
                /* Strikethrough */
                attrs.strikethrough = true;
                attrs_modified = true;
                i += 1;
            }
            10 => {
                /* Primary (default) font */
                i += 1;
            }
            22 => {
                /* Normal intensity (neither bold nor faint) */
                attrs.bold = false;
                attrs_modified = true;
                i += 1;
            }
            23 => {
                /* Not italic */
                attrs.italic = false;
                attrs_modified = true;
                i += 1;
            }
            24 => {
                /* Not underlined */
                attrs.underline = false;
                attrs_modified = true;
                i += 1;
            }
            25 => {
                /* Blink off */
                attrs.blink = false;
                attrs_modified = true;
                i += 1;
            }
            27 => {
                /* Not reversed (turn off reverse video) */
                attrs.reverse = false;
                attrs_modified = true;
                i += 1;
            }
            28 => {
                /* Revealed (not invisible) */
                attrs.invisible = false;
                attrs_modified = true;
                i += 1;
            }
            29 => {
                /* Not strikethrough */
                attrs.strikethrough = false;
                attrs_modified = true;
                i += 1;
            }
            30..=37 => {
                if let Some(&color) = color_map_16.get(&(command - 30)) {
                    fg_color = Some(color);
                }
                i += 1;
            }
            38 => {
                // 256-color or 24-bit RGB foreground
                if i + 1 < ansi_code_parts.len() {
                    if let Ok(mode) = ansi_code_parts[i + 1].parse::<u32>() {
                        if mode == 5 && i + 2 < ansi_code_parts.len() {
                            // 256-color: ESC[38;5;Nm
                            if let Ok(color_idx) = ansi_code_parts[i + 2].parse::<u8>() {
                                fg_color = Some(color_256(color_idx));
                            }
                            i += 3;
                        } else if mode == 2 && i + 4 < ansi_code_parts.len() {
                            // 24-bit RGB: ESC[38;2;R;G;Bm
                            if let (Ok(r), Ok(g), Ok(b)) = (
                                ansi_code_parts[i + 2].parse::<u8>(),
                                ansi_code_parts[i + 3].parse::<u8>(),
                                ansi_code_parts[i + 4].parse::<u8>(),
                            ) {
                                fg_color = Some(Color::RGB(r, g, b));
                            }
                            i += 5;
                        } else {
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            39 => {
                fg_color = Some(DEFAULT_FG_COLOR);
                i += 1;
            }
            40..=47 => {
                if let Some(&color) = color_map_16.get(&(command - 40)) {
                    bg_color = Some(color);
                }
                i += 1;
            }
            48 => {
                // 256-color or 24-bit RGB background
                if i + 1 < ansi_code_parts.len() {
                    if let Ok(mode) = ansi_code_parts[i + 1].parse::<u32>() {
                        if mode == 5 && i + 2 < ansi_code_parts.len() {
                            // 256-color: ESC[48;5;Nm
                            if let Ok(color_idx) = ansi_code_parts[i + 2].parse::<u8>() {
                                bg_color = Some(color_256(color_idx));
                            }
                            i += 3;
                        } else if mode == 2 && i + 4 < ansi_code_parts.len() {
                            // 24-bit RGB: ESC[48;2;R;G;Bm
                            if let (Ok(r), Ok(g), Ok(b)) = (
                                ansi_code_parts[i + 2].parse::<u8>(),
                                ansi_code_parts[i + 3].parse::<u8>(),
                                ansi_code_parts[i + 4].parse::<u8>(),
                            ) {
                                bg_color = Some(Color::RGB(r, g, b));
                            }
                            i += 5;
                        } else {
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            49 => {
                bg_color = Some(DEFAULT_BG_COLOR);
                i += 1;
            }
            90..=97 => {
                // Bright foreground colors
                if let Some(&color) = color_map_bright.get(&(command - 90)) {
                    fg_color = Some(color);
                }
                i += 1;
            }
            100..=107 => {
                // Bright background colors
                if let Some(&color) = color_map_bright.get(&(command - 100)) {
                    bg_color = Some(color);
                }
                i += 1;
            }

            _ => {
                println!("Unknown SGR command '{}'", command);
                i += 1;
            }
        }
    }

    let attrs_result = if attrs_modified { Some(attrs) } else { None };
    ([fg_color, bg_color], attrs_result)
}

// Convert 256-color palette index to RGB
fn color_256(idx: u8) -> Color {
    match idx {
        // 0-7: standard colors (use our color map)
        0..=7 => COLOR_MAP_16[idx as usize].1,
        // 8-15: bright colors (use our bright color map)
        8..=15 => COLOR_MAP_BRIGHT[(idx - 8) as usize].1,
        // 16-231: 216-color cube (6x6x6)
        16..=231 => {
            let idx = idx - 16;
            let r = (idx / 36) % 6;
            let g = (idx / 6) % 6;
            let b = idx % 6;
            Color::RGB(
                if r == 0 { 0 } else { 55 + r * 40 },
                if g == 0 { 0 } else { 55 + g * 40 },
                if b == 0 { 0 } else { 55 + b * 40 },
            )
        }
        // 232-255: grayscale
        232..=255 => {
            let gray = 8 + (idx - 232) * 10;
            Color::RGB(gray, gray, gray)
        }
    }
}

pub fn parse_capital_h(ansi_code: &str) -> [i32; 2] {
    let mut row = 1;
    let mut column = 1;

    let ansi_code_parts: Vec<&str> = ansi_code
        .trim_start_matches("\x1b[")
        .trim_end_matches("H")
        .trim_end_matches("f")
        .split(';')
        .collect();

    if ansi_code_parts.len() == 2 {
        if let Ok(r) = ansi_code_parts[0].parse::<i32>() {
            row = r;
        }
        if let Ok(c) = ansi_code_parts[1].parse::<i32>() {
            column = c;
        }
    } else if ansi_code_parts.len() == 1 && !ansi_code_parts[0].is_empty() {
        if let Ok(r) = ansi_code_parts[0].parse::<i32>() {
            row = r;
        }
    }

    [row, column]
}

pub fn parse_scroll_region(ansi_code: &str) -> [i32; 2] {
    let mut top = 1;
    let mut bottom = -1; // -1 means use screen height

    let ansi_code_parts: Vec<&str> = ansi_code.trim_start_matches("\x1b[").trim_end_matches("r").split(';').collect();

    if ansi_code_parts.len() == 2 {
        if let Ok(t) = ansi_code_parts[0].parse::<i32>() {
            top = t;
        }
        if let Ok(b) = ansi_code_parts[1].parse::<i32>() {
            bottom = b;
        }
    } else if ansi_code_parts.len() == 1 && !ansi_code_parts[0].is_empty() {
        if let Ok(t) = ansi_code_parts[0].parse::<i32>() {
            top = t;
        }
    }

    [top, bottom]
}
