import { PkibooWebRtcSender } from "./webrtc-sender.js";

const status = document.querySelector("#status");
const ready = document.querySelector("#ready");
const transfer = document.querySelector("#transfer");
const picker = document.querySelector("#image");
const sendButton = document.querySelector("#send");
const selection = document.querySelector("#selection");
const progress = document.querySelector("#progress");
let selected = [];

function wormholeCode() {
  const params = new URLSearchParams(location.hash.slice(1));
  return params.get("code");
}

function fail(error) {
  status.className = "error";
  status.textContent = error?.message || String(error);
  ready.hidden = false;
  picker.disabled = false;
  sendButton.disabled = selected.length === 0;
}

async function send(files, code) {
  ready.hidden = true;
  transfer.hidden = false;
  document.querySelector("#filename").textContent = `${files.length} image${files.length === 1 ? "" : "s"}`;
  status.textContent = "Joining the encrypted Wormhole…";
  if (!window.createMagicWormholeSignaling) {
    throw new Error("This page needs the pkiboo Magic Wormhole signaling adapter.");
  }
  const signaling = await window.createMagicWormholeSignaling(code);
  status.textContent = "Establishing the WebRTC data channel…";
  const sender = await PkibooWebRtcSender.connect(signaling, window.PKIBOO_RTC_CONFIG || {});
  status.textContent = "Sending images…";
  await sender.sendFiles(files, value => { progress.value = value; });
  sender.close();
  progress.value = 100;
  status.className = "success";
  status.textContent = "✓ Image sent. You may close this page.";
}

try {
  const code = wormholeCode();
  if (!code) throw new Error("This link does not contain a Wormhole code. Start the recovery command again and open the URL it displays.");
  document.querySelector("#code").textContent = code;
  ready.hidden = false;
  status.textContent = "Choose a clear image containing one paper-share QR code.";
  picker.addEventListener("change", () => {
    selected.push(...Array.from(picker.files || []));
    picker.value = "";
    selection.textContent = `${selected.length} image${selected.length === 1 ? "" : "s"} ready to send.`;
    sendButton.disabled = selected.length === 0;
  });
  sendButton.addEventListener("click", () => {
    if (selected.length === 0) return;
    picker.disabled = true;
    sendButton.disabled = true;
    send(selected, code).catch(fail);
  });
} catch (error) {
  fail(error);
}
