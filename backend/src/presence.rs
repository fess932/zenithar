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
    tx: broadcast::Sender<Outbound>,
}

impl PresenceRegistry {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            online: Mutex::new(HashMap::new()),
            tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Outbound> {
        self.tx.subscribe()
    }

    /// Register a connection; broadcasts `presence: online` on the 0→1 edge.
    pub fn join(&self, id: &str, kind: &str) {
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
