//! Font coverage through the public HTML-to-PDF constructors.
use pdf_oxide::{api::Pdf, PdfDocument};
use std::sync::Mutex;

const SANS: &[u8] = include_bytes!("fixtures/fonts/DejaVuSans.ttf");
const MONO: &[u8] = include_bytes!("fixtures/fonts/DejaVuSansMono.ttf");

#[test]
fn test_missing_glyphs_report_font_and_character_count() {
    struct Capture(Mutex<Vec<String>>);
    impl log::Log for Capture {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() == log::Level::Warn
        }
        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                self.0.lock().unwrap().push(record.args().to_string());
            }
        }
        fn flush(&self) {}
    }
    static WARNINGS: Capture = Capture(Mutex::new(Vec::new()));
    log::set_logger(&WARNINGS).unwrap();
    log::set_max_level(log::LevelFilter::Warn);

    Pdf::from_html_css("<p>A漢漢B</p>", "", SANS.to_vec()).expect("an outline font is usable");
    let warnings = WARNINGS.0.lock().unwrap().clone();
    assert!(
        warnings
            .iter()
            .any(|msg| msg.contains("Body") && msg.contains("2 characters")),
        "expected a warning naming the font and both uncovered characters: {warnings:?}"
    );
}

#[test]
fn test_registered_fallback_recovers_glyphs_inside_text_and_generated_content() {
    // Sans covers both U+01C4 (DZ with caron) and U+10300 (Old Italic A). Mono covers neither.
    for (html, css) in [
        ("<p>AǄ𐌀B</p>", "p { font-family: Mono, Sans }"),
        ("<p>x</p>", "p { font-family: Mono, Sans } p::after { content: 'AǄ𐌀B' }"),
    ] {
        let pdf = Pdf::from_html_css_with_fonts(
            html,
            css,
            vec![
                (
                    "OtherSans".into(),
                    include_bytes!("fixtures/fonts/DejaVuSans-Bold.ttf").to_vec(),
                ),
                ("Mono".into(), MONO.to_vec()),
                ("Sans".into(), SANS.to_vec()),
            ],
        )
        .expect("use both outline fonts");
        let doc = PdfDocument::from_bytes(pdf.into_bytes()).expect("reopen generated PDF");
        let text = doc.extract_text(0).unwrap();
        assert!(text.contains("AǄ𐌀B"), "{html}: {text:?}");
        let chars = doc.extract_chars(0).unwrap();
        let fallback = chars.iter().find(|ch| ch.char == 'Ǆ').unwrap();
        assert!(
            fallback.font_name.ends_with("Sans") && !fallback.font_name.ends_with("OtherSans"),
            "{chars:?}"
        );
    }
}

/// Build a font with one bitmap strike and the mandatory SFNT tables.
/// A single empty glyph is enough: the writer must reject this format at registration.
fn bitmap_font() -> Vec<u8> {
    use ttf_parser::{RawFace, Tag};
    let template = RawFace::parse(SANS, 0).unwrap();
    let mut tables: Vec<_> = [b"head", b"hhea", b"maxp"]
        .into_iter()
        .map(|tag| (tag, template.table(Tag::from_bytes(tag)).unwrap().to_vec()))
        .collect();
    // numGlyphs
    tables[2].1[4..6].copy_from_slice(&1u16.to_be_bytes());
    // Version, flags, one strike at offset 12; ppem, ppi, two equal glyph offsets.
    let sbix = vec![
        0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 12, 0, 16, 0, 72, 0, 0, 0, 12, 0, 0, 0, 12,
    ];
    tables.push((b"sbix", sbix));
    let mut font = vec![0, 1, 0, 0, 0, 4, 0, 64, 0, 2, 0, 0];
    font.resize(12 + 16 * tables.len(), 0);
    for (i, (tag, data)) in tables.into_iter().enumerate() {
        let offset = font.len();
        let record = &mut font[12 + i * 16..][..16];
        record[..4].copy_from_slice(tag);
        record[8..12].copy_from_slice(&(offset as u32).to_be_bytes());
        record[12..16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        font.extend(data);
        font.resize(font.len().next_multiple_of(4), 0);
    }
    font
}

#[test]
fn test_bitmap_only_font_is_rejected_before_it_can_be_embedded() {
    let result =
        Pdf::from_html_css_with_fonts("<p>123</p>", "", vec![("Bitmap".into(), bitmap_font())]);
    let error = result
        .err()
        .expect("bitmap-only font must be rejected")
        .to_string();
    assert!(error.contains("Bitmap") && error.contains("outline"), "{error}");
}
