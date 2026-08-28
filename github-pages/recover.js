import { PkibooWebRtcSender } from "./webrtc-sender.js";

const status = document.querySelector("#status");
const ready = document.querySelector("#ready");
const transfer = document.querySelector("#transfer");
const picker = document.querySelector("#image");
const sendButton = document.querySelector("#send");
const selection = document.querySelector("#selection");
const progress = document.querySelector("#progress");
const camera = document.querySelector("#camera");
const cameraStart = document.querySelector("#camera-start");
const cameraCapture = document.querySelector("#camera-capture");
const cameraFrame = document.querySelector("#camera-frame");
let selected = [];
let cameraStream;

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
  cameraStart.addEventListener("click", async () => {
    try {
      cameraStream = await navigator.mediaDevices.getUserMedia({video: {facingMode: {ideal: "environment"}}, audio: false});
      camera.srcObject = cameraStream;
      camera.hidden = false;
      cameraStart.hidden = true;
      cameraCapture.hidden = false;
      status.textContent = "Point the camera at a paper-share QR code, then capture it.";
    } catch (error) { fail(new Error(`Camera access failed: ${error.message || error}`)); }
  });
  cameraCapture.addEventListener("click", () => {
    if (!camera.videoWidth) return;
    cameraFrame.width = camera.videoWidth;
    cameraFrame.height = camera.videoHeight;
    cameraFrame.getContext("2d").drawImage(camera, 0, 0);
    cameraFrame.toBlob(blob => {
      if (!blob) return;
      selected.push(new File([blob], `camera-${selected.length + 1}.jpg`, {type: "image/jpeg"}));
      selection.textContent = `${selected.length} image${selected.length === 1 ? "" : "s"} ready to send.`;
      sendButton.disabled = false;
    }, "image/jpeg", 0.95);
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
