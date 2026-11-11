//! The datagram port (APP-159, plan D5): one transfer frame per datagram, never reordered,
//! merged, or split.

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

/// The largest UDP payload; a receive buffer of this size never truncates a datagram.
const MAX_DATAGRAM: usize = 65_536;

/// Where a datagram goes and comes from.
pub trait Port {
    /// Sends `octets` as one datagram.
    ///
    /// # Errors
    /// If the transport fails.
    fn send(&mut self, octets: &[u8]) -> io::Result<()>;

    /// The next datagram, waiting at most `wait`; `None` when none arrived in that time. A zero
    /// `wait` only looks.
    ///
    /// # Errors
    /// If the transport fails.
    fn recv(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>>;
}

/// A UDP socket with the address of the peer it sends to.
#[derive(Debug)]
pub struct UdpPort {
    socket: UdpSocket,
    peer: SocketAddr,
}

impl UdpPort {
    /// A port that receives on `socket` and sends to `peer`.
    #[must_use]
    pub const fn new(socket: UdpSocket, peer: SocketAddr) -> Self {
        UdpPort { socket, peer }
    }
}

impl Port for UdpPort {
    fn send(&mut self, octets: &[u8]) -> io::Result<()> {
        let sent = self.socket.send_to(octets, self.peer)?;
        if sent == octets.len() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "datagram sent in part",
            ))
        }
    }

    fn recv(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        let mut buffer = vec![0_u8; MAX_DATAGRAM];
        let result = if wait.is_zero() {
            self.socket.set_nonblocking(true)?;
            let r = self.socket.recv(&mut buffer);
            self.socket.set_nonblocking(false)?;
            r
        } else {
            self.socket.set_nonblocking(false)?;
            self.socket.set_read_timeout(Some(wait))?;
            self.socket.recv(&mut buffer)
        };
        match result {
            Ok(n) => {
                buffer.truncate(n);
                Ok(Some(buffer))
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::UdpSocket;
    use std::time::Duration;

    use super::{Port, UdpPort};

    #[test]
    fn one_frame_per_datagram_in_order() {
        // req: APP-159
        let a = UdpSocket::bind("127.0.0.1:0").unwrap();
        let b = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (aa, ba) = (a.local_addr().unwrap(), b.local_addr().unwrap());
        let mut pa = UdpPort::new(a, ba);
        let mut pb = UdpPort::new(b, aa);
        assert_eq!(pb.recv(Duration::ZERO).unwrap(), None);
        let frames: [&[u8]; 3] = [&[1, 2, 3], &[4], &[5, 6]];
        for f in frames {
            pa.send(f).unwrap();
        }
        for f in frames {
            let got = pb.recv(Duration::from_secs(2)).unwrap().unwrap();
            assert_eq!(got, f, "no merge, no split, no reorder");
        }
        assert_eq!(pb.recv(Duration::from_millis(5)).unwrap(), None);
    }
}
