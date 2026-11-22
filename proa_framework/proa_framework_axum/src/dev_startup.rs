// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    env, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket},
};

/// Report a custom server's browser URL to the `proa dev` process that launched it.
/// Call `from_env` before binding, then `ready` once the routers and listeners
/// are ready. Outside the dev supervisor both operations are inert.
pub struct DevServerStartup(Option<(UdpSocket, String)>);

impl DevServerStartup {
    /// Tell the supervisor to wait for this process, even if a port already accepts TCP.
    pub fn from_env() -> io::Result<Self> {
        let Ok(endpoint) = env::var("PROA_DEV_READY") else {
            return Ok(Self(None));
        };
        let (port, token) = endpoint
            .split_once(':')
            .and_then(|(port, token)| Some((port.parse::<u16>().ok()?, token)))
            .filter(|(port, token)| {
                *port != 0 && token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid PROA_DEV_READY"))?;
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        socket.connect((Ipv4Addr::LOCALHOST, port))?;
        socket.send(token.as_bytes())?;
        Ok(Self(Some((socket, token.to_owned()))))
    }

    /// Publish the HTTP(S) URL a browser should open, including its mount path if needed.
    pub fn ready(self, browser_url: &str) -> io::Result<()> {
        if let Some((socket, token)) = self.0 {
            socket.send(format!("{token}\n{browser_url}").as_bytes())?;
        }
        Ok(())
    }

    pub(crate) fn http_listener_ready(listener: &tokio::net::TcpListener) -> io::Result<()> {
        let startup = Self::from_env()?;
        if startup.0.is_some() {
            let url = http_browser_url(
                listener.local_addr()?,
                env::var("PROA_HOST").ok().as_deref(),
            );
            startup.ready(&url)?;
        }
        Ok(())
    }
}

fn http_browser_url(bound: SocketAddr, configured: Option<&str>) -> String {
    // Preserve a requested DNS name when it reaches the actual listener. A
    // custom main can bind a different address or ask the OS for a free port.
    if let Some(host) =
        configured.filter(|name| name.trim_matches(['[', ']']).parse::<IpAddr>().is_err())
    {
        if (host, bound.port())
            .to_socket_addrs()
            .is_ok_and(|mut addresses| {
                addresses.any(|addr| {
                    addr.ip() == bound.ip()
                        || (bound.ip().is_unspecified() && addr.is_ipv4() == bound.is_ipv4())
                })
            })
        {
            return format!("http://{host}:{}", bound.port());
        }
    }
    let ip = match bound.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    let ip = configured
        .and_then(|name| name.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        .filter(|ip| {
            !ip.is_unspecified() && bound.ip().is_unspecified() && ip.is_ipv4() == bound.is_ipv4()
        })
        .unwrap_or(ip);
    format!("http://{}", SocketAddr::new(ip, bound.port()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_url_uses_the_bound_address_and_port() {
        for (bound, host, expected) in [
            ("127.0.0.1:8123", "localhost", "http://localhost:8123"),
            ("[::1]:8123", "127.0.0.1", "http://[::1]:8123"),
            ("[::]:8123", "::", "http://[::1]:8123"),
            ("0.0.0.0:8123", "127.0.0.25", "http://127.0.0.25:8123"),
        ] {
            assert_eq!(
                http_browser_url(bound.parse().unwrap(), Some(host)),
                expected
            );
        }
    }
}
