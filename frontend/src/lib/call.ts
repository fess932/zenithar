// Voice-call client. The browser holds ONE RTCPeerConnection to the server,
// which is the WebRTC peer in the middle (it forwards our audio to the other
// participants and, later, records it). The server is always the offerer, so we
// only ever answer. Signaling rides the shared chat WebSocket (see chat.ts).
import { get, writable } from "svelte/store";
import { onSignal, sendFrame, notify, joinRoom } from "./chat";
import { isMuted } from "./notify";
import { getMicStream } from "./mic";
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

/// Output volume for incoming call audio (0..1), remembered across calls.
const VOL_KEY = "zenithar.callVolume";
function initialVolume(): number {
  try {
    const raw = localStorage.getItem(VOL_KEY);
    const v = raw == null ? 1 : Number(raw);
    return Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 1;
  } catch {
    return 1;
  }
}
export const callVolume = writable<number>(initialVolume());

/// Set the call output volume (0..1) for the current and future audio; persisted.
export function setVolume(v: number): void {
  const vol = Math.min(1, Math.max(0, v));
  callVolume.set(vol);
  try {
    localStorage.setItem(VOL_KEY, String(vol));
  } catch {
    /* private mode / storage disabled — volume just won't persist */
  }
  for (const el of remoteAudios.values()) el.volume = vol;
}

/// Mic input gain (0..MIC_MAX). 1 = normal; above 1 BOOSTS the mic past 100%
/// (via a Web Audio GainNode on the outgoing stream), which HTMLMediaElement
/// volume can't do. Remembered across calls.
export const MIC_MAX = 2;
const MIC_KEY = "zenithar.micGain";
function initialMicGain(): number {
  try {
    const raw = localStorage.getItem(MIC_KEY);
    const v = raw == null ? 1 : Number(raw);
    return Number.isFinite(v) ? Math.min(MIC_MAX, Math.max(0, v)) : 1;
  } catch {
    return 1;
  }
}
export const micGain = writable<number>(initialMicGain());
// Live gain node in the outgoing chain (set up per call); null between calls.
let micGainNode: GainNode | null = null;
let micSource: MediaStreamAudioSourceNode | null = null;

/// Set the mic gain (0..MIC_MAX) for the current and future calls; persisted.
export function setMicGain(v: number): void {
  const g = Math.min(MIC_MAX, Math.max(0, v));
  micGain.set(g);
  try {
    localStorage.setItem(MIC_KEY, String(g));
  } catch {
    /* private mode / storage disabled — just won't persist */
  }
  if (micGainNode) micGainNode.gain.value = g;
}

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
  if (!canRouteAudio) return;
  const id = await pickOutput(speaker);
  if (id == null) return;
  for (const el of remoteAudios.values()) {
    try {
      await (el as SinkAudio).setSinkId(id);
    } catch {
      /* device vanished or not permitted — keep the current route */
    }
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
// One <audio> per incoming track (a group call forwards each speaker on its own
// track), keyed by track id.
const remoteAudios = new Map<string, HTMLAudioElement>();
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
// One analyser per incoming track; the remote meter shows the loudest.
const remoteAnalysers = new Map<string, AnalyserNode>();
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

/// Route the raw mic through a GainNode so it can be boosted past 100%, and
/// return the stream to actually SEND. Falls back to the raw mic if Web Audio
/// isn't available (gain control is then a no-op).
function micPipeline(input: MediaStream): MediaStream {
  const ctx = meterCtx();
  if (!ctx) return input;
  try {
    micSource = ctx.createMediaStreamSource(input);
    micGainNode = ctx.createGain();
    micGainNode.gain.value = get(micGain);
    const dest = ctx.createMediaStreamDestination();
    micSource.connect(micGainNode).connect(dest);
    return dest.stream;
  } catch {
    micSource = null;
    micGainNode = null;
    return input;
  }
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
  // Sample ~every 3rd frame (~20fps): the bars still look smooth but we avoid a
  // 60fps store write (→ Call.svelte re-render) for the whole call. Cheaper on CPU
  // and battery, especially on phones.
  let frame = 0;
  const tick = (): void => {
    meterRaf = requestAnimationFrame(tick);
    if (frame++ % 3 !== 0) return;
    let remote = 0;
    for (const an of remoteAnalysers.values()) remote = Math.max(remote, rmsLevel(an));
    callLevels.set({
      local: localAnalyser ? rmsLevel(localAnalyser) : 0,
      remote,
    });
  };
  meterRaf = requestAnimationFrame(tick);
}

function stopMeter(): void {
  if (meterRaf !== null) cancelAnimationFrame(meterRaf);
  meterRaf = null;
  localAnalyser = null;
  remoteAnalysers.clear();
  callLevels.set({ local: 0, remote: 0 });
}

// ---- incoming-call ringtone -------------------------------------------------
// A synthesized "ring-ring" repeated on a loop while a call is ringing, so an
// incoming call is audible, not just a toast. Web Audio (no asset, offline-ok);
// reuses the meter's AudioContext. Autoplay policy may keep it silent until the
// page has had a user gesture — the visual ring still shows.
let ringTimer: ReturnType<typeof setInterval> | null = null;
let ringStopAt = 0;

function ringBurst(): void {
  const ctx = meterCtx();
  if (!ctx) return;
  const now = ctx.currentTime;
  // Two short two-tone chirps = one "ring-ring".
  const chirp = (at: number): void => {
    for (const freq of [480, 620]) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, now + at);
      gain.gain.exponentialRampToValueAtTime(0.12, now + at + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + at + 0.4);
      osc.connect(gain).connect(ctx.destination);
      osc.start(now + at);
      osc.stop(now + at + 0.42);
    }
  };
  chirp(0);
  chirp(0.55);
}

function startRing(): void {
  if (ringTimer !== null) return;
  ringStopAt = Date.now() + 45_000; // safety cap if signaling never resolves
  ringBurst();
  ringTimer = setInterval(() => {
    if (Date.now() > ringStopAt) {
      stopRing();
      return;
    }
    ringBurst();
  }, 2500);
}

function stopRing(): void {
  if (ringTimer !== null) clearInterval(ringTimer);
  ringTimer = null;
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
  stopRing();
  joinRoom(inc.roomId);
  startCall(inc.roomId);
}

/// Decline a ringing call (just dismiss; we never joined).
export function declineCall(): void {
  stopRing();
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
    localStream = await getMicStream();
  } catch {
    // No mic / denied — tell the user instead of failing silently.
    notify(get(t)("callNoMic"));
    hangup();
    return;
  }

  pc = new RTCPeerConnection(rtcConfig);
  // Send the mic through the gain stage (so it can be boosted past 100%); the
  // pipeline falls back to the raw mic if Web Audio is unavailable.
  const sendStream = micPipeline(localStream);
  for (const tr of sendStream.getTracks()) pc.addTrack(tr, sendStream);

  // Meter the outgoing (boosted) mic — shows capture works + the effect of gain.
  localAnalyser = analyserFor(sendStream);
  startMeter();

  pc.ontrack = (e) => {
    // Each incoming track is one other participant — play them all.
    const stream = e.streams[0] ?? new MediaStream([e.track]);
    const el = new Audio();
    el.autoplay = true;
    el.srcObject = stream;
    el.volume = get(callVolume); // honor the chosen volume on (re)connect
    void el.play().catch(() => {});
    remoteAudios.set(e.track.id, el);
    void applyOutput(get(callSpeaker)); // honor the chosen route on (re)connect
    // Meter the incoming audio — if every bar stays flat, nothing is coming back.
    const an = analyserFor(stream);
    if (an) remoteAnalysers.set(e.track.id, an);
    startMeter();
    e.track.onended = () => {
      el.srcObject = null;
      remoteAudios.delete(e.track.id);
      remoteAnalysers.delete(e.track.id);
    };
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
      notify(get(t)("callFailed")); // ICE gave up (often NAT/firewall)
      hangup();
    } else if (st === "closed") {
      hangup();
    }
    // "disconnected" is transient — ICE may recover to "connected", or fall
    // through to "failed". Tearing down here ended calls on a brief blip
    // (the ring vanished and you had to redial).
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
  stopRing();
  pendingIce = [];
  remoteReady = false;
  if (timer) clearInterval(timer);
  timer = null;
  localStream?.getTracks().forEach((tr) => tr.stop());
  localStream = null;
  micSource?.disconnect();
  micGainNode?.disconnect();
  micSource = null;
  micGainNode = null;
  for (const el of remoteAudios.values()) el.srcObject = null;
  remoteAudios.clear();
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
        startRing(); // audible ringtone, not just the toast
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
