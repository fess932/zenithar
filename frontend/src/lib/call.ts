// Voice-call client. The browser holds ONE RTCPeerConnection to the server,
// which is the WebRTC peer in the middle (it forwards our audio to the other
// participants and, later, records it). The server is always the offerer, so we
// only ever answer. Signaling rides the shared chat WebSocket (see chat.ts).
import { get, writable } from "svelte/store";
import { onSignal, sendFrame } from "./chat";

export type CallState = "idle" | "ringing" | "connecting" | "live";

export interface CallParticipant {
  id: string;
  name: string;
}

export interface Incoming {
  callId: string;
  roomId: string;
  fromName: string;
}

export const callState = writable<CallState>("idle");
export const callParticipants = writable<CallParticipant[]>([]);
export const callElapsed = writable<number>(0); // seconds, since connected
export const callMuted = writable<boolean>(false);
export const incoming = writable<Incoming | null>(null);

let pc: RTCPeerConnection | null = null;
let localStream: MediaStream | null = null;
let remoteAudio: HTMLAudioElement | null = null;
let callId: string | null = null;
let timer: ReturnType<typeof setInterval> | null = null;

// Empty ICE list works on localhost/LAN (host candidates); the server has a
// public IP, so no TURN. A public STUN can be added later if needed.
const RTC_CONFIG: RTCConfiguration = { iceServers: [] };

/// Start a call in a room (or accept the one ringing — same handshake: we ask
/// the server to add us, it answers with an SDP offer).
export function startCall(roomId: string): void {
  if (get(callState) !== "idle") return;
  callState.set("connecting");
  incoming.set(null);
  if (!sendFrame({ type: "call-start", room_id: roomId })) {
    teardown();
  }
}

/// Accept the currently ringing call.
export function acceptCall(): void {
  const inc = get(incoming);
  if (inc) startCall(inc.roomId);
}

/// Decline a ringing call (just dismiss; we never joined).
export function declineCall(): void {
  incoming.set(null);
  callState.set("idle");
}

/// Hang up / leave the call.
export function hangup(): void {
  if (callId) sendFrame({ type: "call-leave", call_id: callId });
  teardown();
}

export function toggleMute(): void {
  if (!localStream) return;
  const enabled = localStream.getAudioTracks().some((tr) => tr.enabled);
  for (const tr of localStream.getAudioTracks()) tr.enabled = !enabled;
  callMuted.set(enabled); // was enabled → now muted
}

async function onOffer(id: string, sdp: string): Promise<void> {
  callId = id;
  try {
    localStream = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    });
  } catch {
    // No mic / denied — bail out cleanly.
    hangup();
    return;
  }

  pc = new RTCPeerConnection(RTC_CONFIG);
  for (const tr of localStream.getTracks()) pc.addTrack(tr, localStream);

  pc.ontrack = (e) => {
    if (!remoteAudio) {
      remoteAudio = new Audio();
      remoteAudio.autoplay = true;
    }
    remoteAudio.srcObject = e.streams[0] ?? new MediaStream([e.track]);
    void remoteAudio.play().catch(() => {});
  };
  pc.onicecandidate = (e) => {
    if (e.candidate && callId) {
      sendFrame({ type: "call-ice", call_id: callId, candidate: JSON.stringify(e.candidate) });
    }
  };
  pc.onconnectionstatechange = () => {
    const st = pc?.connectionState;
    if (st === "connected") startTimer();
    else if (st === "failed" || st === "closed" || st === "disconnected") hangup();
  };

  await pc.setRemoteDescription({ type: "offer", sdp });
  const answer = await pc.createAnswer();
  await pc.setLocalDescription(answer);
  sendFrame({ type: "call-answer", call_id: id, sdp: answer.sdp });
}

async function onIce(id: string, candidate: string): Promise<void> {
  if (!pc || id !== callId) return;
  try {
    await pc.addIceCandidate(JSON.parse(candidate));
  } catch {
    /* candidate arrived before remote description; browser will retry on next */
  }
}

function startTimer(): void {
  if (timer) return;
  callState.set("live");
  callElapsed.set(0);
  timer = setInterval(() => callElapsed.update((s) => s + 1), 1000);
}

function teardown(): void {
  if (timer) clearInterval(timer);
  timer = null;
  localStream?.getTracks().forEach((tr) => tr.stop());
  localStream = null;
  if (remoteAudio) {
    remoteAudio.srcObject = null;
    remoteAudio = null;
  }
  pc?.close();
  pc = null;
  callId = null;
  callState.set("idle");
  callParticipants.set([]);
  callMuted.set(false);
  callElapsed.set(0);
  incoming.set(null);
}

/// Wire the signaling frames coming off the shared chat socket. Call once at app
/// start (after `connect()`), idempotent enough for a single mount.
export function initCallSignaling(): void {
  onSignal((f) => {
    switch (f.type) {
      case "call-ringing": {
        // Don't ring for a call we're already in.
        if (get(callState) !== "idle") return;
        incoming.set({
          callId: f.call_id as string,
          roomId: f.room_id as string,
          fromName: f.from_name as string,
        });
        callState.set("ringing");
        break;
      }
      case "call-offer":
        void onOffer(f.call_id as string, f.sdp as string);
        break;
      case "call-ice":
        void onIce(f.call_id as string, f.candidate as string);
        break;
      case "call-state":
        if (f.call_id === callId) callParticipants.set(f.participants as CallParticipant[]);
        break;
      case "call-ended":
        if (f.call_id === callId || get(callState) === "ringing") teardown();
        break;
    }
  });
}
