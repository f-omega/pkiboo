//! Asynchronous ingestion of numbered paper-share QR chunks from image files.
use super::assembler::{AssemblyEvent, CompletedShare, ShareAssembler};
pub use super::assembler::{PAPER_QR_FORMAT, decode_qr_segment, document_hash};
#[cfg(test)]
use super::assembler::{PaperQrSegment, encode_qr_segment};
use crate::{keypair::split::share::PaperShare, ui::Task};
use std::{
    collections::{HashSet, VecDeque},
    error::Error,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Default)]
pub struct PaperInput {
    directory: Option<PathBuf>,
    assembler: ShareAssembler,
    pending: VecDeque<CompletedShare>,
    reported: HashSet<String>,
    #[cfg(feature = "wormhole")]
    magic_wormhole: bool,
}

impl PaperInput {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self {
            directory,
            ..Self::default()
        }
    }

    #[cfg(feature = "wormhole")]
    pub fn enable_magic_wormhole(&mut self, enabled: bool) {
        self.magic_wormhole = enabled;
    }

    /// Wait for the next complete, commitment-valid paper share. Repeated calls
    /// yield additional complete documents, making this usable beside media or
    /// future remote-share waiters in a ceremony.
    pub async fn next_share<T: Task>(&mut self, task: &T) -> Result<PaperShare, Box<dyn Error>> {
        if let Some(directory) = &self.directory
            && !directory.is_dir()
        {
            return Err(format!("Paper input {} is not a directory", directory.display()).into());
        }
        #[cfg(feature = "wormhole")]
        let mut wormhole_receive = self
            .magic_wormhole
            .then(|| Box::pin(super::wormhole::receive_image(task.clone())));

        loop {
            if let Some(complete) = self.pending.pop_front() {
                complete.paper.share.verify()?;
                task.set_message(format!(
                    "Read complete paper share {} from {} QR chunk{}",
                    complete.paper.paper_name,
                    complete.pieces,
                    if complete.pieces == 1 { "" } else { "s" }
                ))
                .await;
                return Ok(complete.paper);
            }
            let scan = if let Some(directory) = &self.directory {
                scan_directory(directory, &mut self.assembler)?
            } else {
                let progress = self.assembler.progress();
                Scan {
                    complete: Vec::new(),
                    waiting: self.assembler.waiting(),
                    problems: Vec::new(),
                    discovered_shares: progress.discovered_shares,
                    complete_shares: progress.complete_shares,
                    loaded_pieces: progress.loaded_pieces,
                    total_pieces: progress.total_pieces,
                }
            };
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
            self.pending.extend(scan.complete);
            if let Some(complete) = self.pending.pop_front() {
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
            task.set_message(if scan.waiting.is_empty() {
                if progress.is_empty() {
                    self.directory.as_ref().map_or_else(
                        || "Waiting for a paper-share image".into(),
                        |directory| {
                            format!("Waiting for pkiboo QR chunks in {}", directory.display())
                        },
                    )
                } else {
                    progress
                }
            } else {
                format!("{progress}; waiting for {}", scan.waiting.join("; "))
            })
            .await;
            #[cfg(feature = "wormhole")]
            if let Some(receive) = wormhole_receive.as_mut() {
                tokio::select! {
                    received = receive => {
                        match received {
                            Ok(received) => {
                                for image in received.0 {
                                    let result = ingest_image_bytes(
                                        &image.bytes,
                                        &format!("Magic Wormhole image {}", image.filename),
                                        &mut self.assembler,
                                    );
                                    self.pending.extend(result.complete);
                                    for problem in result.problems {
                                        if self.reported.insert(problem.clone()) {
                                            crate::cli_common::warn(problem);
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                let warning = format!(
                                    "Magic Wormhole receiver stopped: {error}. Paper input is still waiting; creating a new link"
                                );
                                if self.reported.insert(warning.clone()) {
                                    crate::cli_common::warn(warning);
                                }
                            }
                        }
                        wormhole_receive = Some(Box::pin(super::wormhole::receive_image(task.clone())));
                        continue;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
                continue;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

struct Scan {
    complete: Vec<CompletedShare>,
    waiting: Vec<String>,
    problems: Vec<String>,
    discovered_shares: usize,
    complete_shares: usize,
    loaded_pieces: usize,
    total_pieces: usize,
}

#[derive(Default)]
struct ImageIngest {
    complete: Vec<CompletedShare>,
    problems: Vec<String>,
}

fn ingest_image_bytes(bytes: &[u8], source: &str, assembler: &mut ShareAssembler) -> ImageIngest {
    let image = match image::load_from_memory(bytes) {
        Ok(image) => image.to_luma8(),
        Err(error) => {
            return ImageIngest {
                problems: vec![format!("Could not read {source}: {error}")],
                ..ImageIngest::default()
            };
        }
    };
    let (width, height) = image.dimensions();
    let mut scanner = zbar_rust::ZBarImageScanner::new();
    // Only run the QR decoder. Besides avoiding wasted work, this prevents
    // unrelated linear-barcode decoders from emitting warnings for dense QR
    // images.
    for (symbol, enabled) in [
        (zbar_rust::ZBarSymbolType::ZBarNone, 0),
        (zbar_rust::ZBarSymbolType::ZBarQRCode, 1),
    ] {
        if let Err(error) = scanner.set_config(
            symbol,
            zbar_rust::ZBarConfig::ZBarCfgEnable,
            enabled,
        ) {
            return ImageIngest {
                problems: vec![format!("Could not configure QR scanning: {error}")],
                ..ImageIngest::default()
            };
        }
    }
    // zbar-rust predates ZBAR_CFG_BINARY and calls its ordinal (4)
    // `ZBarCfgNum`. Current ZBar uses that value to disable text recoding,
    // which is essential because pkiboo QR payloads are arbitrary BSON bytes.
    if let Err(error) = scanner.set_config(
        zbar_rust::ZBarSymbolType::ZBarQRCode,
        zbar_rust::ZBarConfig::ZBarCfgNum,
        1,
    ) {
        return ImageIngest {
            problems: vec![format!("Could not configure binary QR scanning: {error}")],
            ..ImageIngest::default()
        };
    }
    let symbols = match scanner.scan_y800(image.into_raw(), width, height) {
        Ok(symbols) => symbols,
        Err(error) => {
            return ImageIngest {
                problems: vec![format!("Could not scan QR codes in {source}: {error}")],
                ..ImageIngest::default()
            };
        }
    };
    if symbols.is_empty() {
        return ImageIngest {
            problems: vec![format!("Could not find a QR code in {source}")],
            ..ImageIngest::default()
        };
    }

    let mut result = ImageIngest::default();
    let mut matched = false;
    for symbol in symbols {
        let segment = match decode_qr_segment(&symbol.data) {
            Ok(segment) if segment.format == PAPER_QR_FORMAT => segment,
            _ => continue,
        };
        matched = true;
        match assembler.ingest(segment) {
            AssemblyEvent::ShareCompleted(share) => result.complete.push(share),
            AssemblyEvent::Problem(problem) => {
                result.problems.push(format!("{source}: {problem}"))
            }
            AssemblyEvent::PieceAccepted | AssemblyEvent::DuplicateIgnored => {}
        }
    }
    if !matched {
        result
            .problems
            .push(format!("No pkiboo paper-share QR code found in {source}"));
    }
    result
}

fn scan_directory(
    directory: &Path,
    assembler: &mut ShareAssembler,
) -> Result<Scan, Box<dyn Error>> {
    let mut problems = Vec::new();
    let mut complete = Vec::new();
    let mut paths = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    paths.sort_by_key(|e| e.path());
    for entry in paths {
        let path = entry.path();
        if !path.is_file() || !is_image(&path) {
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                problems.push(format!("Could not read image {}: {error}", path.display()));
                continue;
            }
        };
        let result = ingest_image_bytes(&bytes, &format!("image {}", path.display()), assembler);
        complete.extend(result.complete);
        problems.extend(result.problems);
    }
    let progress = assembler.progress();
    Ok(Scan {
        complete,
        waiting: assembler.waiting(),
        problems,
        discovered_shares: progress.discovered_shares,
        complete_shares: progress.complete_shares,
        loaded_pieces: progress.loaded_pieces,
        total_pieces: progress.total_pieces,
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
    fn zbar_reports_missing_numbered_chunks() {
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
        let scan = scan_directory(&dir, &mut ShareAssembler::default()).unwrap();
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
        let scan = scan_directory(&dir, &mut ShareAssembler::default()).unwrap();
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

        let scan = scan_directory(&directory, &mut ShareAssembler::default()).unwrap();
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

        let scan = scan_directory(&directory, &mut ShareAssembler::default()).unwrap();
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
