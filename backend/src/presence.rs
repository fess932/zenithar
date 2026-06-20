//! Online presence (Phase 7). The server counts live WS connections per
//! principal — the first marks them online, the last offline. Changes fan out to
//! every socket so the UI can show who's connected right now. A principal may
//! have several connections (tabs / devices); only the 0↔1 transitions emit.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::broadcast;

use crate::models::{Outbound, PresenceEntry};

pub struct PresenceRegistry {
    // principal_id -> (kind, live connection count)
    online: Mutex<HashMap<String, (String, usize)>>,
    // principal_id -> unix millis of last activity (kept after they go offline,
    // so the connections list can show when they were last seen).
    last_seen: Mutex<HashMap<String, i64>>,
    // principal_id -> last measured WS ping round-trip (ms), while online.
    ping: Mutex<HashMap<String, i64>>,
    tx: broadcast::Sender<Outbound>,
}

impl PresenceRegistry {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            online: Mutex::new(HashMap::new()),
            last_seen: Mutex::new(HashMap::new()),
            ping: Mutex::new(HashMap::new()),
            tx,
        }
    }

    /// Record a measured WS ping/pong round-trip (ms) for a principal.
    pub fn set_ping(&self, id: &str, ms: i64) {
        self.ping.lock().unwrap().insert(id.to_string(), ms);
    }

    /// Snapshot of `principal_id -> last ping ms`.
    pub fn ping_map(&self) -> HashMap<String, i64> {
        self.ping.lock().unwrap().clone()
    }

    /// Record activity (a received frame) so we know when this principal was last
    /// seen — the last-packet time shown in the connections list.
    pub fn touch(&self, id: &str) {
        self.last_seen
            .lock()
            .unwrap()
            .insert(id.to_string(), crate::now_millis());
    }

    /// Snapshot of `principal_id -> last-seen millis`.
    pub fn last_seen_map(&self) -> HashMap<String, i64> {
        self.last_seen.lock().unwrap().clone()
    }

    /// Whether a principal has at least one live connection right now.
    pub fn is_online(&self, id: &str) -> bool {
        self.online.lock().unwrap().contains_key(id)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Outbound> {
        self.tx.subscribe()
    }

    /// Register a connection; broadcasts `presence: online` on the 0→1 edge.
    pub fn join(&self, id: &str, kind: &str) {
        self.touch(id);
        let became_online = {
            let mut map = self.online.lock().unwrap();
            let e = map.entry(id.to_string()).or_insert((kind.to_string(), 0));
            e.1 += 1;
            e.1 == 1
        };
        if became_online {
            let _ = self.tx.send(Outbound::Presence {
                id: id.to_string(),
                kind: kind.to_string(),
                online: true,
            });
        }
    }

    /// Drop a connection; broadcasts `presence: offline` on the 1→0 edge.
    pub fn leave(&self, id: &str) {
        let gone = {
            let mut map = self.online.lock().unwrap();
            match map.get_mut(id) {
                Some(e) => {
                    e.1 = e.1.saturating_sub(1);
                    if e.1 == 0 {
                        let kind = e.0.clone();
                        map.remove(id);
                        Some(kind)
                    } else {
                        None
                    }
                }
                None => None,
            }
        };
        if let Some(kind) = gone {
            self.ping.lock().unwrap().remove(id); // ping is only meaningful online
            let _ = self.tx.send(Outbound::Presence {
                id: id.to_string(),
                kind,
                online: false,
            });
        }
    }

    /// Everyone currently online (for a freshly connected socket).
    pub fn snapshot(&self) -> Vec<PresenceEntry> {
        self.online
            .lock()
            .unwrap()
            .iter()
            .map(|(id, (kind, _))| PresenceEntry {
                id: id.clone(),
                kind: kind.clone(),
            })
            .collect()
    }
}

impl Default for PresenceRegistry {
    fn default() -> Self {
        Self::new()
    }
}
