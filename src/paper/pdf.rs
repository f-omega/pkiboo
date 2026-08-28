//! PDF rendering for portable paper recovery shares.
use crate::keypair::split::share::PaperShare;
use base64::{Engine, engine::general_purpose::STANDARD};
use printpdf::*;
use qrcode::{EcLevel, QrCode, types::Color as QrColor};
use std::error::Error;

const LEFT: f32 = 18.0;
const RIGHT: f32 = 192.0;

/// Render a complete paper artifact as an A4 PDF and return its bytes.
///
/// Data pages contain independently numbered QR payloads and a printed Base64
/// copy of the exact QR bytes. The final page repeats the recovery instructions
/// and inventories every recorded paper and storage placement.
pub fn generate_paper_pdf(paper: &PaperShare, qr_bytes: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    if qr_bytes == 0 {
        return Err("paper QR byte count must be greater than zero".into());
    }
    let serialized = yaml_serde::to_string(paper)?.into_bytes();
    let document_hash = crate::paper::input::document_hash(&serialized)?;
    let chunks = serialized.chunks(qr_bytes).collect::<Vec<_>>();
    let data_pages = chunks.len();
    let total_pages = data_pages + 1;
    let created_on = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let prose = instructions(paper);
    let mut pages = Vec::with_capacity(total_pages);

    for (index, chunk) in chunks.into_iter().enumerate() {
        let segment = crate::paper::assembler::PaperQrSegment {
            format: crate::paper::assembler::PAPER_QR_FORMAT.into(),
            paper: paper.paper_name.to_string(),
            document_hash: document_hash.clone(),
            share: paper.share.x,
            shares: paper.share.shamir.shares,
            piece: index + 1,
            pieces: data_pages,
            data: chunk.to_vec(),
        };
        let qr_bytes = crate::paper::assembler::encode_qr_segment(&segment)?;
        let qr = QrCode::with_error_correction_level(&qr_bytes, EcLevel::M)?;
        let mut ops = page_header(
            paper,
            index + 1,
            total_pages,
            Some((index + 1, data_pages)),
            &created_on,
        );
        let prose_bottom = add_instruction_paragraphs(&mut ops, &prose, 24.0, 261.0, 9.2, 4.5);
        let divider_y = prose_bottom - 3.0;
        add_content_rule(&mut ops, divider_y);
        let qr_top = divider_y - 5.0;
        let qr_bottom = qr_top - 100.0;
        add_qr(&mut ops, &qr, 55.0, qr_bottom, 100.0);
        add_wrapped_text(
            &mut ops,
            &STANDARD.encode(&qr_bytes),
            24.0,
            qr_bottom - 8.0,
            7.0,
            3.5,
            108,
            BuiltinFont::Courier,
        );
        pages.push(PdfPage::new(Mm(210.0), Mm(297.0), ops));
    }

    let mut ops = page_header(paper, total_pages, total_pages, None, &created_on);
    let prose_bottom = add_instruction_paragraphs(&mut ops, &prose, 24.0, 261.0, 9.2, 4.5);
    let mut y = prose_bottom - 3.0;
    add_content_rule(&mut ops, y);
    y -= 10.0;
    add_text(
        &mut ops,
        &format!("Share placement inventory as of {created_on} (UTC)"),
        18.0,
        y,
        14.0,
        BuiltinFont::HelveticaBold,
    );
    y -= 9.0;
    add_text(
        &mut ops,
        "Paper shares",
        18.0,
        y,
        11.0,
        BuiltinFont::HelveticaBold,
    );
    for name in &paper.placements.paper {
        y -= 6.0;
        let this_paper = name
            .rsplit_once(": ")
            .map(|(_, paper_name)| paper_name == paper.paper_name.to_string())
            .unwrap_or_else(|| name == &paper.paper_name.to_string());
        add_text(
            &mut ops,
            &format!("- {name}{}", if this_paper { " (this one)" } else { "" }),
            22.0,
            y,
            10.0,
            BuiltinFont::Helvetica,
        );
    }
    if !paper.placements.storage.is_empty() {
        y -= 10.0;
        add_text(
            &mut ops,
            "Shares on storage",
            18.0,
            y,
            11.0,
            BuiltinFont::HelveticaBold,
        );
        for name in &paper.placements.storage {
            y -= 6.0;
            add_text(
                &mut ops,
                &format!("- {name}"),
                22.0,
                y,
                10.0,
                BuiltinFont::Helvetica,
            );
        }
    }
    pages.push(PdfPage::new(Mm(210.0), Mm(297.0), ops));

    let mut doc = PdfDocument::new(&format!("pkiboo paper share {}", paper.paper_name));
    doc.with_pages(pages);
    let mut warnings = Vec::new();
    Ok(doc.save(&PdfSaveOptions::default(), &mut warnings))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpanColor {
    Normal,
    Identity,
}

type InstructionSpan = (String, BuiltinFont, SpanColor);
type InstructionParagraph = Vec<InstructionSpan>;

fn span(text: impl Into<String>, font: BuiltinFont) -> InstructionSpan {
    (text.into(), font, SpanColor::Normal)
}

fn identity_span(text: impl Into<String>) -> InstructionSpan {
    (text.into(), BuiltinFont::HelveticaBold, SpanColor::Identity)
}

fn instructions(p: &PaperShare) -> Vec<InstructionParagraph> {
    vec![
        vec![
            span(
                "This is a piece of a private key named ",
                BuiltinFont::Helvetica,
            ),
            identity_span(&p.key_name),
            span(" which was created with the ", BuiltinFont::Helvetica),
            span("pkiboo", BuiltinFont::CourierBold),
            span(" utility.", BuiltinFont::Helvetica),
        ],
        vec![span(
            format!(
                "There are {} other pieces. Anyone with access to {} pieces can combine them to construct this key",
                p.share.x, p.share.shamir.threshold
            ),
            BuiltinFont::Helvetica,
        )],
        vec![span(
            "You should store this piece away from other pieces of the same key. Storing keys together compromises the security of this process",
            BuiltinFont::Helvetica,
        )],
        vec![
            span(
                "In the event you lose all complete copies of the key, ",
                BuiltinFont::Helvetica,
            ),
            span("pkiboo", BuiltinFont::CourierBold),
            span(
                " can be used to reconstruct it. Run ",
                BuiltinFont::Helvetica,
            ),
            span("pkiboo key restore", BuiltinFont::CourierBold),
            span(
                " to get instructions on how to restore this key",
                BuiltinFont::Helvetica,
            ),
        ],
        vec![
            span("To learn more about ", BuiltinFont::Helvetica),
            span("pkiboo", BuiltinFont::CourierBold),
            span(", visit ", BuiltinFont::Helvetica),
            span("https://github.com/f-omega/pkiboo", BuiltinFont::Courier),
        ],
    ]
}

fn page_header(
    p: &PaperShare,
    page: usize,
    pages: usize,
    qr_piece: Option<(usize, usize)>,
    created_on: &str,
) -> Vec<Op> {
    let mut ops = Vec::new();
    add_centered_text(
        &mut ops,
        &paper_split_name(p),
        282.0,
        18.0,
        BuiltinFont::HelveticaBold,
    );
    let identity = match qr_piece {
        Some((piece, pieces)) => format!(
            "share {} of {}  •  piece {} of {}",
            p.share.x, p.share.shamir.shares, piece, pieces
        ),
        None => format!("share {} of {}", p.share.x, p.share.shamir.shares),
    };
    add_centered_text(&mut ops, &identity, 275.0, 10.0, BuiltinFont::Helvetica);
    ops.push(Op::SetOutlineThickness { pt: Pt(1.2) });
    ops.push(Op::SetOutlineColor { col: rule_color() });
    ops.push(Op::DrawLine {
        line: Line {
            points: vec![line_point(LEFT, 268.0), line_point(RIGHT, 268.0)],
            is_closed: false,
        },
    });
    ops.push(Op::SetFillColor {
        col: Color::Greyscale(Greyscale {
            percent: 0.55,
            icc_profile: None,
        }),
    });
    add_centered_text(
        &mut ops,
        &format!("created on {created_on} (UTC)"),
        13.0,
        7.5,
        BuiltinFont::Helvetica,
    );
    add_centered_text(
        &mut ops,
        &format!("page {page} of {pages}"),
        8.0,
        9.0,
        BuiltinFont::Helvetica,
    );
    ops.push(Op::SetFillColor {
        col: Color::Greyscale(Greyscale {
            percent: 0.0,
            icc_profile: None,
        }),
    });
    ops
}

fn paper_split_name(paper: &PaperShare) -> String {
    let paper_name = paper.paper_name.to_string();
    paper_name
        .strip_suffix(&format!("-{}", paper.share.x))
        .unwrap_or(&paper_name)
        .to_owned()
}

fn add_instruction_paragraphs(
    ops: &mut Vec<Op>,
    paragraphs: &[InstructionParagraph],
    x: f32,
    y: f32,
    size: f32,
    leading: f32,
) -> f32 {
    let mut line_number = 0usize;
    let mut last_y = y;
    for paragraph in paragraphs {
        for line in wrap_spans(paragraph, 98) {
            let line_y = y - line_number as f32 * leading;
            add_styled_line(ops, &line, x, line_y, size);
            last_y = line_y;
            line_number += 1;
        }
        line_number += 1;
    }
    last_y
}

fn add_styled_line(ops: &mut Vec<Op>, spans: &[InstructionSpan], x: f32, y: f32, size: f32) {
    ops.extend([
        Op::StartTextSection,
        Op::SetTextCursor {
            pos: Point::new(Mm(x), Mm(y)),
        },
    ]);
    for (text, font, color) in spans {
        ops.extend([
            Op::SetFillColor {
                col: match color {
                    SpanColor::Normal => Color::Greyscale(Greyscale::new(0.0, None)),
                    SpanColor::Identity => identity_blue(),
                },
            },
            Op::SetFont {
                font: PdfFontHandle::Builtin(*font),
                size: Pt(size),
            },
            Op::ShowText {
                items: vec![TextItem::Text(text.clone())],
            },
        ]);
    }
    ops.extend([
        Op::EndTextSection,
        Op::SetFillColor {
            col: Color::Greyscale(Greyscale::new(0.0, None)),
        },
    ]);
}

fn wrap_spans(spans: &[InstructionSpan], columns: usize) -> Vec<Vec<InstructionSpan>> {
    let mut lines = Vec::new();
    let mut line = Vec::new();
    let mut width = 0usize;

    for (text, font, color) in spans {
        for word in text.split_whitespace() {
            let word_width = word.chars().count();
            let separator = usize::from(width > 0);
            if width > 0 && width + separator + word_width > columns {
                lines.push(std::mem::take(&mut line));
                width = 0;
            }
            let text = if width == 0 {
                word.to_owned()
            } else {
                format!(" {word}")
            };
            width += text.chars().count();
            line.push((text, *font, *color));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn identity_blue() -> Color {
    Color::Rgb(Rgb::new(0.25, 0.38, 0.60, None))
}

fn rule_color() -> Color {
    Color::Rgb(Rgb::new(0.30, 0.46, 0.54, None))
}

fn add_content_rule(ops: &mut Vec<Op>, y: f32) {
    ops.extend([
        Op::SetOutlineColor { col: rule_color() },
        Op::SetOutlineThickness { pt: Pt(1.2) },
        Op::DrawLine {
            line: Line {
                points: vec![line_point(LEFT, y), line_point(RIGHT, y)],
                is_closed: false,
            },
        },
    ]);
}

fn add_centered_text(ops: &mut Vec<Op>, text: &str, y: f32, size: f32, font: BuiltinFont) {
    // Built-in PDF fonts do not expose metrics here. This conservative average
    // glyph width gives a visually centered heading while keeping long names safe.
    let width_mm = text.chars().count() as f32 * size * 0.176;
    let x = ((210.0 - width_mm) / 2.0).max(LEFT);
    add_text(ops, text, x, y, size, font);
}
fn add_wrapped_text(
    ops: &mut Vec<Op>,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    leading: f32,
    columns: usize,
    font: BuiltinFont,
) {
    for (i, line) in wrap(text, columns).iter().enumerate() {
        add_text(ops, line, x, y - i as f32 * leading, size, font);
    }
}
fn wrap(text: &str, columns: usize) -> Vec<String> {
    if columns == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + word.chars().count() + 1 > columns {
            lines.push(std::mem::take(&mut line));
        }

        let mut remaining = word;
        while remaining.chars().count() > columns {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            let split_at = remaining
                .char_indices()
                .nth(columns)
                .map(|(index, _)| index)
                .unwrap_or(remaining.len());
            lines.push(remaining[..split_at].to_owned());
            remaining = &remaining[split_at..];
        }
        if !remaining.is_empty() {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(remaining);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
fn add_text(ops: &mut Vec<Op>, text: &str, x: f32, y: f32, size: f32, font: BuiltinFont) {
    ops.extend([
        Op::StartTextSection,
        Op::SetTextCursor {
            pos: Point::new(Mm(x), Mm(y)),
        },
        Op::SetFont {
            font: PdfFontHandle::Builtin(font),
            size: Pt(size),
        },
        Op::ShowText {
            items: vec![TextItem::Text(text.into())],
        },
        Op::EndTextSection,
    ]);
}
fn add_qr(ops: &mut Vec<Op>, qr: &QrCode, x: f32, y: f32, size: f32) {
    let quiet = 4usize;
    let modules = qr.width() + quiet * 2;
    let cell = size / modules as f32;
    ops.push(Op::SetFillColor {
        col: Color::Greyscale(Greyscale {
            percent: 0.0,
            icc_profile: None,
        }),
    });
    for row in 0..qr.width() {
        for col in 0..qr.width() {
            if qr[(col, row)] == QrColor::Dark {
                let left = x + (col + quiet) as f32 * cell;
                let bottom = y + size - (row + quiet + 1) as f32 * cell;
                ops.push(Op::DrawPolygon {
                    polygon: rect(left, bottom, cell),
                });
            }
        }
    }
}
fn rect(x: f32, y: f32, s: f32) -> Polygon {
    Polygon {
        rings: vec![PolygonRing {
            points: vec![
                line_point(x, y),
                line_point(x + s, y),
                line_point(x + s, y + s),
                line_point(x, y + s),
            ],
        }],
        mode: PaintMode::Fill,
        winding_order: WindingOrder::NonZero,
    }
}
fn line_point(x: f32, y: f32) -> LinePoint {
    LinePoint {
        p: Point::new(Mm(x), Mm(y)),
        bezier: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        keypair::split::share::*,
        multihash::{HashAlgorithm, MultiHash},
        pkiboo::Paper,
        util::Name,
    };
    #[test]
    fn pdf_has_data_and_inventory_pages() {
        let shares = crate::keypair::split::share::split_private_key(
            b"pem",
            MultiHash::new(HashAlgorithm::SHA256, "ab".repeat(32)),
            2,
            2,
        )
        .unwrap();
        let paper = PaperShare {
            key_name: "root-key".into(),
            paper_name: Name::<Paper>::new("one-two-three-four-five-six".into()),
            share: shares[0].clone(),
            placements: PaperSharePlacements {
                paper: vec!["one-two-three-four-five-six".into()],
                storage: vec!["vault-usb".into()],
            },
        };
        let pdf = generate_paper_pdf(&paper, 256).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.len() > 10_000);
    }

    #[test]
    fn wraps_unbroken_base64_at_the_requested_width() {
        let text = "A".repeat(250);
        let lines = wrap(&text, 80);
        assert_eq!(
            lines.iter().map(String::len).collect::<Vec<_>>(),
            vec![80, 80, 80, 10]
        );
        assert_eq!(lines.concat(), text);
    }

    #[test]
    fn instruction_wrapping_preserves_explicit_span_styles() {
        let paragraph = vec![
            span("ordinary words", BuiltinFont::Helvetica),
            identity_span("important-name"),
            span("command", BuiltinFont::CourierBold),
        ];
        let lines = wrap_spans(&paragraph, 20);
        assert!(lines.len() > 1);
        let spans = lines.into_iter().flatten().collect::<Vec<_>>();
        assert!(spans.iter().any(|(text, font, color)| {
            text.contains("important-name")
                && *font == BuiltinFont::HelveticaBold
                && *color == SpanColor::Identity
        }));
        assert!(spans.iter().any(|(text, font, color)| {
            text.contains("command")
                && *font == BuiltinFont::CourierBold
                && *color == SpanColor::Normal
        }));
    }

    #[test]
    fn rejects_an_empty_qr_payload_budget() {
        let shares = crate::keypair::split::share::split_private_key(
            b"pem",
            MultiHash::new(HashAlgorithm::SHA256, "ab".repeat(32)),
            2,
            2,
        )
        .unwrap();
        let paper = PaperShare {
            key_name: "root-key".into(),
            paper_name: Name::<Paper>::new("one-two-three-four-five-six".into()),
            share: shares[0].clone(),
            placements: PaperSharePlacements {
                paper: Vec::new(),
                storage: Vec::new(),
            },
        };
        assert!(generate_paper_pdf(&paper, 0).is_err());
    }
}
