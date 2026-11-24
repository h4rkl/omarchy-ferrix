//! Renders the Ferrix type stack (Plex Sans Condensed, Plex Mono, Noto
//! fallback) over Latin, Arabic, Devanagari, CJK and mixed-direction samples
//! to a PPM sheet for shaping review:
//!
//!   FERRIX_FONT_DIRS=/path/to/noto cargo run -p ferrix-ui --example type-sheet -- out/type-sheet.ppm
//!
//! Fallback faces come from `FERRIX_FONT_DIRS` (or `/usr/share/fonts`). Each
//! sample line ends with `ok` when every glyph resolved, or `MISSING` when a
//! script has no installed face; nothing is silently dropped. The sample text
//! is synthetic ("test contact" and pangram-style phrases).

use ferrix_ui::draw::{Canvas, Style, Type, Weight};
use ferrix_ui::theme;

const SAMPLES: &[(&str, &str)] = &[
    ("Latin", "Omarchy Ferrix 0123456789 \u{2026}"),
    (
        "Arabic (RTL, joined)",
        "\u{062c}\u{0647}\u{0629} \u{0627}\u{062a}\u{0635}\u{0627}\u{0644} \u{062a}\u{062c}\u{0631}\u{064a}\u{0628}\u{064a}\u{0629}",
    ),
    (
        "Arabic + digits",
        "\u{0627}\u{062e}\u{062a}\u{0628}\u{0627}\u{0631} 09 \u{0645}\u{0631}\u{062d}\u{0628}\u{0627}",
    ),
    (
        "Devanagari (conjuncts)",
        "\u{092a}\u{0930}\u{0940}\u{0915}\u{094d}\u{0937}\u{0923} \u{0938}\u{0902}\u{092a}\u{0930}\u{094d}\u{0915} \u{0936}\u{094d}\u{0930}\u{0940}",
    ),
    (
        "Han (CJK SC)",
        "\u{6d4b}\u{8bd5}\u{8054}\u{7cfb}\u{4eba} \u{4e2d}\u{6587}",
    ),
    (
        "Kana / Hangul",
        "\u{30c6}\u{30b9}\u{30c8} \u{d14c}\u{c2a4}\u{d2b8}",
    ),
    (
        "Mixed LTR + RTL",
        "Test Contact 12 \u{0627}\u{062e}\u{062a}\u{0628}\u{0627}\u{0631}",
    ),
];

fn main() -> Result<(), String> {
    let out = std::env::args().nth(1).ok_or("usage: type-sheet OUT.ppm")?;
    let t = theme::get();
    let ty = Type::load()?;
    let row_h = 120;
    let width = 1400;
    let mut c = Canvas::new(width, row_h * (SAMPLES.len() as i32 + 2), t.bg);
    let label = Style::regular(22.0, t.fg_dim);
    for (i, (name, text)) in SAMPLES.iter().enumerate() {
        let y = i as i32 * row_h;
        ty.text(&mut c, label, name, 16, y + 34);
        let ok = ty.has_glyphs(Weight::Regular, text);
        ty.text(
            &mut c,
            label,
            if ok { "ok" } else { "MISSING" },
            width - 140,
            y + 34,
        );
        ty.text(&mut c, Style::regular(44.0, t.fg), text, 16, y + 92);
        let sans_w = ty.width(Style::regular(44.0, 0), text);
        ty.text(&mut c, Style::bold(44.0, t.fg), text, 48 + sans_w, y + 92);
    }
    let y = SAMPLES.len() as i32 * row_h;
    ty.text(&mut c, label, "Plex Mono", 16, y + 34);
    ty.text(
        &mut c,
        Style::mono(40.0, t.fg),
        "fn main() { println!(\"ferrix\"); } // 0O 1lI",
        16,
        y + 92,
    );
    std::fs::write(&out, c.ppm_bytes()).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}
