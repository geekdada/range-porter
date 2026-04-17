use crate::forward::udp::{forward_to_client, forward_to_target};
use crate::socket::new_connected_udp_socket;
use crate::stats::port::PortStats;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use std::cmp;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;
use tracing::warn;

#[derive(Debug)]
pub struct UdpSession {
    id: u64,
    source: SocketAddr,
    upstream: Arc<UdpSocket>,
    last_active_epoch: AtomicU64,
    closed: AtomicBool,
    shutdown: CancellationToken,
}

#[derive(Debug)]
pub struct UdpSessionTable {
    target: SocketAddr,
    idle_timeout: Duration,
    listener_socket: Arc<UdpSocket>,
    stats: Arc<PortStats>,
    shutdown: CancellationToken,
    next_session_id: AtomicU64,
    sessions: DashMap<SocketAddr, Arc<UdpSession>>,
}

impl UdpSession {
    fn new(id: u64, source: SocketAddr, upstream: Arc<UdpSocket>) -> Self {
        Self {
            id,
            source,
            upstream,
            last_active_epoch: AtomicU64::new(unix_timestamp_now()),
            closed: AtomicBool::new(false),
            shutdown: CancellationToken::new(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn source(&self) -> SocketAddr {
        self.source
    }

    pub fn upstream(&self) -> &UdpSocket {
        self.upstream.as_ref()
    }

    pub fn touch(&self) {
        self.last_active_epoch
            .store(unix_timestamp_now(), Ordering::Relaxed);
    }

    pub fn last_active_epoch(&self) -> u64 {
        self.last_active_epoch.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.shutdown.cancel();
    }

    pub fn shutdown_token(&self) -> &CancellationToken {
        &self.shutdown
    }

    pub fn mark_closed(&self) -> bool {
        !self.closed.swap(true, Ordering::Relaxed)
    }
}

impl UdpSessionTable {
    pub fn new(
        target: SocketAddr,
        idle_timeout: Duration,
        listener_socket: Arc<UdpSocket>,
        stats: Arc<PortStats>,
        shutdown: CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            target,
            idle_timeout,
            listener_socket,
            stats,
            shutdown,
            next_session_id: AtomicU64::new(1),
            sessions: DashMap::new(),
        })
    }

    pub fn get_or_create(self: &Arc<Self>, source: SocketAddr) -> io::Result<Arc<UdpSession>> {
        if let Some(session) = self
            .sessions
            .get(&source)
            .map(|entry| entry.value().clone())
        {
            session.touch();
            return Ok(session);
        }

        let upstream = Arc::new(new_connected_udp_socket(self.target)?);
        let session = Arc::new(UdpSession::new(
            self.next_session_id.fetch_add(1, Ordering::Relaxed),
            source,
            upstream,
        ));

        match self.sessions.entry(source) {
            Entry::Occupied(entry) => {
                let existing = entry.get().clone();
                existing.touch();
                Ok(existing)
            }
            Entry::Vacant(entry) => {
                entry.insert(session.clone());
                self.stats.record_udp_flow_open();
                self.spawn_reply_task(session.clone());
                Ok(session)
            }
        }
    }

    pub fn remove_session(&self, source: SocketAddr, session_id: u64) {
        if let Some((_, session)) = self
            .sessions
            .remove_if(&source, |_, current| current.id() == session_id)
        {
            session.cancel();
            if session.mark_closed() {
                self.stats.record_udp_flow_close();
            }
        }
    }

    pub fn shutdown_all(&self) {
        let sessions: Vec<_> = self
            .sessions
            .iter()
            .map(|entry| (*entry.key(), entry.value().id()))
            .collect();

        for (source, session_id) in sessions {
            self.remove_session(source, session_id);
        }
    }

    pub async fn cleanup_loop(self: Arc<Self>) {
        let timeout_seconds = cmp::max(1, self.idle_timeout.as_secs());
        let interval_seconds = cmp::min(timeout_seconds, 5);
        let mut interval = tokio::time::interval(Duration::from_secs(interval_seconds));

        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => break,
                _ = interval.tick() => {
                    let now = unix_timestamp_now();
                    let stale_sessions: Vec<_> = self.sessions
                        .iter()
                        .filter_map(|entry| {
                            let session = entry.value();
                            let idle = now.saturating_sub(session.last_active_epoch());
                            (idle >= timeout_seconds).then_some((*entry.key(), session.id()))
                        })
                        .collect();

                    for (source, session_id) in stale_sessions {
                        self.remove_session(source, session_id);
                    }
                }
            }
        }
    }

    fn spawn_reply_task(self: &Arc<Self>, session: Arc<UdpSession>) {
        let table = Arc::clone(self);
        tokio::spawn(async move {
            table.reply_loop(session).await;
        });
    }

    async fn reply_loop(self: Arc<Self>, session: Arc<UdpSession>) {
        let mut buffer = vec![0_u8; 65_535];

        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => break,
                _ = session.shutdown_token().cancelled() => break,
                result = session.upstream().recv(&mut buffer) => {
                    match result {
                        Ok(bytes_read) => {
                            if bytes_read == 0 {
                                continue;
                            }

                            match forward_to_client(
                                &self.listener_socket,
                                session.source(),
                                &buffer[..bytes_read],
                            )
                            .await
                            {
                                Ok(_) => {
                                    session.touch();
                                    self.stats.add_udp_out(bytes_read as u64);
                                }
                                Err(error) => {
                                    warn!(source = %session.source(), ?error, "failed to forward UDP response to client");
                                    break;
                                }
                            }
                        }
                        Err(error) => {
                            warn!(source = %session.source(), ?error, "udp session receive loop ended with an error");
                            break;
                        }
                    }
                }
            }
        }

        self.remove_session(session.source(), session.id());
    }

    pub async fn forward_client_packet(
        &self,
        session: &UdpSession,
        payload: &[u8],
    ) -> io::Result<()> {
        match forward_to_target(session.upstream(), payload).await {
            Ok(_) => {
                session.touch();
                self.stats.add_udp_in(payload.len() as u64);
                Ok(())
            }
            Err(error) => {
                self.remove_session(session.source(), session.id());
                Err(error)
            }
        }
    }
}

fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock drifted before unix epoch")
        .as_secs()
}
