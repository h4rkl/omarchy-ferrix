//! Reports, for each character read from a file, whether the type stack
//! (embedded Plex plus `FERRIX_FONT_DIRS`/`/usr/share/fonts` fallback)
//! resolves it, so UI strings never ship tofu after a font change:
//!
//!   FERRIX_FONT_DIRS=/path/to/noto cargo run -p ferrix-ui --example glyph-audit -- chars.txt
//!
//! Exits 1 when any character is missing.

use ferrix_ui::draw::{Type, Weight};

fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: glyph-audit CHARS.txt")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let ty = Type::load()?;
    let mut missing = 0;
    for ch in text
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
    {
        let s = ch.to_string();
        let regular = ty.has_glyphs(Weight::Regular, &s);
        let bold = ty.has_glyphs(Weight::Bold, &s);
        if !(regular && bold) {
            missing += 1;
            println!(
                "MISSING U+{:04X} {ch} regular={regular} bold={bold}",
                ch as u32
            );
        }
    }
    println!(
        "checked {} characters, {missing} missing",
        text.chars().count()
    );
    if missing > 0 {
        std::process::exit(1);
    }
    Ok(())
}
