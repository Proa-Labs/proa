// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use std::{
    net::{Ipv4Addr, UdpSocket},
    process::Command,
};

/// Each launch gets a private reply address and token. A late reply from a
/// stopped child can never open a browser for its replacement.
pub(super) struct ReadyChannel {
    socket: UdpSocket,
    token: String,
    pub(super) reporting: bool,
    pub(super) port_was_busy: bool,
}

impl ReadyChannel {
    pub(super) fn bind(command: &mut Command, host: &str, port: u16) -> Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        socket.set_nonblocking(true)?;
        let token = super::reload::session_token()?;
        command.env(
            "PROA_DEV_READY",
            format!("{}:{token}", socket.local_addr()?.port()),
        );
        Ok(Self {
            socket,
            token,
            reporting: false,
            port_was_busy: super::reload::listener_responds(&super::reload::probe_addresses(
                host, port,
            )),
        })
    }

    pub(super) fn reported_url(&mut self) -> Option<String> {
        let mut bytes = [0u8; 8192];
        while let Ok((n, _)) = self.socket.recv_from(&mut bytes) {
            let Ok(message) = std::str::from_utf8(&bytes[..n]) else {
                continue;
            };
            let (token, url) = message.split_once('\n').unwrap_or((message, ""));
            if token != self.token {
                continue;
            }
            self.reporting = true;
            if !url.is_empty() && n < bytes.len() && valid_browser_url(url) {
                return Some(url.to_owned());
            }
        }
        None
    }
}

fn valid_browser_url(value: &str) -> bool {
    !value
        .bytes()
        .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        && reqwest::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "https" | "http")
                && url.has_host()
                && url.username().is_empty()
                && url.password().is_none()
        })
}
