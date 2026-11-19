//! The in-process connection between a frontend and its engine thread.
//!
//! Unix uses a socket pair. Windows has no socket pair, so it uses a loopback
//! TCP connection with the same nonblocking reads and writes.
use std::io;

#[cfg(unix)]
pub(crate) type Socket = std::os::unix::net::UnixStream;
#[cfg(windows)]
pub(crate) type Socket = std::net::TcpStream;
#[cfg(unix)]
pub(crate) type AsyncSocket = tokio::net::UnixStream;
#[cfg(windows)]
pub(crate) type AsyncSocket = tokio::net::TcpStream;

/// Two connected ends: the frontend's and the engine's.
#[cfg(unix)]
pub(crate) fn pair() -> io::Result<(Socket, Socket)> {
    Socket::pair()
}

/// Two connected ends: the frontend's and the engine's.
#[cfg(windows)]
pub(crate) fn pair() -> io::Result<(Socket, Socket)> {
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let client = TcpStream::connect(listener.local_addr()?)?;
    let ours = client.local_addr()?;
    // Another local process could connect first; accept only our own end.
    // The listener closes when this returns, so the port is open for
    // milliseconds.
    loop {
        let (server, peer) = listener.accept()?;
        if peer == ours {
            client.set_nodelay(true)?;
            server.set_nodelay(true)?;
            return Ok((client, server));
        }
    }
}

/// A nonblocking connection that can wait until it is readable or writable.
pub(crate) trait Ready {
    fn nonblocking(&self) -> io::Result<()>;
    /// Waits up to `millis` for the connection to become readable (or
    /// writable). Returns early on readiness, and also on timeout.
    fn wait(&self, write: bool, millis: i32) -> io::Result<()>;
}

impl Ready for Socket {
    fn nonblocking(&self) -> io::Result<()> {
        self.set_nonblocking(true)
    }
    #[cfg(unix)]
    fn wait(&self, write: bool, millis: i32) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        let mut fd = libc::pollfd {
            fd: self.as_raw_fd(),
            events: if write { libc::POLLOUT } else { libc::POLLIN },
            revents: 0,
        };
        // SAFETY: one live owned socket descriptor.
        if unsafe { libc::poll(&mut fd, 1, millis) } < 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    fn wait(&self, write: bool, millis: i32) -> io::Result<()> {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{
            WSAPoll, POLLRDNORM, POLLWRNORM, SOCKET, WSAPOLLFD,
        };
        let mut fd = WSAPOLLFD {
            fd: self.as_raw_socket() as SOCKET,
            events: if write { POLLWRNORM } else { POLLRDNORM },
            revents: 0,
        };
        // SAFETY: one live owned socket.
        if unsafe { WSAPoll(&mut fd, 1, millis) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn pair_carries_bytes_both_ways_and_wakes_waiting_readers() {
        let (mut a, mut b) = pair().unwrap();
        a.write_all(b"ping").unwrap();
        let mut got = [0; 4];
        b.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"ping");
        b.write_all(b"pong").unwrap();
        a.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"pong");

        b.nonblocking().unwrap();
        let mut byte = [0; 1];
        assert_eq!(
            b.read(&mut byte).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        a.write_all(b"x").unwrap();
        b.wait(false, 2000).unwrap();
        assert_eq!(b.read(&mut byte).unwrap(), 1);
        b.wait(true, 2000).unwrap();
    }
}
