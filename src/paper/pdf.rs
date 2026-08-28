//! PDF rendering for portable paper recovery shares.
use crate::keypair::split::share::PaperShare;
use base64::{Engine, engine::general_purpose::STANDARD};
use printpdf::*;
use qrcode::{EcLevel, QrCode, types::Color as QrColor};
use std::error::Error;

const CHUNK_BYTES: usize = 900;
const LEFT: f32 = 18.0;
const RIGHT: f32 = 192.0;

/// Render a complete paper artifact as an A4 PDF and return its bytes.
///
/// Data pages contain independently numbered QR payloads and a printed Base64
/// copy of the exact QR bytes. The final page repeats the recovery instructions
/// and inventories every recorded paper and storage placement.
pub fn generate_paper_pdf(paper: &PaperShare) -> Result<Vec<u8>, Box<dyn Error>> {
    let serialized = yaml_serde::to_string(paper)?.into_bytes();
    let document_hash = crate::paper::input::document_hash(&serialized)?;
    let chunks = serialized.chunks(CHUNK_BYTES).collect::<Vec<_>>();
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
        add_instruction_paragraph(&mut ops, &prose, 18.0, 248.0, 9.2, 4.5);
        add_qr(&mut ops, &qr, 55.0, 70.0, 100.0);
        add_wrapped_text(
            &mut ops,
            &STANDARD.encode(&qr_bytes),
            18.0,
            62.0,
            6.0,
            3.0,
            112,
            BuiltinFont::Courier,
        );
        pages.push(PdfPage::new(Mm(210.0), Mm(297.0), ops));
    }

    let mut ops = page_header(paper, total_pages, total_pages);
    add_instruction_paragraph(&mut ops, &prose, 18.0, 248.0, 9.2, 4.5);
    let mut y = 145.0;
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
        add_text(
            &mut ops,
            &format!("- {name}"),
            22.0,
            y,
            10.0,
            BuiltinFont::Helvetica,
        );
    }
    y -= 10.0;
    add_text(
        &mut ops,
        "Shares on storage",
        18.0,
        y,
        11.0,
        BuiltinFont::HelveticaBold,
    );
    if paper.placements.storage.is_empty() {
        y -= 6.0;
        add_text(&mut ops, "- none", 22.0, y, 10.0, BuiltinFont::Helvetica);
    } else {
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

fn instructions(p: &PaperShare) -> String {
    let other_paper = p.placements.paper.len().saturating_sub(1);
    format!(
        "This is a share of a private key named {} which was created with the pkiboo utility. This is share {} of {}. If anyone has {} other shares, they can combine these to produce this private key. You should store this key in a safe place where unauthorized people cannot get it. You should not store this with any other shares of the same key. Doing so compromises the security of this process. In the event you lose all complete copies of the key, you can use the shares to reconstruct the key. Pkiboo can natively read this key. Run 'pkiboo key restore' to get instructions on how to restore this key. In addition to this key there are {} other paper shares and {} shares located on storage.",
        p.key_name,
        p.share.x,
        p.share.shamir.shares,
        p.share.shamir.threshold.saturating_sub(1),
        other_paper,
        p.placements.storage.len()
    )
}

fn page_header(p: &PaperShare, piece: usize, pieces: usize) -> Vec<Op> {
    let mut ops = Vec::new();
    add_text(
        &mut ops,
        &p.paper_name.to_string(),
        LEFT,
        280.0,
        18.0,
        BuiltinFont::HelveticaBold,
    );
    add_text(
        &mut ops,
        &format!(
            "share {} of {}  •  piece {} of {}",
            p.share.x, p.share.shamir.shares, piece, pieces
        ),
        LEFT,
        270.0,
        10.0,
        BuiltinFont::Helvetica,
    );
    ops.push(Op::SetOutlineThickness { pt: Pt(1.2) });
    ops.push(Op::DrawLine {
        line: Line {
            points: vec![line_point(LEFT, 265.0), line_point(RIGHT, 265.0)],
            is_closed: false,
        },
    });
    ops
}

fn add_instruction_paragraph(
    ops: &mut Vec<Op>,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    leading: f32,
) {
    for (index, line) in wrap(text, 105).iter().enumerate() {
        if index == 0 {
            let (before, after) = line
                .split_once("share")
                .expect("instructions contain share");
            ops.extend([
                Op::StartTextSection,
                Op::SetTextCursor {
                    pos: Point::new(Mm(x), Mm(y)),
                },
                Op::SetFont {
                    font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                    size: Pt(size),
                },
                Op::ShowText {
                    items: vec![TextItem::Text(before.into())],
                },
                Op::SetFont {
                    font: PdfFontHandle::Builtin(BuiltinFont::HelveticaOblique),
                    size: Pt(size),
                },
                Op::ShowText {
                    items: vec![TextItem::Text("share".into())],
                },
                Op::SetFont {
                    font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                    size: Pt(size),
                },
                Op::ShowText {
                    items: vec![TextItem::Text(after.into())],
                },
                Op::EndTextSection,
            ]);
        } else {
            add_text(
                ops,
                line,
                x,
                y - index as f32 * leading,
                size,
                BuiltinFont::Helvetica,
            );
        }
    }
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
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + word.len() + 1 > columns {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
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
        let pdf = generate_paper_pdf(&paper).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.len() > 10_000);
    }
}
