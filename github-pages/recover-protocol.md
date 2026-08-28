# Browser recovery protocol

`recover.html` is a static GitHub Pages client. The native pkiboo ceremony
creates a Magic Wormhole code and the page joins it. The yet-to-be-added
Wormhole adapter must provide:

```js
const signaling = await createMagicWormholeSignaling(code);
await signaling.send({ type: "webrtc-offer", sdp, candidates });
const message = await signaling.receive();
```

The adapter carries only JSON signaling messages. After the offer/answer and
ICE candidates are exchanged, `PkibooWebRtcSender` opens the ordered
`pkiboo-paper-images` data channel. Its application messages are:

- `{"type":"file-begin","name":string,"size":number}`
- binary data-channel messages containing file bytes
- `{"type":"file-end"}`
- native acknowledgement: `{"type":"ack"}` or `{"type":"error","message":string}`
- `{"type":"session-end"}`

The page can stage zero or more images, including repeated mobile camera
captures, and sends them all over one WebRTC session. The browser-side helper
does not trust filenames for filesystem operations; pkiboo treats them as
display labels only.
