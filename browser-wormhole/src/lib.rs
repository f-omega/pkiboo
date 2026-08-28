//! Browser boundary for pkiboo's Magic Wormhole signaling client.
//!
//! WebRTC remains implemented by browser JavaScript. This crate owns the
//! browser-compatible rendezvous/mailbox adapter and exposes JSON messages to
//! JavaScript. The protocol implementation will be filled in behind this
//! stable boundary.

use wasm_bindgen::prelude::*;
use magic_wormhole::{Code, MailboxConnection, Wormhole, transfer};

#[wasm_bindgen]
pub struct SignalingAdapter {
    code: String,
    wormhole: Option<Wormhole>,
}

#[wasm_bindgen]
impl SignalingAdapter {
    /// Create an adapter for a user-entered Magic Wormhole code.
    #[wasm_bindgen(constructor)]
    pub fn new(code: String) -> Result<SignalingAdapter, JsValue> {
        if code.trim().is_empty() {
            return Err(JsValue::from_str("Magic Wormhole code cannot be empty"));
        }
        Ok(Self { code, wormhole: None })
    }

    #[wasm_bindgen(getter)]
    pub fn code(&self) -> String { self.code.clone() }

    /// Send a signaling object through the authenticated Wormhole mailbox.
    ///
    /// This is intentionally async-shaped so the final rendezvous transport
    /// can be added without changing the JavaScript integration.
    pub async fn send(&mut self, message: JsValue) -> Result<(), JsValue> {
        let value: serde_json::Value = serde_wasm_bindgen::from_value(message).map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.ensure_connected().await?;
        self.wormhole.as_mut().unwrap().send_json(&value).await.map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Receive the next signaling object from the authenticated mailbox.
    pub async fn receive(&mut self) -> Result<JsValue, JsValue> {
        self.ensure_connected().await?;
        let value: serde_json::Value = self.wormhole.as_mut().unwrap().receive_json().await.map_err(|e| JsValue::from_str(&e.to_string()))?.map_err(|e| JsValue::from_str(&e.to_string()))?;
        serde_wasm_bindgen::to_value(&value).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    async fn ensure_connected(&mut self) -> Result<(), JsValue> {
        if self.wormhole.is_some() { return Ok(()); }
        let code: Code = self.code.parse().map_err(|e| JsValue::from_str(&format!("invalid Wormhole code: {e}")))?;
        let mailbox = MailboxConnection::connect(transfer::APP_CONFIG, code, false).await.map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.wormhole = Some(Wormhole::connect(mailbox).await.map_err(|e| JsValue::from_str(&e.to_string()))?);
        Ok(())
    }
}

#[wasm_bindgen]
pub async fn create_signaling(code: String) -> Result<SignalingAdapter, JsValue> {
    SignalingAdapter::new(code)
}
