// Voice-call client. The browser holds ONE RTCPeerConnection to the server,
// which is the WebRTC peer in the middle (it forwards our audio to the other
// participants and, later, records it). The server is always the offerer, so we
// only ever answer. Signaling rides the shared chat WebSocket (see chat.ts).
import { get, writable } from "svelte/store";
import { onSignal, sendFrame, notify, joinRoom } from "./chat";
import { isMuted } from "./notify";
import { t } from "./i18n";

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
/// Live audio levels (0..1): `local` = your mic, `remote` = what's coming back
/// from the other side. Drives the call meters — and is a debugging aid: if your
/// mic bar moves but the remote one doesn't, audio isn't returning from the peer.
export const callLevels = writable<{ local: number; remote: number }>({ local: 0, remote: 0 });

/// Output routing (mostly for phones): true = loudspeaker, false = earpiece.
export const callSpeaker = writable<boolean>(false);
/// Whether the browser lets us route call audio at all (HTMLMediaElement.setSinkId).
/// Chrome/Android: yes; iOS Safari: no (it gives no web control over the route),
/// so the UI hides the toggle there instead of showing a dead button.
export const canRouteAudio =
  typeof HTMLMediaElement !== "undefined" && "setSinkId" in HTMLMediaElement.prototype;

type SinkAudio = HTMLAudioElement & { setSinkId(id: string): Promise<void> };

// Pick an output device for the requested route by matching the OS device labels
// (localized, so we match a few languages); fall back to the system default.
async function pickOutput(speaker: boolean): Promise<string | null> {
  try {
    const outs = (await navigator.mediaDevices.enumerateDevices()).filter(
      (d) => d.kind === "audiooutput",
    );
    if (outs.length === 0) return null;
    const re = speaker ? /speaker|loud|громк/i : /earpiece|handset|разговор|телефон/i;
    const hit = outs.find((d) => re.test(d.label));
    return (hit ?? outs.find((d) => d.deviceId === "default") ?? outs[0]).deviceId;
  } catch {
    return null;
  }
}

async function applyOutput(speaker: boolean): Promise<void> {
  if (!remoteAudio || !canRouteAudio) return;
  const id = await pickOutput(speaker);
  if (id == null) return;
  try {
    await (remoteAudio as SinkAudio).setSinkId(id);
  } catch {
    /* device vanished or not permitted — keep the current route */
  }
}

/// Toggle loudspeaker ⇄ earpiece for the current call (no-op without setSinkId).
export function toggleSpeaker(): void {
  const next = !get(callSpeaker);
  callSpeaker.set(next);
  void applyOutput(next);
}

let pc: RTCPeerConnection | null = null;
let localStream: MediaStream | null = null;
let remoteAudio: HTMLAudioElement | null = null;
let callId: string | null = null;
let timer: ReturnType<typeof setInterval> | null = null;
let connectTimer: ReturnType<typeof setTimeout> | null = null;

// If we don't reach a live connection within this window, surface a failure
// instead of sitting on "connecting…" forever (e.g. ICE can't traverse NAT).
const CONNECT_TIMEOUT_MS = 20000;

// ICE servers (STUN/TURN). Fetched from the server (`/api/ice`, driven by
// ZENITHAR_ICE_SERVERS) so a self-host can point calls at its own coturn without
// rebuilding the frontend. STUN lets a client behind NAT learn its own public
// address and offer it to the server — required when the server can't derive it
// via peer-reflexive. Empty (default) still works on a LAN / same network.
let rtcConfig: RTCConfiguration = { iceServers: [] };

async function loadIceServers(): Promise<void> {
  try {
    const r = await fetch("/api/ice");
    if (r.ok) {
      const servers = (await r.json()) as RTCIceServer[];
      if (Array.isArray(servers)) rtcConfig = { iceServers: servers };
    }
  } catch {
    /* keep the empty default */
  }
}

// ---- audio level metering (mic + incoming) ---------------------------------
// A Web Audio AnalyserNode per stream → RMS → `callLevels`. Drives the on-call
// meter and is a debugging aid (mic bar moves but remote doesn't = no audio back).
type ACtor = typeof AudioContext;
let meterCtxInst: AudioContext | null = null;
let localAnalyser: AnalyserNode | null = null;
let remoteAnalyser: AnalyserNode | null = null;
let meterRaf: number | null = null;

function meterCtx(): AudioContext | null {
  const Ctor: ACtor | undefined =
    window.AudioContext ?? (window as unknown as { webkitAudioContext?: ACtor }).webkitAudioContext;
  if (!Ctor) return null;
  meterCtxInst ??= new Ctor();
  if (meterCtxInst.state === "suspended") void meterCtxInst.resume();
  return meterCtxInst;
}

function analyserFor(stream: MediaStream): AnalyserNode | null {
  const ctx = meterCtx();
  if (!ctx) return null;
  const an = ctx.createAnalyser();
  an.fftSize = 256;
  ctx.createMediaStreamSource(stream).connect(an);
  return an;
}

function rmsLevel(an: AnalyserNode): number {
  // Fresh ArrayBuffer-backed view (so the type is Uint8Array<ArrayBuffer>, what
  // getByteTimeDomainData wants); allocation is negligible per frame.
  const buf = new Uint8Array(new ArrayBuffer(an.fftSize));
  an.getByteTimeDomainData(buf);
  let sum = 0;
  for (let i = 0; i < buf.length; i++) {
    const x = (buf[i] - 128) / 128;
    sum += x * x;
  }
  // RMS scaled so ordinary speech fills the meter; clamp to 0..1.
  return Math.min(1, Math.sqrt(sum / buf.length) * 2.5);
}

function startMeter(): void {
  if (meterRaf !== null) return;
  const tick = (): void => {
    callLevels.set({
      local: localAnalyser ? rmsLevel(localAnalyser) : 0,
      remote: remoteAnalyser ? rmsLevel(remoteAnalyser) : 0,
    });
    meterRaf = requestAnimationFrame(tick);
  };
  meterRaf = requestAnimationFrame(tick);
}

function stopMeter(): void {
  if (meterRaf !== null) cancelAnimationFrame(meterRaf);
  meterRaf = null;
  localAnalyser = null;
  remoteAnalyser = null;
  callLevels.set({ local: 0, remote: 0 });
}

// The server (offerer) trickles its ICE candidates immediately after the offer,
// often BEFORE our getUserMedia resolves and we've built `pc` + set the remote
// description. Buffer any early candidates and flush them once the PC is ready,
// otherwise the server's only candidate is silently dropped and ICE fails.
let pendingIce: RTCIceCandidateInit[] = [];
let remoteReady = false;

/// Start a call in a room (or accept the one ringing — same handshake: we ask
/// the server to add us, it answers with an SDP offer).
export function startCall(roomId: string): void {
  // Allowed both from idle (caller) and ringing (callee accepting) — the latter
  // is why accepting did nothing before: the guard rejected the "ringing" state.
  const st = get(callState);
  if (st !== "idle" && st !== "ringing") return;
  callState.set("connecting");
  incoming.set(null);
  if (!sendFrame({ type: "call-start", room_id: roomId })) {
    notify(get(t)("callFailed"));
    teardown();
    return;
  }
  armConnectWatchdog();
}

function armConnectWatchdog(): void {
  clearConnectWatchdog();
  connectTimer = setTimeout(() => {
    if (get(callState) !== "live") {
      notify(get(t)("callFailed"));
      hangup();
    }
  }, CONNECT_TIMEOUT_MS);
}

function clearConnectWatchdog(): void {
  if (connectTimer) clearTimeout(connectTimer);
  connectTimer = null;
}

/// Accept the currently ringing call. Switch into that room first (so an employee
/// answering a client's cross-room ring lands in the channel), then join the call.
export function acceptCall(): void {
  const inc = get(incoming);
  if (!inc) return;
  joinRoom(inc.roomId);
  startCall(inc.roomId);
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
    // No mic / denied — tell the user instead of failing silently.
    notify(get(t)("callNoMic"));
    hangup();
    return;
  }

  pc = new RTCPeerConnection(rtcConfig);
  for (const tr of localStream.getTracks()) pc.addTrack(tr, localStream);

  // Meter your own mic right away (shows capture works even before connect).
  localAnalyser = analyserFor(localStream);
  startMeter();

  pc.ontrack = (e) => {
    const stream = e.streams[0] ?? new MediaStream([e.track]);
    if (!remoteAudio) {
      remoteAudio = new Audio();
      remoteAudio.autoplay = true;
    }
    remoteAudio.srcObject = stream;
    void remoteAudio.play().catch(() => {});
    void applyOutput(get(callSpeaker)); // honor the chosen route on (re)connect
    // Meter the incoming audio — if this bar stays flat, nothing is coming back.
    remoteAnalyser = analyserFor(stream);
    startMeter();
  };
  pc.onicecandidate = (e) => {
    if (e.candidate && callId) {
      sendFrame({ type: "call-ice", call_id: callId, candidate: JSON.stringify(e.candidate) });
    }
  };
  pc.onconnectionstatechange = () => {
    const st = pc?.connectionState;
    if (st === "connected") startTimer();
    else if (st === "failed") {
      notify(get(t)("callFailed")); // ICE couldn't connect (often NAT/firewall)
      hangup();
    } else if (st === "closed" || st === "disconnected") {
      hangup();
    }
  };

  await pc.setRemoteDescription({ type: "offer", sdp });
  // Remote description is set — now early-buffered candidates can be applied.
  remoteReady = true;
  for (const c of pendingIce) {
    try {
      await pc.addIceCandidate(c);
    } catch {
      /* ignore a bad/stale candidate */
    }
  }
  pendingIce = [];

  const answer = await pc.createAnswer();
  await pc.setLocalDescription(answer);
  sendFrame({ type: "call-answer", call_id: id, sdp: answer.sdp });
}

async function onIce(id: string, candidate: string): Promise<void> {
  if (id !== callId) return;
  const cand = JSON.parse(candidate) as RTCIceCandidateInit;
  // Buffer until the PC exists AND the remote description is set; flushing too
  // early throws ("remote description was null") and loses the candidate.
  if (!pc || !remoteReady) {
    pendingIce.push(cand);
    return;
  }
  try {
    await pc.addIceCandidate(cand);
  } catch {
    /* candidate arrived before remote description; browser will retry on next */
  }
}

function startTimer(): void {
  clearConnectWatchdog();
  if (timer) return;
  callState.set("live");
  callElapsed.set(0);
  timer = setInterval(() => callElapsed.update((s) => s + 1), 1000);
}

function teardown(): void {
  clearConnectWatchdog();
  stopMeter();
  pendingIce = [];
  remoteReady = false;
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
  callSpeaker.set(false);
  callElapsed.set(0);
  incoming.set(null);
}

/// Wire the signaling frames coming off the shared chat socket. Call once at app
/// start (after `connect()`), idempotent enough for a single mount.
export function initCallSignaling(): void {
  void loadIceServers(); // refresh ICE config (STUN/TURN) before any call starts
  onSignal((f) => {
    switch (f.type) {
      case "call-ringing": {
        // Don't ring for a call we're already in.
        if (get(callState) !== "idle") return;
        const roomId = f.room_id as string;
        // A muted room (per-room mute) doesn't ring — same as its messages.
        if (isMuted(roomId)) return;
        incoming.set({
          callId: f.call_id as string,
          roomId,
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
