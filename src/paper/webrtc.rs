//! WebRTC signaling over a Magic Wormhole rendezvous channel.
//!
//! The browser creates the offer and gathers its ICE candidates.  We use the
//! Wormhole only for the small, authenticated SDP exchange; bulk paper images
//! travel over the resulting WebRTC data channel.

use magic_wormhole::Wormhole;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::sync::mpsc;
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCSessionDescription};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IceCandidate {
    pub candidate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_m_line_index: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WebRtcOffer {
    #[serde(rename = "type")]
    pub kind: String,
    pub sdp: String,
    #[serde(default)]
    pub candidates: Vec<IceCandidate>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WebRtcAnswer {
    #[serde(rename = "type")]
    pub kind: String,
    pub sdp: String,
    pub candidates: Vec<IceCandidate>,
}

/// Consume one browser offer from Wormhole and produce a WebRTC answer.
///
/// This deliberately stops at signaling: the data-channel receiver is owned
/// by the paper assembler, so callers can attach their own handler when the
/// connection is ready.
pub(crate) async fn answer_offer(
    wormhole: &mut Wormhole,
    handler: Arc<dyn PeerConnectionEventHandler>,
) -> Result<(impl PeerConnection, WebRtcAnswer), Box<dyn Error>> {
    let message: WebRtcOffer = wormhole
        .receive_json()
        .await?
        .map_err(|e| format!("invalid WebRTC offer: {e}"))?;
    if message.kind != "webrtc-offer" {
        return Err(format!("expected webrtc-offer, received {}", message.kind).into());
    }
    let offer = RTCSessionDescription::offer(message.sdp)?;
    let pc = PeerConnectionBuilder::new()
        .with_handler(handler)
        .with_udp_addrs(vec!["0.0.0.0:0"])
        .build()
        .await?;
    pc.set_remote_description(offer).await?;
    let answer = pc.create_answer(None).await?;
    pc.set_local_description(answer).await?;
    let local = pc.local_description().await.ok_or("missing local WebRTC description")?;
    let response = WebRtcAnswer {
        kind: "webrtc-answer".into(),
        sdp: local.sdp,
        candidates: Vec::new(),
    };
    wormhole.send_json(&response).await?;
    Ok((pc, response))
}

/// Candidate events are sent separately so a browser can apply trickle ICE.
/// The initial answer still contains the gathered SDP for compatibility.
pub(crate) async fn answer_offer_with_trickle(
    wormhole: &mut Wormhole,
) -> Result<(impl PeerConnection, mpsc::Receiver<IceCandidate>), Box<dyn Error>> {
    let (tx, rx) = mpsc::channel(32);
    let handler = Arc::new(TrickleHandler { sender: tokio::sync::Mutex::new(tx) });
    let (pc, _) = answer_offer(wormhole, handler.clone()).await?;
    Ok((pc, rx))
}

pub(crate) async fn receive_images(wormhole: &mut Wormhole) -> Result<Vec<(String, Vec<u8>)>, Box<dyn Error>> {
    let (tx, rx) = oneshot::channel();
    let handler = Arc::new(ChannelHandler { sender: tokio::sync::Mutex::new(Some(tx)) });
    let _pc = answer_offer(wormhole, handler).await?;
    let channel = rx.await.map_err(|_| "WebRTC data channel was not opened")?;
    let mut files = Vec::new();
    let mut current: Option<(String, Vec<u8>)> = None;
    while let Some(event) = channel.poll().await {
        match event {
            DataChannelEvent::OnMessage(message) => {
                if message.is_string {
                    let text = String::from_utf8_lossy(&message.data);
                    let value: serde_json::Value = serde_json::from_str(&text)?;
                    match value.get("type").and_then(|v| v.as_str()) {
                        Some("file-begin") => {
                            let name = value.get("name").and_then(|v| v.as_str()).unwrap_or("image").to_string();
                            current = Some((name, Vec::new()));
                        }
                        Some("file-end") => {
                            if let Some(file) = current.take() { files.push(file); channel.send_text("{\"type\":\"file-ack\"}").await?; }
                        }
                        Some("session-end") => break,
                        _ => {}
                    }
                } else if let Some((_, bytes)) = current.as_mut() {
                    bytes.extend_from_slice(&message.data);
                }
            }
            DataChannelEvent::OnClose => break,
            _ => {}
        }
    }
    Ok(files)
}

struct ChannelHandler { sender: tokio::sync::Mutex<Option<oneshot::Sender<Arc<dyn DataChannel>>>> }
#[async_trait::async_trait]
impl PeerConnectionEventHandler for ChannelHandler {
    async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
        if channel.label().await.ok().as_deref() == Some("pkiboo-paper-images") {
            if let Some(sender) = self.sender.lock().await.take() { let _ = sender.send(channel); }
        }
    }
}

struct TrickleHandler { sender: tokio::sync::Mutex<mpsc::Sender<IceCandidate>> }

#[async_trait::async_trait]
impl PeerConnectionEventHandler for TrickleHandler {
    async fn on_ice_candidate(&self, event: webrtc::peer_connection::RTCPeerConnectionIceEvent) {
        if let Ok(candidate) = event.candidate.to_json() {
            let value = IceCandidate {
                candidate: candidate.candidate,
                sdp_mid: candidate.sdp_mid,
                sdp_m_line_index: candidate.sdp_mline_index,
            };
            let _ = self.sender.lock().await.send(value).await;
        }
    }
}
