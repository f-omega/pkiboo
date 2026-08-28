//! Asynchronous ingestion of numbered paper-share QR chunks from image files.
use crate::{keypair::split::share::PaperShare, ui::Task};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    path::{Path, PathBuf},
    time::Duration,
};

pub const PAPER_QR_FORMAT: &str = "pkiboo-paper-share";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaperQrSegment {
    pub format: String,
    pub paper: String,
    pub document_hash: String,
    pub share: u8,
    pub shares: u8,
    pub piece: usize,
    pub pieces: usize,
    /// Raw bytes from this piece of the serialized YAML `PaperShare`.
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

pub fn encode_qr_segment(segment: &PaperQrSegment) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(bson::serialize_to_vec(segment)?)
}

fn decode_qr_segment(bytes: &[u8]) -> Result<PaperQrSegment, Box<dyn Error>> {
    Ok(bson::deserialize_from_slice(bytes)?)
}

#[derive(Default)]
pub struct PaperInput {
    directory: PathBuf,
    emitted: HashSet<String>,
    reported: HashSet<String>,
}

impl PaperInput {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            ..Self::default()
        }
    }

    /// Wait for the next complete, commitment-valid paper share. Repeated calls
    /// yield additional complete documents, making this usable beside media or
    /// future remote-share waiters in a ceremony.
    pub async fn next_share<T: Task>(&mut self, task: &T) -> Result<PaperShare, Box<dyn Error>> {
        if !self.directory.is_dir() {
            return Err(format!(
                "Paper input {} is not a directory",
                self.directory.display()
            )
            .into());
        }
        loop {
            let scan = scan_directory(&self.directory)?;
            task.set_progress(scan.loaded_pieces, scan.total_pieces)
                .await;
            let progress = if scan.discovered_shares == 0 {
                String::new()
            } else {
                format!(
                    "{} of {} shares complete; {} of {} pieces loaded",
                    scan.complete_shares,
                    scan.discovered_shares,
                    scan.loaded_pieces,
                    scan.total_pieces
                )
            };
            for problem in scan.problems {
                if self.reported.insert(problem.clone()) {
                    crate::cli_common::warn(problem);
                }
            }
            for complete in scan.complete {
                if self.emitted.insert(complete.hash.clone()) {
                    complete.paper.share.verify()?;
                    task.set_message(format!(
                        "Read complete paper share {} from {} QR chunk{} ({progress})",
                        complete.paper.paper_name,
                        complete.pieces,
                        if complete.pieces == 1 { "" } else { "s" }
                    ))
                    .await;
                    return Ok(complete.paper);
                }
            }
            task.set_message(if scan.waiting.is_empty() {
                if progress.is_empty() {
                    format!(
                        "Waiting for pkiboo QR chunks in {}",
                        self.directory.display()
                    )
                } else {
                    progress
                }
            } else {
                format!("{progress}; waiting for {}", scan.waiting.join("; "))
            })
            .await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

struct Complete {
    hash: String,
    paper: PaperShare,
    pieces: usize,
}
struct Scan {
    complete: Vec<Complete>,
    waiting: Vec<String>,
    problems: Vec<String>,
    discovered_shares: usize,
    complete_shares: usize,
    loaded_pieces: usize,
    total_pieces: usize,
}
#[derive(Default)]
struct Assembly {
    pieces: usize,
    chunks: BTreeMap<usize, Vec<u8>>,
    invalid: bool,
}

fn scan_directory(directory: &Path) -> Result<Scan, Box<dyn Error>> {
    let mut groups = HashMap::<(String, String, u8, u8), Assembly>::new();
    let mut problems = Vec::new();
    let mut paths = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    paths.sort_by_key(|e| e.path());
    for entry in paths {
        let path = entry.path();
        if !path.is_file() || !is_image(&path) {
            continue;
        }
        let image = match image::open(&path) {
            Ok(i) => i.to_luma8(),
            Err(e) => {
                problems.push(format!("Could not read image {}: {e}", path.display()));
                continue;
            }
        };
        let mut prepared = rqrr::PreparedImage::prepare(image);
        let grids = prepared.detect_grids();
        if grids.is_empty() {
            problems.push(format!("Could not find a QR code in {}", path.display()));
            continue;
        }
        let mut matched = false;
        let mut decode_errors = Vec::new();
        for grid in grids {
            let mut content = Vec::new();
            match grid.decode_to(&mut content) {
                Ok(_) => {
                    let segment = match decode_qr_segment(&content) {
                        Ok(s) if s.format == PAPER_QR_FORMAT => s,
                        _ => continue,
                    };
                    matched = true;
                    if segment.piece == 0 || segment.pieces == 0 || segment.piece > segment.pieces {
                        problems.push(format!(
                            "Invalid pkiboo QR chunk numbering in {}",
                            path.display()
                        ));
                        continue;
                    }
                    let data = segment.data;
                    let group = groups
                        .entry((
                            segment.paper,
                            segment.document_hash,
                            segment.share,
                            segment.shares,
                        ))
                        .or_default();
                    if group.pieces != 0 && group.pieces != segment.pieces {
                        group.invalid = true;
                    }
                    group.pieces = segment.pieces;
                    if group
                        .chunks
                        .get(&segment.piece)
                        .is_some_and(|old| old != &data)
                    {
                        group.invalid = true;
                    } else {
                        group.chunks.insert(segment.piece, data);
                    }
                }
                Err(e) => decode_errors.push(e.to_string()),
            }
        }
        if !matched {
            problems.push(if decode_errors.is_empty() {
                format!("No pkiboo paper-share QR code found in {}", path.display())
            } else {
                format!(
                    "Could not read QR code in {}: {}",
                    path.display(),
                    decode_errors.join("; ")
                )
            });
        }
    }
    let mut complete = Vec::new();
    let mut waiting = Vec::new();
    let discovered_shares = groups.len();
    let mut loaded_pieces = 0usize;
    let mut total_pieces = 0usize;
    for ((paper_name, hash, share, _), group) in groups {
        loaded_pieces += group.chunks.len().min(group.pieces);
        total_pieces += group.pieces;
        if group.invalid {
            problems.push(format!(
                "Conflicting QR chunks found for paper {paper_name}"
            ));
            continue;
        }
        let missing = (1..=group.pieces)
            .filter(|n| !group.chunks.contains_key(n))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            waiting.push(format!(
                "paper {paper_name} chunks {}",
                missing
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            continue;
        }
        let bytes = (1..=group.pieces)
            .flat_map(|n| group.chunks[&n].clone())
            .collect::<Vec<_>>();
        if document_hash(&bytes)? != hash {
            problems.push(format!(
                "QR chunks for paper {paper_name} failed their document hash"
            ));
            continue;
        }
        match yaml_serde::from_slice::<PaperShare>(&bytes) {
            Ok(p) if p.paper_name.to_string() == paper_name && p.share.x == share => {
                complete.push(Complete {
                    hash,
                    paper: p,
                    pieces: group.pieces,
                })
            }
            _ => problems.push(format!(
                "QR chunks for paper {paper_name} do not contain a valid paper share"
            )),
        }
    }
    let complete_shares = complete.len();
    Ok(Scan {
        complete,
        waiting,
        problems,
        discovered_shares,
        complete_shares,
        loaded_pieces,
        total_pieces,
    })
}

fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "bmp"
                | "tif"
                | "tiff"
                | "webp"
                | "pnm"
                | "pgm"
                | "ppm"
        )
    })
}
pub fn document_hash(bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    Ok(
        openssl::hash::hash(openssl::hash::MessageDigest::sha256(), bytes)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        keypair::split::share::{PaperSharePlacements, recover_private_key, split_private_key},
        multihash::{HashAlgorithm, MultiHash},
        pkiboo::Paper,
        util::Name,
    };
    use qrcode::{EcLevel, QrCode, types::Color};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    fn temporary_directory(label: &str) -> PathBuf {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("pkiboo-{label}-{}-{sequence}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        directory
    }

    fn paper_share(name: &str, share: crate::keypair::split::share::ShamirShareFile) -> PaperShare {
        PaperShare {
            key_name: "root".into(),
            paper_name: Name::<Paper>::new(name.into()),
            share,
            placements: PaperSharePlacements {
                paper: Vec::new(),
                storage: Vec::new(),
            },
        }
    }

    fn qr_payloads(paper: &PaperShare) -> Vec<Vec<u8>> {
        let document = yaml_serde::to_string(paper).unwrap().into_bytes();
        let hash = document_hash(&document).unwrap();
        let pieces = document.chunks(900).count();
        document
            .chunks(900)
            .enumerate()
            .map(|(index, data)| {
                encode_qr_segment(&PaperQrSegment {
                    format: PAPER_QR_FORMAT.into(),
                    paper: paper.paper_name.to_string(),
                    document_hash: hash.clone(),
                    share: paper.share.x,
                    shares: paper.share.shamir.shares,
                    piece: index + 1,
                    pieces,
                    data: data.to_vec(),
                })
                .unwrap()
            })
            .collect()
    }

    fn write_paper_qrs(directory: &Path, paper: &PaperShare) {
        for (index, payload) in qr_payloads(paper).iter().enumerate() {
            write_qr(
                &directory.join(format!("{}-{index}.png", paper.paper_name)),
                payload,
            );
        }
    }

    fn write_qr(path: &Path, payload: &[u8]) {
        let qr = QrCode::with_error_correction_level(payload, EcLevel::M).unwrap();
        let quiet = 4usize;
        let scale = 6usize;
        let side = (qr.width() + quiet * 2) * scale;
        let mut image = image::GrayImage::from_pixel(side as u32, side as u32, image::Luma([255]));
        for y in 0..qr.width() {
            for x in 0..qr.width() {
                if qr[(x, y)] == Color::Dark {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            image.put_pixel(
                                ((x + quiet) * scale + dx) as u32,
                                ((y + quiet) * scale + dy) as u32,
                                image::Luma([0]),
                            );
                        }
                    }
                }
            }
        }
        image.save(path).unwrap();
    }

    #[test]
    fn rqrr_reports_missing_numbered_chunks() {
        let dir = std::env::temp_dir().join(format!("pkiboo-qr-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let data = b"first part";
        let hash = document_hash(b"first partsecond part").unwrap();
        let segment = PaperQrSegment {
            format: PAPER_QR_FORMAT.into(),
            paper: "six-word-paper-name-here-now".into(),
            document_hash: hash,
            share: 2,
            shares: 5,
            piece: 1,
            pieces: 2,
            data: data.to_vec(),
        };
        write_qr(&dir.join("page.png"), &encode_qr_segment(&segment).unwrap());
        let scan = scan_directory(&dir).unwrap();
        assert!(scan.complete.is_empty());
        assert!(scan.waiting.join(" ").contains("chunks 2"));
        assert_eq!(scan.discovered_shares, 1);
        assert_eq!(scan.complete_shares, 0);
        assert_eq!(scan.loaded_pieces, 1);
        assert_eq!(scan.total_pieces, 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn image_without_qr_is_reported() {
        let dir = std::env::temp_dir().join(format!("pkiboo-no-qr-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        image::GrayImage::from_pixel(200, 200, image::Luma([255]))
            .save(dir.join("blank.png"))
            .unwrap();
        let scan = scan_directory(&dir).unwrap();
        assert!(
            scan.problems
                .iter()
                .any(|p| p.contains("Could not find a QR code"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn single_paper_share_round_trips_through_qr_images() {
        let directory = temporary_directory("single-paper-round-trip");
        let shares = split_private_key(
            b"exact private key bytes",
            MultiHash::new(HashAlgorithm::SHA256, "ab".repeat(32)),
            3,
            2,
        )
        .unwrap();
        let expected = paper_share("one-two-three-four-five-six", shares[0].clone());
        write_paper_qrs(&directory, &expected);

        let scan = scan_directory(&directory).unwrap();
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        assert!(scan.waiting.is_empty(), "{:?}", scan.waiting);
        assert_eq!(scan.complete.len(), 1);
        assert_eq!(scan.complete_shares, 1);
        assert_eq!(scan.loaded_pieces, scan.total_pieces);
        scan.complete[0].paper.share.verify().unwrap();
        assert_eq!(
            yaml_serde::to_string(&scan.complete[0].paper).unwrap(),
            yaml_serde::to_string(&expected).unwrap()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn entire_recovery_set_round_trips_through_qr_images() {
        let directory = temporary_directory("recovery-set-round-trip");
        let private_key = b"exact private key bytes recovered from all QR images";
        let shares = split_private_key(
            private_key,
            MultiHash::new(HashAlgorithm::SHA256, "cd".repeat(32)),
            3,
            2,
        )
        .unwrap();
        for (index, share) in shares.into_iter().enumerate() {
            let paper = paper_share(
                &format!("paper-share-number-{}-safe-copy", index + 1),
                share,
            );
            write_paper_qrs(&directory, &paper);
        }

        let scan = scan_directory(&directory).unwrap();
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        assert!(scan.waiting.is_empty(), "{:?}", scan.waiting);
        assert_eq!(scan.complete.len(), 3);
        let decoded = scan
            .complete
            .into_iter()
            .map(|complete| complete.paper.share)
            .collect::<Vec<_>>();
        assert_eq!(recover_private_key(&decoded).unwrap(), private_key);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
