// ANSI escape codes for IntelliJ Darcula-like theme
// Using 256-color palette for better terminal compatibility

pub const RESET: &str = "\x1b[0m";

// Backgrounds - using 256-color for compatibility
pub const BG_DARK: &str = "\x1b[48;5;236m";
pub const BG_HEADER: &str = "\x1b[48;5;238m";
pub const BG_SELECTED: &str = "\x1b[48;5;24m";
pub const BG_PANEL: &str = "\x1b[48;5;235m";
pub const BG_HUNK: &str = "\x1b[48;5;239m";

// Foregrounds - 256-color palette
pub const FG_DEFAULT: &str = "\x1b[38;5;252m";
pub const FG_ADDED: &str = "\x1b[38;5;114m";
pub const FG_REMOVED: &str = "\x1b[38;5;210m";
pub const FG_HEADER: &str = "\x1b[38;5;75m";
pub const FG_SEPARATOR: &str = "\x1b[38;5;240m";
pub const FG_DIM: &str = "\x1b[38;5;245m";

/// Convert RGB to closest 256-color palette index
pub fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    // Check for grayscale first (where r ≈ g ≈ b)
    if r == g && g == b {
        if r < 8 {
            return 16; // black
        }
        if r > 248 {
            return 231; // white
        }
        return (((r as u16 - 8) / 10) as u8) + 232; // grayscale 232-255
    }
    
    // Convert to 6x6x6 color cube (indices 16-231)
    let r_idx = if r < 48 { 0 } else { ((r as u16 - 35) / 40).min(5) as u8 };
    let g_idx = if g < 48 { 0 } else { ((g as u16 - 35) / 40).min(5) as u8 };
    let b_idx = if b < 48 { 0 } else { ((b as u16 - 35) / 40).min(5) as u8 };
    
    16 + 36 * r_idx + 6 * g_idx + b_idx
}
