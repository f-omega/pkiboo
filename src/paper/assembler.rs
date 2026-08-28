//! Source-independent assembly of numbered paper-share QR segments.
use crate::keypair::split::share::PaperShare;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
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

pub fn decode_qr_segment(bytes: &[u8]) -> Result<PaperQrSegment, Box<dyn Error>> {
    Ok(bson::deserialize_from_slice(bytes)?)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssemblyProgress {
    pub discovered_shares: usize,
    pub complete_shares: usize,
    pub loaded_pieces: usize,
    pub total_pieces: usize,
}

pub struct CompletedShare {
    pub paper: PaperShare,
    pub pieces: usize,
}

pub enum AssemblyEvent {
    PieceAccepted,
    DuplicateIgnored,
    ShareCompleted(CompletedShare),
    Problem(String),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DocumentId {
    paper: String,
    hash: String,
    share: u8,
    shares: u8,
}

#[derive(Default)]
struct PartialShare {
    pieces: usize,
    chunks: BTreeMap<usize, Vec<u8>>,
    invalid: bool,
    complete: bool,
}

#[derive(Default)]
pub struct ShareAssembler {
    documents: HashMap<DocumentId, PartialShare>,
}

impl ShareAssembler {
    pub fn ingest(&mut self, segment: PaperQrSegment) -> AssemblyEvent {
        if segment.format != PAPER_QR_FORMAT {
            return AssemblyEvent::Problem("QR code is not a pkiboo paper share".into());
        }
        if segment.piece == 0 || segment.pieces == 0 || segment.piece > segment.pieces {
            return AssemblyEvent::Problem(format!(
                "Invalid QR chunk numbering for paper {}",
                segment.paper
            ));
        }

        let id = DocumentId {
            paper: segment.paper,
            hash: segment.document_hash,
            share: segment.share,
            shares: segment.shares,
        };
        let document = self.documents.entry(id.clone()).or_default();
        if document.pieces != 0 && document.pieces != segment.pieces {
            document.invalid = true;
            return AssemblyEvent::Problem(format!(
                "Conflicting QR chunk totals found for paper {}",
                id.paper
            ));
        }
        document.pieces = segment.pieces;
        if let Some(old) = document.chunks.get(&segment.piece) {
            if old == &segment.data {
                return AssemblyEvent::DuplicateIgnored;
            }
            document.invalid = true;
            return AssemblyEvent::Problem(format!(
                "Conflicting QR chunk {} found for paper {}",
                segment.piece, id.paper
            ));
        }
        if document.invalid || document.complete {
            return AssemblyEvent::DuplicateIgnored;
        }
        document.chunks.insert(segment.piece, segment.data);
        if document.chunks.len() != document.pieces {
            return AssemblyEvent::PieceAccepted;
        }

        let bytes = (1..=document.pieces)
            .flat_map(|piece| document.chunks[&piece].clone())
            .collect::<Vec<_>>();
        if !matches!(document_hash(&bytes), Ok(hash) if hash == id.hash) {
            document.invalid = true;
            return AssemblyEvent::Problem(format!(
                "QR chunks for paper {} failed their document hash",
                id.paper
            ));
        }
        match yaml_serde::from_slice::<PaperShare>(&bytes) {
            Ok(paper) if paper.paper_name.to_string() == id.paper && paper.share.x == id.share => {
                document.complete = true;
                AssemblyEvent::ShareCompleted(CompletedShare {
                    paper,
                    pieces: document.pieces,
                })
            }
            _ => {
                document.invalid = true;
                AssemblyEvent::Problem(format!(
                    "QR chunks for paper {} do not contain a valid paper share",
                    id.paper
                ))
            }
        }
    }

    pub fn progress(&self) -> AssemblyProgress {
        AssemblyProgress {
            discovered_shares: self.documents.len(),
            complete_shares: self.documents.values().filter(|d| d.complete).count(),
            loaded_pieces: self
                .documents
                .values()
                .map(|d| d.chunks.len().min(d.pieces))
                .sum(),
            total_pieces: self.documents.values().map(|d| d.pieces).sum(),
        }
    }

    pub fn waiting(&self) -> Vec<String> {
        self.documents
            .iter()
            .filter(|(_, document)| !document.complete && !document.invalid)
            .filter_map(|(id, document)| {
                let missing = (1..=document.pieces)
                    .filter(|piece| !document.chunks.contains_key(piece))
                    .map(|piece| piece.to_string())
                    .collect::<Vec<_>>();
                (!missing.is_empty())
                    .then(|| format!("paper {} chunks {}", id.paper, missing.join(", ")))
            })
            .collect()
    }
}

pub fn document_hash(bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    Ok(
        openssl::hash::hash(openssl::hash::MessageDigest::sha256(), bytes)?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        keypair::split::share::{PaperSharePlacements, split_private_key},
        multihash::{HashAlgorithm, MultiHash},
        pkiboo::Paper,
        util::Name,
    };

    #[test]
    fn assembles_out_of_order_segments_without_an_image_backend() {
        let share = split_private_key(
            b"private key",
            MultiHash::new(HashAlgorithm::SHA256, "ab".repeat(32)),
            3,
            2,
        )
        .unwrap()
        .remove(0);
        let paper = PaperShare {
            key_name: "root".into(),
            paper_name: Name::<Paper>::new("calm-blue-otter-1".into()),
            share,
            placements: PaperSharePlacements {
                paper: Vec::new(),
                storage: Vec::new(),
            },
        };
        let bytes = yaml_serde::to_string(&paper).unwrap().into_bytes();
        let hash = document_hash(&bytes).unwrap();
        let chunks = bytes.chunks(bytes.len().div_ceil(3)).collect::<Vec<_>>();
        let segment = |index: usize| PaperQrSegment {
            format: PAPER_QR_FORMAT.into(),
            paper: paper.paper_name.to_string(),
            document_hash: hash.clone(),
            share: paper.share.x,
            shares: paper.share.shamir.shares,
            piece: index + 1,
            pieces: chunks.len(),
            data: chunks[index].to_vec(),
        };

        let mut assembler = ShareAssembler::default();
        assert!(matches!(
            assembler.ingest(segment(1)),
            AssemblyEvent::PieceAccepted
        ));
        assert!(matches!(
            assembler.ingest(segment(1)),
            AssemblyEvent::DuplicateIgnored
        ));
        assert!(matches!(
            assembler.ingest(segment(0)),
            AssemblyEvent::PieceAccepted
        ));
        let progress = assembler.progress();
        assert_eq!(progress.discovered_shares, 1);
        assert_eq!(progress.loaded_pieces, 2);
        assert_eq!(progress.total_pieces, 3);

        let AssemblyEvent::ShareCompleted(completed) = assembler.ingest(segment(2)) else {
            panic!("final segment did not complete the share");
        };
        assert_eq!(
            completed.paper.paper_name.to_string(),
            paper.paper_name.to_string()
        );
        assert_eq!(assembler.progress().complete_shares, 1);
    }
}
