// WebRTC image sender used by recover.html.
//
// `signaling` is intentionally tiny: it must provide
//   send(object) -> Promise<void>
//   receive() -> Promise<object>
// The Magic Wormhole WASM adapter will implement those two methods. Wormhole
// authenticates this SDP exchange; WebRTC then carries the image bytes.

export class PkibooWebRtcSender {
  constructor(channel, peer) {
    this.channel = channel;
    this.peer = peer;
    this.acks = [];
    channel.addEventListener("message", event => {
      if (typeof event.data !== "string") return;
      try {
        const message = JSON.parse(event.data);
        if (message.type === "ack" || message.type === "error") {
          const waiter = this.acks.shift();
          if (waiter) (message.type === "ack" ? waiter.resolve(message) : waiter.reject(new Error(message.message)));
        }
      } catch (_) {
        // Ignore non-protocol text from a peer.
      }
    });
  }

  static async connect(signaling, rtcConfig = {}) {
    const peer = new RTCPeerConnection(rtcConfig);
    const channel = peer.createDataChannel("pkiboo-paper-images", {ordered: true});
    const candidates = [];
    peer.onicecandidate = event => { if (event.candidate) candidates.push(event.candidate.toJSON()); };
    await peer.setLocalDescription(await peer.createOffer());
    await waitForIce(peer);
    await signaling.send({type: "webrtc-offer", sdp: peer.localDescription.sdp, candidates});
    let answer;
    while (!answer) {
      const message = await signaling.receive();
      if (message.type === "webrtc-answer") answer = message;
      else if (message.type === "error") throw new Error(message.message || "Signaling failed");
    }
    await peer.setRemoteDescription({type: "answer", sdp: answer.sdp});
    for (const candidate of answer.candidates || []) await peer.addIceCandidate(candidate);
    await new Promise((resolve, reject) => {
      channel.addEventListener("open", resolve, {once: true});
      channel.addEventListener("close", () => reject(new Error("WebRTC channel closed")), {once: true});
    });
    return new PkibooWebRtcSender(channel, peer);
  }

  async sendFiles(files, onProgress = () => {}) {
    const total = files.reduce((sum, file) => sum + file.size, 0);
    let sent = 0;
    for (const file of files) {
      await this.sendControl({type: "file-begin", name: file.name, size: file.size});
      const reader = file.stream().getReader();
      while (true) {
        const {done, value} = await reader.read();
        if (done) break;
        await this.sendBinary(value);
        sent += value.byteLength;
        onProgress(total === 0 ? 100 : Math.round(sent * 100 / total));
      }
      await this.sendControl({type: "file-end"});
      await this.waitForAck();
    }
    await this.sendControl({type: "session-end"});
  }

  sendBinary(bytes) {
    return new Promise((resolve, reject) => {
      const send = () => {
        try { this.channel.send(bytes); resolve(); } catch (error) { reject(error); }
      };
      if (this.channel.bufferedAmount < 4 * 1024 * 1024) send();
      else {
        this.channel.bufferedAmountLowThreshold = 1024 * 1024;
        this.channel.addEventListener("bufferedamountlow", send, {once: true});
      }
    });
  }

  sendControl(message) {
    this.channel.send(JSON.stringify(message));
  }

  waitForAck() {
    return new Promise((resolve, reject) => this.acks.push({resolve, reject}));
  }

  close() { this.channel.close(); this.peer.close(); }
}

function waitForIce(peer) {
  if (peer.iceGatheringState === "complete") return Promise.resolve();
  return new Promise(resolve => peer.addEventListener("icegatheringstatechange", () => {
    if (peer.iceGatheringState === "complete") resolve();
  }));
}
