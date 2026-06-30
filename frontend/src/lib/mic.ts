// Microphone input selection. Both calls and voice notes go through
// `audioConstraints()`, so picking a device here applies everywhere. The choice
// persists in localStorage; "" means the system default.
import { writable, get } from "svelte/store";

const KEY = "zenithar.mic";

function read(): string {
  try {
    return localStorage.getItem(KEY) ?? "";
  } catch {
    return "";
  }
}

export const selectedMic = writable<string>(read());
selectedMic.subscribe((v) => {
  try {
    if (v) localStorage.setItem(KEY, v);
    else localStorage.removeItem(KEY);
  } catch {
    /* private mode — keep it in memory only */
  }
});

const BASE: MediaTrackConstraints = {
  echoCancellation: true,
  noiseSuppression: true,
  autoGainControl: true,
};

/// getUserMedia audio constraints, honoring the chosen input device.
export function audioConstraints(): MediaTrackConstraints {
  const id = get(selectedMic);
  return id ? { ...BASE, deviceId: { exact: id } } : { ...BASE };
}

/// Open the mic stream with the chosen device, FALLING BACK to the default if
/// that device is gone (e.g. a remembered headset got unplugged) — otherwise an
/// `exact` deviceId throws and calls/voice notes would just fail.
export async function getMicStream(): Promise<MediaStream> {
  try {
    return await navigator.mediaDevices.getUserMedia({ audio: audioConstraints() });
  } catch (e) {
    const name = e instanceof Error ? e.name : "";
    const deviceGone = ["OverconstrainedError", "NotFoundError", "NotReadableError"].includes(name);
    if (get(selectedMic) && deviceGone) {
      return navigator.mediaDevices.getUserMedia({ audio: { ...BASE } });
    }
    throw e;
  }
}

export interface MicDevice {
  deviceId: string;
  label: string;
}

/// List audio-input devices. Labels are only populated after a permission grant
/// (any prior call/voice note), so we fall back to a numbered name.
export async function listMics(): Promise<MicDevice[]> {
  try {
    const devs = await navigator.mediaDevices.enumerateDevices();
    return devs
      .filter((d) => d.kind === "audioinput")
      .map((d, i) => ({ deviceId: d.deviceId, label: d.label || `Микрофон ${i + 1}` }));
  } catch {
    return [];
  }
}
