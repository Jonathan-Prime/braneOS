// ============================================================
// Brane OS Kernel — Socket API
// ============================================================
//
// Provides kernel-level socket abstraction for TCP/UDP.
// Manages a global socket table with create/bind/connect/send/recv.
//
// Reference: ARCHITECTURE.md §5.4 (planned)
// ============================================================

#![allow(dead_code)]

use spin::Mutex;

// -----------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------

/// Socket protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

/// Socket state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketState {
    Closed,
    Created,
    Bound,
    Listening,
    Connected,
    Established,
}

/// IPv4 endpoint (address + port).
#[derive(Debug, Clone, Copy)]
pub struct Endpoint {
    pub addr: [u8; 4],
    pub port: u16,
}

impl Endpoint {
    pub const fn zero() -> Self {
        Self {
            addr: [0; 4],
            port: 0,
        }
    }
}

/// A kernel socket.
#[derive(Debug, Clone, Copy)]
pub struct Socket {
    pub id: u32,
    pub protocol: Protocol,
    pub state: SocketState,
    pub local: Endpoint,
    pub remote: Endpoint,
    pub owner_pid: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

impl Socket {
    const fn empty() -> Self {
        Self {
            id: 0,
            protocol: Protocol::Tcp,
            state: SocketState::Closed,
            local: Endpoint::zero(),
            remote: Endpoint::zero(),
            owner_pid: 0,
            rx_bytes: 0,
            tx_bytes: 0,
        }
    }

    fn is_active(&self) -> bool {
        self.state != SocketState::Closed
    }
}

// -----------------------------------------------------------------------
// Socket Table
// -----------------------------------------------------------------------

const MAX_SOCKETS: usize = 32;

pub struct SocketTable {
    sockets: [Socket; MAX_SOCKETS],
    next_id: u32,
}

impl SocketTable {
    const fn new() -> Self {
        Self {
            sockets: [Socket::empty(); MAX_SOCKETS],
            next_id: 1,
        }
    }

    /// Create a new socket, returns socket ID.
    pub fn create(&mut self, protocol: Protocol, owner_pid: u64) -> Result<u32, SocketError> {
        for slot in self.sockets.iter_mut() {
            if !slot.is_active() {
                let id = self.next_id;
                self.next_id += 1;
                *slot = Socket {
                    id,
                    protocol,
                    state: SocketState::Created,
                    local: Endpoint::zero(),
                    remote: Endpoint::zero(),
                    owner_pid,
                    rx_bytes: 0,
                    tx_bytes: 0,
                };
                return Ok(id);
            }
        }
        Err(SocketError::TableFull)
    }

    /// Bind a socket to a local endpoint.
    pub fn bind(&mut self, id: u32, addr: [u8; 4], port: u16) -> Result<(), SocketError> {
        let sock = self.get_mut(id)?;
        if sock.state != SocketState::Created {
            return Err(SocketError::InvalidState);
        }
        sock.local = Endpoint { addr, port };
        sock.state = SocketState::Bound;
        Ok(())
    }

    /// Set a socket to listening state (TCP only).
    pub fn listen(&mut self, id: u32) -> Result<(), SocketError> {
        let sock = self.get_mut(id)?;
        if sock.protocol != Protocol::Tcp {
            return Err(SocketError::InvalidProtocol);
        }
        if sock.state != SocketState::Bound {
            return Err(SocketError::InvalidState);
        }
        sock.state = SocketState::Listening;
        Ok(())
    }

    /// Connect a socket to a remote endpoint.
    pub fn connect(&mut self, id: u32, addr: [u8; 4], port: u16) -> Result<(), SocketError> {
        let sock = self.get_mut(id)?;
        sock.remote = Endpoint { addr, port };
        sock.state = SocketState::Connected;
        Ok(())
    }

    /// Close a socket.
    pub fn close(&mut self, id: u32) -> Result<(), SocketError> {
        let sock = self.get_mut(id)?;
        sock.state = SocketState::Closed;
        Ok(())
    }

    /// Send a datagram (stub).
    pub fn send_to(
        &mut self,
        id: u32,
        buf: &[u8],
        _addr: [u8; 4],
        _port: u16,
    ) -> Result<(), SocketError> {
        let sock = self.get_mut(id)?;
        if sock.state != SocketState::Bound && sock.state != SocketState::Connected {
            return Err(SocketError::InvalidState);
        }
        sock.tx_bytes += buf.len() as u64;
        Ok(())
    }

    /// Receive a datagram (stub).
    pub fn recv_from(
        &mut self,
        id: u32,
        _buf: &mut [u8],
    ) -> Result<(usize, Endpoint), SocketError> {
        let sock = self.get_mut(id)?;
        if sock.state != SocketState::Bound && sock.state != SocketState::Connected {
            return Err(SocketError::InvalidState);
        }
        // Stub: always return Timeout (no data)
        Err(SocketError::Timeout)
    }

    /// Get socket info by ID.
    pub fn get(&self, id: u32) -> Option<&Socket> {
        self.sockets.iter().find(|s| s.id == id && s.is_active())
    }

    /// Get mutable socket by ID.
    fn get_mut(&mut self, id: u32) -> Result<&mut Socket, SocketError> {
        self.sockets
            .iter_mut()
            .find(|s| s.id == id && s.is_active())
            .ok_or(SocketError::NotFound)
    }

    /// Number of active sockets.
    pub fn active_count(&self) -> usize {
        self.sockets.iter().filter(|s| s.is_active()).count()
    }

    /// List all active sockets.
    pub fn active_sockets(&self) -> impl Iterator<Item = &Socket> {
        self.sockets.iter().filter(|s| s.is_active())
    }

    /// Maximum socket capacity.
    pub fn capacity(&self) -> usize {
        MAX_SOCKETS
    }
}

/// Socket error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketError {
    NotFound,
    TableFull,
    InvalidState,
    InvalidProtocol,
    ConnectionRefused,
    Timeout,
    IoError,
}

/// Global socket table.
pub static SOCKET_TABLE: Mutex<SocketTable> = Mutex::new(SocketTable::new());
