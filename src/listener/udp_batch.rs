use std::io;
use std::net::SocketAddr;
use tokio::net::UdpSocket;

/// Upper bound on the per-syscall batch size. Stack-allocated recvmmsg scratch
/// arrays are sized to this constant; actual batch usage comes from `BatchBuf`.
const MAX_BATCH_SIZE: usize = 32;

/// Datagram slot size that accommodates any legal UDP payload (RFC 768).
pub const MAX_DATAGRAM: usize = 65_535;

/// Batch / slot sizing for the shared ingress listener. 32 × 65 KiB = 2 MiB
/// per port — one-time allocation.
pub const LISTENER_BATCH_SIZE: usize = 32;
pub const LISTENER_SLOT_SIZE: usize = MAX_DATAGRAM;

/// Batch / slot sizing for per-session reply buffers. Smaller batch keeps the
/// per-session footprint bounded (8 × 65 KiB = 512 KiB) while still amortising
/// syscall overhead 8×. Slot size stays at MAX_DATAGRAM so we never truncate
/// legitimate UDP datagrams of any size.
pub const SESSION_BATCH_SIZE: usize = 8;
pub const SESSION_SLOT_SIZE: usize = MAX_DATAGRAM;

/// Reusable buffer for batched UDP receive. Holds a flat allocation sliced
/// into `batch_size` slots of `slot_size` bytes each.
pub struct BatchBuf {
    data: Box<[u8]>,
    lens: Box<[usize]>,
    addrs: Box<[Option<SocketAddr>]>,
    // Non-Linux platforms fall back to single recv_from and ignore batch_size.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    batch_size: usize,
    slot_size: usize,
}

impl BatchBuf {
    pub fn new(batch_size: usize, slot_size: usize) -> Self {
        assert!(
            batch_size > 0 && batch_size <= MAX_BATCH_SIZE,
            "batch_size must be 1..={MAX_BATCH_SIZE}",
        );
        Self {
            data: vec![0u8; batch_size * slot_size].into_boxed_slice(),
            lens: vec![0; batch_size].into_boxed_slice(),
            addrs: vec![None; batch_size].into_boxed_slice(),
            batch_size,
            slot_size,
        }
    }

    pub fn slot(&self, index: usize) -> &[u8] {
        let offset = index * self.slot_size;
        &self.data[offset..offset + self.lens[index]]
    }

    pub fn addr(&self, index: usize) -> Option<SocketAddr> {
        self.addrs[index]
    }

    fn slot_mut_full(&mut self, index: usize) -> &mut [u8] {
        let offset = index * self.slot_size;
        &mut self.data[offset..offset + self.slot_size]
    }
}

/// Receive up to `buf.batch_size` datagrams from an unconnected socket,
/// capturing each sender's address. Returns the count of filled slots.
#[cfg(target_os = "linux")]
#[allow(clippy::needless_range_loop)] // three parallel mutable arrays — index-based is clearer
pub async fn recv_batch(socket: &UdpSocket, buf: &mut BatchBuf) -> io::Result<usize> {
    use std::mem;
    use std::os::fd::AsRawFd;
    use tokio::io::Interest;

    let batch = buf.batch_size;

    loop {
        socket.readable().await?;

        let result: io::Result<usize> = socket.try_io(Interest::READABLE, || {
            let fd = socket.as_raw_fd();

            let mut iovs: [libc::iovec; MAX_BATCH_SIZE] = unsafe { mem::zeroed() };
            let mut msgs: [libc::mmsghdr; MAX_BATCH_SIZE] = unsafe { mem::zeroed() };
            let mut sockaddrs: [libc::sockaddr_storage; MAX_BATCH_SIZE] =
                unsafe { mem::zeroed() };

            for i in 0..batch {
                let slot = buf.slot_mut_full(i);
                iovs[i].iov_base = slot.as_mut_ptr() as *mut libc::c_void;
                iovs[i].iov_len = slot.len();
                msgs[i].msg_hdr.msg_iov = &mut iovs[i] as *mut libc::iovec;
                msgs[i].msg_hdr.msg_iovlen = 1;
                msgs[i].msg_hdr.msg_name =
                    &mut sockaddrs[i] as *mut libc::sockaddr_storage as *mut libc::c_void;
                msgs[i].msg_hdr.msg_namelen =
                    mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            }

            let received = unsafe {
                libc::recvmmsg(
                    fd,
                    msgs.as_mut_ptr(),
                    batch as libc::c_uint,
                    libc::MSG_DONTWAIT as _,
                    std::ptr::null_mut(),
                )
            };

            if received < 0 {
                return Err(io::Error::last_os_error());
            }

            let received = received as usize;
            for i in 0..received {
                buf.lens[i] = msgs[i].msg_len as usize;
                buf.addrs[i] = unsafe { sockaddr_storage_to_socket_addr(&sockaddrs[i]) };
            }
            Ok(received)
        });

        match result {
            Ok(n) => return Ok(n),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Receive up to `buf.batch_size` datagrams from a connected socket — the
/// source address is known (the connected peer), so we skip address extraction
/// (msg_name = NULL). Intended for per-session upstream reply batching.
#[cfg(target_os = "linux")]
#[allow(clippy::needless_range_loop)] // two parallel mutable arrays — index-based is clearer
pub async fn recv_batch_connected(
    socket: &UdpSocket,
    buf: &mut BatchBuf,
) -> io::Result<usize> {
    use std::mem;
    use std::os::fd::AsRawFd;
    use tokio::io::Interest;

    let batch = buf.batch_size;

    loop {
        socket.readable().await?;

        let result: io::Result<usize> = socket.try_io(Interest::READABLE, || {
            let fd = socket.as_raw_fd();

            let mut iovs: [libc::iovec; MAX_BATCH_SIZE] = unsafe { mem::zeroed() };
            let mut msgs: [libc::mmsghdr; MAX_BATCH_SIZE] = unsafe { mem::zeroed() };

            for i in 0..batch {
                let slot = buf.slot_mut_full(i);
                iovs[i].iov_base = slot.as_mut_ptr() as *mut libc::c_void;
                iovs[i].iov_len = slot.len();
                msgs[i].msg_hdr.msg_iov = &mut iovs[i] as *mut libc::iovec;
                msgs[i].msg_hdr.msg_iovlen = 1;
                // msg_name left NULL — connected socket has a fixed peer.
            }

            let received = unsafe {
                libc::recvmmsg(
                    fd,
                    msgs.as_mut_ptr(),
                    batch as libc::c_uint,
                    libc::MSG_DONTWAIT as _,
                    std::ptr::null_mut(),
                )
            };

            if received < 0 {
                return Err(io::Error::last_os_error());
            }

            let received = received as usize;
            for i in 0..received {
                buf.lens[i] = msgs[i].msg_len as usize;
            }
            Ok(received)
        });

        match result {
            Ok(n) => return Ok(n),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e),
        }
    }
}

#[cfg(target_os = "linux")]
unsafe fn sockaddr_storage_to_socket_addr(
    storage: &libc::sockaddr_storage,
) -> Option<SocketAddr> {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};

    match storage.ss_family as libc::c_int {
        libc::AF_INET => {
            let sa = unsafe { &*(storage as *const _ as *const libc::sockaddr_in) };
            let ip = Ipv4Addr::from(u32::from_be(sa.sin_addr.s_addr));
            Some(SocketAddr::V4(SocketAddrV4::new(
                ip,
                u16::from_be(sa.sin_port),
            )))
        }
        libc::AF_INET6 => {
            let sa = unsafe { &*(storage as *const _ as *const libc::sockaddr_in6) };
            let ip = Ipv6Addr::from(sa.sin6_addr.s6_addr);
            Some(SocketAddr::V6(SocketAddrV6::new(
                ip,
                u16::from_be(sa.sin6_port),
                sa.sin6_flowinfo,
                sa.sin6_scope_id,
            )))
        }
        _ => None,
    }
}

#[cfg(not(target_os = "linux"))]
pub async fn recv_batch(socket: &UdpSocket, buf: &mut BatchBuf) -> io::Result<usize> {
    let slot = buf.slot_mut_full(0);
    let (bytes_read, source) = socket.recv_from(slot).await?;
    buf.lens[0] = bytes_read;
    buf.addrs[0] = Some(source);
    Ok(1)
}

#[cfg(not(target_os = "linux"))]
pub async fn recv_batch_connected(
    socket: &UdpSocket,
    buf: &mut BatchBuf,
) -> io::Result<usize> {
    let slot = buf.slot_mut_full(0);
    let bytes_read = socket.recv(slot).await?;
    buf.lens[0] = bytes_read;
    Ok(1)
}
