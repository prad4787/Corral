//! Port forwarder started by launchd with socket activation.
//!
//! launchd (root) binds 127.0.0.1:80 and :443 and passes the listening
//! sockets to this process, which runs as the logged-in user (the plist's
//! `UserName`). It forwards raw TCP to Caddy's internal ports, so TLS is still
//! terminated by Caddy and no Corral code ever runs as root.

use std::os::fd::FromRawFd;
use std::os::raw::{c_char, c_int};

unsafe extern "C" {
    /// `<launch.h>`: returns 0 and a malloc'd fd array for the named socket.
    fn launch_activate_socket(
        name: *const c_char,
        fds: *mut *mut c_int,
        cnt: *mut libc::size_t,
    ) -> c_int;
}

/// Listening fds launchd created for socket `name` (from the plist).
pub fn activated_sockets(name: &str) -> std::io::Result<Vec<std::net::TcpListener>> {
    let cname = std::ffi::CString::new(name).expect("no NUL");
    let mut fds: *mut c_int = std::ptr::null_mut();
    let mut cnt: libc::size_t = 0;
    // SAFETY: valid pointers; launchd allocates `fds` which we free below.
    let rc = unsafe { launch_activate_socket(cname.as_ptr(), &mut fds, &mut cnt) };
    if rc != 0 {
        return Err(std::io::Error::from_raw_os_error(rc));
    }
    let mut out = Vec::with_capacity(cnt);
    for i in 0..cnt {
        // SAFETY: launchd handed us ownership of `cnt` listening sockets.
        let fd = unsafe { *fds.add(i) };
        let l = unsafe { std::net::TcpListener::from_raw_fd(fd) };
        l.set_nonblocking(true)?;
        out.push(l);
    }
    // SAFETY: the array was allocated by launchd with malloc.
    unsafe { libc::free(fds.cast()) };
    Ok(out)
}

async fn pump(listener: tokio::net::TcpListener, target: u16) -> std::io::Result<()> {
    loop {
        let (mut inbound, _) = listener.accept().await?;
        tokio::spawn(async move {
            match tokio::net::TcpStream::connect(("127.0.0.1", target)).await {
                Ok(mut outbound) => {
                    let _ = inbound.set_nodelay(true);
                    let _ = outbound.set_nodelay(true);
                    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                }
                Err(e) => tracing::debug!(target, error = %e, "Corral is not running"),
            }
        });
    }
}

/// Forward `(listener, target port)` pairs forever.
pub async fn serve(pairs: Vec<(std::net::TcpListener, u16)>) -> std::io::Result<()> {
    let mut tasks = Vec::new();
    for (l, target) in pairs {
        tasks.push(tokio::spawn(pump(
            tokio::net::TcpListener::from_std(l)?,
            target,
        )));
    }
    for t in tasks {
        t.await.map_err(std::io::Error::other)??;
    }
    Ok(())
}

/// Entry point for `--port-forward <http_target> <https_target>`, as run by launchd.
pub fn main_from_launchd(http_target: u16, https_target: u16) -> anyhow::Result<()> {
    let mut pairs = Vec::new();
    for (name, target) in [("http", http_target), ("https", https_target)] {
        for l in activated_sockets(name)? {
            pairs.push((l, target));
        }
    }
    anyhow::ensure!(!pairs.is_empty(), "launchd passed no sockets");
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(serve(pairs))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn forwards_bytes_both_ways() {
        // Upstream echo server standing in for Caddy.
        let up = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let up_port = up.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = up.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut b = [0u8; 64];
                    let n = s.read(&mut b).await.unwrap();
                    s.write_all(&[b"echo:".as_slice(), &b[..n]].concat())
                        .await
                        .unwrap();
                });
            }
        });
        let front = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        front.set_nonblocking(true).unwrap();
        let front_port = front.local_addr().unwrap().port();
        tokio::spawn(serve(vec![(front, up_port)]));
        let mut c = tokio::net::TcpStream::connect(("127.0.0.1", front_port))
            .await
            .unwrap();
        c.write_all(b"hello").await.unwrap();
        let mut b = [0u8; 64];
        let n = c.read(&mut b).await.unwrap();
        assert_eq!(&b[..n], b"echo:hello");
    }

    #[test]
    fn activation_fails_outside_launchd() {
        assert!(activated_sockets("http").is_err());
    }
}
