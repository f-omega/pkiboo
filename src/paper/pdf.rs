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
    let prose = instructions(paper);
    let mut pages = Vec::with_capacity(total_pages);

    for (index, chunk) in chunks.into_iter().enumerate() {
        let segment = crate::paper::input::PaperQrSegment {
            format: crate::paper::input::PAPER_QR_FORMAT.into(),
            paper: paper.paper_name.to_string(),
            document_hash: document_hash.clone(),
            share: paper.share.x,
            shares: paper.share.shamir.shares,
            piece: index + 1,
            pieces: data_pages,
            data: STANDARD.encode(chunk),
        };
        let qr_bytes = yaml_serde::to_string(&segment)?.into_bytes();
        let qr = QrCode::with_error_correction_level(&qr_bytes, EcLevel::M)?;
        let mut ops = page_header(paper, index + 1, total_pages);
        let prose_bottom =
            add_instruction_paragraphs(&mut ops, paper, &prose, 18.0, 248.0, 9.2, 4.5);
        let divider_y = prose_bottom - 3.0;
        add_blue_rule(&mut ops, divider_y);
        let qr_top = divider_y - 5.0;
        let qr_bottom = qr_top - 100.0;
        add_qr(&mut ops, &qr, 55.0, qr_bottom, 100.0);
        add_wrapped_text(
            &mut ops,
            &STANDARD.encode(&qr_bytes),
            18.0,
            qr_bottom - 8.0,
            6.0,
            3.0,
            112,
            BuiltinFont::Courier,
        );
        pages.push(PdfPage::new(Mm(210.0), Mm(297.0), ops));
    }

    let mut ops = page_header(paper, total_pages, total_pages);
    let prose_bottom = add_instruction_paragraphs(&mut ops, paper, &prose, 18.0, 248.0, 9.2, 4.5);
    let mut y = prose_bottom - 3.0;
    add_blue_rule(&mut ops, y);
    y -= 10.0;
    add_text(
        &mut ops,
        "Share placement inventory",
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

fn instructions(p: &PaperShare) -> Vec<String> {
    let other_paper = p.placements.paper.len().saturating_sub(1);
    vec![
        format!("This is a share of a private key named {} which was created with the pkiboo utility.", p.key_name),
        format!("This is share {} of {}. If anyone has {} other shares, they can combine these to produce this private key.", p.share.x, p.share.shamir.shares, p.share.shamir.threshold.saturating_sub(1)),
        "You should store this in a safe place where unauthorized people cannot get it.".into(),
        "You should not store this with any other shares of the same key. Doing so compromises the security of this process.".into(),
        "In the event you lose all complete copies of the key, you can use the shares to reconstruct the key.".into(),
        "Pkiboo can natively read this share. Run 'pkiboo key restore' to get instructions on how to restore this key.".into(),
        format!("In addition to this share there are {other_paper} other paper shares and {} shares located on storage.", p.placements.storage.len()),
    ]
}

fn page_header(p: &PaperShare, piece: usize, pieces: usize) -> Vec<Op> {
    let mut ops = Vec::new();
    add_centered_text(
        &mut ops,
        &format!("🧩 {}", p.paper_name),
        280.0,
        18.0,
        BuiltinFont::HelveticaBold,
    );
    add_centered_text(
        &mut ops,
        &format!("share {} of {}", p.share.x, p.share.shamir.shares),
        270.0,
        10.0,
        BuiltinFont::Helvetica,
    );
    ops.push(Op::SetOutlineThickness { pt: Pt(1.2) });
    ops.push(Op::SetOutlineColor {
        col: electric_blue(),
    });
    ops.push(Op::DrawLine {
        line: Line {
            points: vec![line_point(LEFT, 265.0), line_point(RIGHT, 265.0)],
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
        &format!("page {piece} of {pieces}"),
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

fn add_instruction_paragraphs(
    ops: &mut Vec<Op>,
    paper: &PaperShare,
    paragraphs: &[String],
    x: f32,
    y: f32,
    size: f32,
    leading: f32,
) -> f32 {
    let mut line_number = 0usize;
    let mut last_y = y;
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        for (line_index, line) in wrap(paragraph, 105).iter().enumerate() {
            let line_y = y - line_number as f32 * leading;
            let font = if paragraph_index == 5 {
                BuiltinFont::CourierBold
            } else {
                BuiltinFont::Helvetica
            };
            let highlight = match paragraph_index {
                0 => Some(paper.key_name.as_str()),
                1 if line_index == 0 => Some(paragraph.split('.').next().unwrap_or(paragraph)),
                _ => None,
            };
            add_highlighted_text(ops, line, highlight, x, line_y, size, font);
            last_y = line_y;
            line_number += 1;
        }
        line_number += 1;
    }
    last_y
}

fn add_highlighted_text(
    ops: &mut Vec<Op>,
    text: &str,
    highlight: Option<&str>,
    x: f32,
    y: f32,
    size: f32,
    font: BuiltinFont,
) {
    let Some(highlight) = highlight.filter(|needle| text.contains(needle)) else {
        add_text(ops, text, x, y, size, font);
        return;
    };
    let (before, rest) = text.split_once(highlight).expect("highlight is present");
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
            items: vec![TextItem::Text(before.into())],
        },
        Op::SetFillColor {
            col: electric_blue(),
        },
        Op::SetFont {
            font: PdfFontHandle::Builtin(BuiltinFont::HelveticaBold),
            size: Pt(size),
        },
        Op::ShowText {
            items: vec![TextItem::Text(highlight.into())],
        },
        Op::SetFillColor {
            col: Color::Greyscale(Greyscale::new(0.0, None)),
        },
        Op::SetFont {
            font: PdfFontHandle::Builtin(font),
            size: Pt(size),
        },
        Op::ShowText {
            items: vec![TextItem::Text(rest.into())],
        },
        Op::EndTextSection,
    ]);
}

fn electric_blue() -> Color {
    Color::Rgb(Rgb::new(0.0, 0.55, 1.0, None))
}

fn add_blue_rule(ops: &mut Vec<Op>, y: f32) {
    ops.extend([
        Op::SetOutlineColor {
            col: electric_blue(),
        },
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
