use std::io;
use std::net::SocketAddr;
use tokio::net::UdpSocket;

pub const BATCH_SIZE: usize = 32;
pub const MAX_DATAGRAM: usize = 65_535;

/// Reusable per-listener buffer for batched UDP ingress. Owns a single flat
/// allocation sliced into `BATCH_SIZE` slots to avoid per-iteration churn.
pub struct BatchBuf {
    data: Box<[u8]>,
    lens: Box<[usize]>,
    addrs: Box<[Option<SocketAddr>]>,
}

impl BatchBuf {
    pub fn new() -> Self {
        Self {
            data: vec![0u8; BATCH_SIZE * MAX_DATAGRAM].into_boxed_slice(),
            lens: vec![0; BATCH_SIZE].into_boxed_slice(),
            addrs: vec![None; BATCH_SIZE].into_boxed_slice(),
        }
    }

    pub fn slot(&self, index: usize) -> &[u8] {
        let offset = index * MAX_DATAGRAM;
        &self.data[offset..offset + self.lens[index]]
    }

    pub fn addr(&self, index: usize) -> Option<SocketAddr> {
        self.addrs[index]
    }

    fn slot_mut_full(&mut self, index: usize) -> &mut [u8] {
        let offset = index * MAX_DATAGRAM;
        &mut self.data[offset..offset + MAX_DATAGRAM]
    }
}

/// Receive up to `BATCH_SIZE` datagrams in a single wake-up. Returns the
/// count of filled slots; callers should iterate 0..n and use `addr` + `slot`.
#[cfg(target_os = "linux")]
pub async fn recv_batch(socket: &UdpSocket, buf: &mut BatchBuf) -> io::Result<usize> {
    use std::mem;
    use std::os::fd::AsRawFd;
    use tokio::io::Interest;

    loop {
        socket.readable().await?;

        let result: io::Result<usize> = socket.try_io(Interest::READABLE, || {
            let fd = socket.as_raw_fd();

            let mut iovs: [libc::iovec; BATCH_SIZE] = unsafe { mem::zeroed() };
            let mut msgs: [libc::mmsghdr; BATCH_SIZE] = unsafe { mem::zeroed() };
            let mut sockaddrs: [libc::sockaddr_storage; BATCH_SIZE] = unsafe { mem::zeroed() };

            for i in 0..BATCH_SIZE {
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
                    BATCH_SIZE as libc::c_uint,
                    libc::MSG_DONTWAIT,
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
