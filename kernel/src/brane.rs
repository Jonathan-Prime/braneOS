#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — Brane Protocol
// ============================================================
//
// The Brane Protocol enables secure communication between
// Brane OS instances and external devices (external branes).
//
// Types of branes:
//   - Companion: mobile devices (phones, tablets)
//   - Peer: other PCs or servers
//   - IoT: embedded devices, sensors, actuators
//
// Lifecycle:
//   1. Discovery: broadcast/listen for brane announcements
//   2. Pairing: mutual authentication + capability negotiation
//   3. Session: encrypted, capability-mediated communication
//   4. Disconnect: graceful teardown with audit logging
//
// All brane operations are mediated by the capability_broker
// and recorded by the audit_service.
//
// Spec reference: ARCHITECTURE.md §9 (Capa 5 — Brane Interface)
// ============================================================

use spin::Mutex;

use crate::audit;
use crate::sched::TaskId;
use crate::security::CapabilityId;

// -----------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------

pub type BraneId = u64;
pub type SessionId = u64;

/// Type of external brane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BraneType {
    /// Mobile device (phone, tablet).
    Companion,
    /// Another PC or server.
    Peer,
    /// Embedded / IoT device.
    IoT,
    /// Unknown device type.
    Unknown,
}

/// Transport layer used for brane communication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    TcpIp,
    Bluetooth,
    Ble,
    UsbDirect,
    Local, // loopback / same-machine
}

/// Authentication status of a brane session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    /// Not authenticated yet.
    Pending,
    /// Challenge sent, awaiting response.
    ChallengeSent,
    /// Fully authenticated (mutual auth complete).
    Authenticated,
    /// Authentication failed.
    Failed,
}

/// State of a brane connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Discovering,
    Pairing,
    Active,
    Suspended,
    Disconnecting,
    Disconnected,
}

// -----------------------------------------------------------------------
// Brane Info (discovered device)
// -----------------------------------------------------------------------

/// Information about a discovered external brane.
#[derive(Debug, Clone)]
pub struct BraneInfo {
    pub id: BraneId,
    pub brane_type: BraneType,
    pub name: [u8; 32],
    pub name_len: usize,
    pub transport: Transport,
    /// Capabilities this brane advertises.
    pub advertised_caps: u32,
    /// Signal strength / quality (0–100).
    pub signal_quality: u8,
}

impl BraneInfo {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("???")
    }
}

// -----------------------------------------------------------------------
// Brane Session
// -----------------------------------------------------------------------

/// An active session with an external brane.
#[derive(Debug, Clone)]
pub struct BraneSession {
    pub session_id: SessionId,
    pub remote_brane: BraneId,
    pub brane_type: BraneType,
    pub transport: Transport,
    pub auth_status: AuthStatus,
    pub state: SessionState,
    pub owner_task: TaskId,
    pub capability_granted: Option<CapabilityId>,
    /// Messages sent in this session.
    pub msgs_sent: u64,
    /// Messages received in this session.
    pub msgs_received: u64,
    /// Tick when session was established.
    pub established_at: u64,
}

// -----------------------------------------------------------------------
// Brane Message
// -----------------------------------------------------------------------

/// Maximum payload for brane messages.
pub const BRANE_MAX_PAYLOAD: usize = 2048;

/// Type of brane-level message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BraneMessageType {
    /// Discovery broadcast.
    Discover,
    /// Announcement in response to discovery.
    Announce,
    /// Pairing request.
    PairRequest,
    /// Pairing accepted.
    PairAccept,
    /// Pairing rejected.
    PairReject,
    /// Application-level data.
    Data,
    /// Telemetry / system status.
    Telemetry,
    /// Notification / alert.
    Notification,
    /// Command to execute on remote brane.
    Command,
    /// Response to a command.
    CommandResponse,
    /// Graceful disconnect.
    Disconnect,
}

/// A message in the brane protocol.
#[derive(Clone)]
pub struct BraneMessage {
    pub msg_type: BraneMessageType,
    pub source_brane: BraneId,
    pub dest_brane: BraneId,
    pub session_id: SessionId,
    pub payload_len: usize,
    pub payload: [u8; BRANE_MAX_PAYLOAD],
}

impl BraneMessage {
    pub fn new(
        msg_type: BraneMessageType,
        source: BraneId,
        dest: BraneId,
        session: SessionId,
        data: &[u8],
    ) -> Result<Self, BraneError> {
        if data.len() > BRANE_MAX_PAYLOAD {
            return Err(BraneError::PayloadTooLarge);
        }
        let mut payload = [0u8; BRANE_MAX_PAYLOAD];
        payload[..data.len()].copy_from_slice(data);
        Ok(Self {
            msg_type,
            source_brane: source,
            dest_brane: dest,
            session_id: session,
            payload_len: data.len(),
            payload,
        })
    }

    pub fn data(&self) -> &[u8] {
        &self.payload[..self.payload_len]
    }
}

// -----------------------------------------------------------------------
// Errors
// -----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BraneError {
    NotFound,
    AlreadyConnected,
    AuthenticationFailed,
    NotAuthenticated,
    SessionFull,
    PayloadTooLarge,
    NotConnected,
    PermissionDenied,
    TransportUnavailable,
    Timeout,
}

// -----------------------------------------------------------------------
// Brane Manager
// -----------------------------------------------------------------------

const MAX_DISCOVERED: usize = 16;
const MAX_SESSIONS: usize = 8;

/// The global Brane Manager handles discovery, pairing, and sessions.
pub struct BraneManager {
    /// Our local brane ID.
    local_id: BraneId,
    /// Discovered external branes.
    discovered: [Option<BraneInfo>; MAX_DISCOVERED],
    /// Active sessions.
    sessions: [Option<BraneSession>; MAX_SESSIONS],
    next_session_id: SessionId,
    /// Statistics.
    total_discovered: u64,
    total_connections: u64,
    total_disconnections: u64,
}

impl Default for BraneManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BraneManager {
    pub const fn new() -> Self {
        const NO_BRANE: Option<BraneInfo> = None;
        const NO_SESSION: Option<BraneSession> = None;
        Self {
            local_id: 0,
            discovered: [NO_BRANE; MAX_DISCOVERED],
            sessions: [NO_SESSION; MAX_SESSIONS],
            next_session_id: 1,
            total_discovered: 0,
            total_connections: 0,
            total_disconnections: 0,
        }
    }

    /// Set the local brane ID (called during init).
    pub fn set_local_id(&mut self, id: BraneId) {
        self.local_id = id;
    }

    /// Simulate discovering an external brane.
    ///
    /// In a real implementation, this would broadcast on the network
    /// and listen for responses.
    pub fn register_discovered(
        &mut self,
        name: &str,
        brane_type: BraneType,
        transport: Transport,
        caps: u32,
        signal: u8,
    ) -> Result<BraneId, BraneError> {
        for slot in self.discovered.iter_mut() {
            if slot.is_none() {
                self.total_discovered += 1;
                let id = self.total_discovered;

                let mut name_buf = [0u8; 32];
                let len = name.len().min(32);
                name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);

                *slot = Some(BraneInfo {
                    id,
                    brane_type,
                    name: name_buf,
                    name_len: len,
                    transport,
                    advertised_caps: caps,
                    signal_quality: signal,
                });

                crate::serial_println!(
                    "[brane] Discovered: '{}' ({:?}, {:?}, signal={}%)",
                    name,
                    brane_type,
                    transport,
                    signal
                );
                return Ok(id);
            }
        }
        Err(BraneError::SessionFull)
    }

    /// Initiate a connection to a discovered brane.
    pub fn connect(
        &mut self,
        brane_id: BraneId,
        owner_task: TaskId,
    ) -> Result<SessionId, BraneError> {
        // Find the discovered brane
        let brane_info = self
            .discovered
            .iter()
            .flatten()
            .find(|b| b.id == brane_id)
            .ok_or(BraneError::NotFound)?;

        // Check not already connected
        if self
            .sessions
            .iter()
            .flatten()
            .any(|s| s.remote_brane == brane_id && s.state == SessionState::Active)
        {
            return Err(BraneError::AlreadyConnected);
        }

        let brane_type = brane_info.brane_type;
        let transport = brane_info.transport;
        let brane_name = brane_info.name_str();

        // Find a free session slot
        for slot in self.sessions.iter_mut() {
            if slot.is_none() {
                let session_id = self.next_session_id;
                self.next_session_id += 1;

                let tick = crate::sched::SCHEDULER.lock().total_ticks();

                *slot = Some(BraneSession {
                    session_id,
                    remote_brane: brane_id,
                    brane_type,
                    transport,
                    auth_status: AuthStatus::Authenticated, // simplified for now
                    state: SessionState::Active,
                    owner_task,
                    capability_granted: None,
                    msgs_sent: 0,
                    msgs_received: 0,
                    established_at: tick,
                });

                self.total_connections += 1;

                // Audit the connection
                audit::log_brane_connect(owner_task, brane_id, audit::AuditResult::Success);

                crate::serial_println!(
                    "[brane] Connected to '{}' (session={}, {:?})",
                    brane_name,
                    session_id,
                    transport
                );
                return Ok(session_id);
            }
        }
        Err(BraneError::SessionFull)
    }

    /// Send a message over a brane session.
    pub fn send(&mut self, session_id: SessionId, msg: &BraneMessage) -> Result<(), BraneError> {
        for session in self.sessions.iter_mut().flatten() {
            if session.session_id == session_id {
                if session.state != SessionState::Active {
                    return Err(BraneError::NotConnected);
                }
                if session.auth_status != AuthStatus::Authenticated {
                    return Err(BraneError::NotAuthenticated);
                }

                session.msgs_sent += 1;
                crate::serial_println!(
                    "[brane] Sent {:?} to brane {} ({} bytes, session={})",
                    msg.msg_type,
                    session.remote_brane,
                    msg.payload_len,
                    session_id
                );
                return Ok(());
            }
        }
        Err(BraneError::NotFound)
    }

    /// Disconnect from a brane.
    pub fn disconnect(&mut self, session_id: SessionId) -> Result<(), BraneError> {
        for slot in self.sessions.iter_mut() {
            if let Some(session) = slot {
                if session.session_id == session_id {
                    let brane_id = session.remote_brane;
                    let task = session.owner_task;

                    crate::serial_println!(
                        "[brane] Disconnected from brane {} (session={}, sent={}, recv={})",
                        brane_id,
                        session_id,
                        session.msgs_sent,
                        session.msgs_received
                    );

                    audit::log_brane_connect(task, brane_id, audit::AuditResult::Success);
                    self.total_disconnections += 1;
                    *slot = None;
                    return Ok(());
                }
            }
        }
        Err(BraneError::NotFound)
    }

    /// List active sessions.
    pub fn active_sessions(&self) -> impl Iterator<Item = &BraneSession> {
        self.sessions
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|s| s.state == SessionState::Active)
    }

    /// Number of active sessions.
    pub fn active_session_count(&self) -> usize {
        self.sessions
            .iter()
            .flatten()
            .filter(|s| s.state == SessionState::Active)
            .count()
    }

    /// Number of discovered branes.
    pub fn discovered_count(&self) -> usize {
        self.discovered.iter().filter(|s| s.is_some()).count()
    }

    /// Statistics: (total_discovered, total_connections, total_disconnections).
    pub fn stats(&self) -> (u64, u64, u64) {
        (
            self.total_discovered,
            self.total_connections,
            self.total_disconnections,
        )
    }
}

/// Global brane manager.
pub static BRANE: Mutex<BraneManager> = Mutex::new(BraneManager::new());
