// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    time::{Duration, Instant},
};

use super::*;

/// Read from a live event stream until `frames` blank-line-terminated frames
/// have arrived, so assertions never race the writer.
fn read_frames(reader: &mut BufReader<TcpStream>, frames: usize) -> String {
    let mut collected = String::new();
    let mut seen = 0;
    while seen < frames {
        let mut line = String::new();
        assert!(
            reader.read_line(&mut line).expect("read event frame") > 0,
            "stream closed after {collected}"
        );
        if line == "\n" || line == "\r\n" {
            seen += 1;
        }
        collected.push_str(&line);
    }
    collected
}

/// Open an event stream and consume everything up to the first real event:
/// the HTTP headers, then the `retry` directive that tells `EventSource` how
/// fast to reconnect after a restart.
fn connect(server: &ReloadServer) -> BufReader<TcpStream> {
    let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, server.port())).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    write!(
        writer,
        "GET {DEV_EVENTS_PATH}?token={} HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n",
        server.token()
    )
    .expect("send request");
    let mut reader = BufReader::new(stream);

    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "headers ended");
        if line == "\r\n" {
            break;
        }
    }

    let retry = read_frames(&mut reader, 1);
    assert!(retry.contains("retry: 1000"), "{retry}");
    reader
}

fn wait_for_clients(server: &ReloadServer, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while server.client_count() != expected {
        assert!(
            Instant::now() < deadline,
            "expected {expected} live-reload clients, found {}",
            server.client_count()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn event_stream_greets_a_new_client_and_broadcasts_to_every_tab() {
    let server = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    let mut first = connect(&server);
    let mut second = connect(&server);

    // `hello` carries the generation a tab compares against on reconnect.
    let greeting = read_frames(&mut first, 1);
    assert!(greeting.contains("event: hello"), "{greeting}");
    assert!(greeting.contains("\"generation\":0"), "{greeting}");
    read_frames(&mut second, 1);

    wait_for_clients(&server, 2);
    server.broadcast(&ReloadEvent::Building);
    server.broadcast(&ReloadEvent::Reload);

    for stream in [&mut first, &mut second] {
        let frames = read_frames(stream, 2);
        assert!(frames.contains("event: building"), "{frames}");
        assert!(frames.contains("event: reload"), "{frames}");
        // Only an outcome advances the generation; `building` is progress.
        assert!(frames.contains("\"generation\":1"), "{frames}");
    }
}

#[test]
fn a_client_connecting_after_a_failed_build_is_told_about_it() {
    let server = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    server.broadcast(&ReloadEvent::Failed(
        "error[E0425]: cannot find value `x`\n  --> src/main.rs:3:5".to_string(),
    ));

    let mut stream = connect(&server);
    let frames = read_frames(&mut stream, 2);
    assert!(frames.contains("event: hello"), "{frames}");
    assert!(frames.contains("event: failed"), "{frames}");
    // The newline is escaped, so a multi-line compiler error stays on one
    // `data:` line and cannot forge a frame boundary.
    assert!(frames.contains("\\n  --> src/main.rs:3:5"), "{frames}");
    assert_eq!(frames.matches("data:").count(), 2, "{frames}");
}

#[test]
fn a_successful_build_clears_the_stored_failure() {
    let server = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    server.broadcast(&ReloadEvent::Failed("boom".to_string()));
    server.broadcast(&ReloadEvent::Reload);

    let mut stream = connect(&server);
    let frames = read_frames(&mut stream, 1);
    assert!(frames.contains("event: hello"), "{frames}");
    assert!(!frames.contains("event: failed"), "{frames}");
    assert!(frames.contains("\"generation\":1"), "{frames}");
}

#[test]
fn closed_tabs_are_dropped_from_the_roster() {
    let server = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    let stream = connect(&server);
    wait_for_clients(&server, 1);

    drop(stream);
    // The write that fails is what retires the client, so one broadcast is
    // needed to notice and a second to observe the roster shrink.
    let deadline = Instant::now() + Duration::from_secs(10);
    while server.client_count() != 0 {
        assert!(Instant::now() < deadline, "closed tab was never retired");
        server.broadcast(&ReloadEvent::Building);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn other_paths_are_rejected_without_taking_the_server_down() {
    let server = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, server.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET /favicon.ico HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 404"), "{response}");

    // The event stream still works after a stray request.
    let mut events = connect(&server);
    assert!(read_frames(&mut events, 1).contains("event: hello"));
}

#[test]
fn a_query_string_still_reaches_the_event_stream() {
    let server = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, server.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    write!(
        writer,
        "GET {DEV_EVENTS_PATH}?t=1&token={}&x=2 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        server.token()
    )
    .unwrap();

    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status).unwrap();
    assert!(status.starts_with("HTTP/1.1 200"), "{status}");
}

/// Send a raw request line and return the response status line.
fn status_line(server: &ReloadServer, target: &str) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, server.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(stream, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).unwrap();
    response
}

#[test]
fn the_event_stream_admits_only_this_sessions_token() {
    let server = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    let token = server.token().to_string();
    assert_eq!(token.len(), 32, "{token}");
    assert!(
        token.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{token}"
    );

    // A page from any other origin, or from an earlier `proa dev`, has no way
    // to present this token, so it can neither read build failures nor drive
    // a reload. `EventSource` treats the non-200 as fatal and stops retrying.
    for target in [
        DEV_EVENTS_PATH.to_string(),
        format!("{DEV_EVENTS_PATH}?token="),
        format!("{DEV_EVENTS_PATH}?token=00000000000000000000000000000000"),
        // The near-miss must differ from the real token: replacing the last
        // character with a fixed digit collides one run in sixteen.
        format!(
            "{DEV_EVENTS_PATH}?token={}{}",
            &token[..31],
            if token.ends_with('0') { '1' } else { '0' }
        ),
    ] {
        assert!(
            status_line(&server, &target).starts_with("HTTP/1.1 403"),
            "{target} was admitted"
        );
    }

    assert!(
        status_line(&server, &format!("{DEV_EVENTS_PATH}?token={token}"))
            .starts_with("HTTP/1.1 200")
    );
}

#[test]
fn two_dev_sessions_never_share_a_token() {
    let first = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    let second = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    assert_ne!(first.token(), second.token());
}

#[test]
fn constant_time_comparison_matches_plain_equality() {
    assert!(constant_time_eq("abc", "abc"));
    assert!(constant_time_eq("", ""));
    assert!(!constant_time_eq("abc", "abd"));
    assert!(!constant_time_eq("abc", "ab"));
    assert!(!constant_time_eq("ab", "abc"));
}

#[test]
fn a_busy_port_falls_back_without_squatting_on_the_next_site() {
    let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let taken = occupied.local_addr().unwrap().port();

    let server = ReloadServer::bind("127.0.0.1", taken).expect("bind beside the busy port");
    assert_ne!(server.port(), taken);

    // Scanning upward from the requested port would claim the ports a second
    // Proa site wants for its own dev server, and that site would then fail
    // to bind with an error naming neither live reload nor this process. The
    // neighbouring ports have to still be free afterwards.
    for offset in 1..=4 {
        let Some(neighbour) = taken.checked_add(offset) else {
            break;
        };
        if neighbour == server.port() {
            continue;
        }
        match TcpListener::bind((Ipv4Addr::LOCALHOST, neighbour)) {
            Ok(_) => {}
            // Another process on this machine may hold it; only a bind by
            // this server would be a defect, and that is excluded above.
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {}
            Err(error) => panic!("unexpected bind failure on {neighbour}: {error}"),
        }
    }

    let mut stream = connect(&server);
    assert!(read_frames(&mut stream, 1).contains("event: hello"));
}

#[test]
fn a_request_head_that_never_ends_is_refused_instead_of_growing_without_limit() {
    let server = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, server.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();

    // A request line with no newline in sight. `read_line` would otherwise
    // keep growing its buffer for as long as the client keeps typing.
    write!(stream, "GET /").unwrap();
    let filler = "a".repeat(4096);
    for _ in 0..8 {
        if stream.write_all(filler.as_bytes()).is_err() {
            break;
        }
    }
    let _ = stream.flush();

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    assert!(
        response.is_empty() || response.starts_with("HTTP/1.1 4"),
        "{response}"
    );

    // The server is still serving after refusing it.
    let mut events = connect(&server);
    assert!(read_frames(&mut events, 1).contains("event: hello"));
}

#[test]
fn cross_origin_headers_are_present_because_the_stream_lives_on_another_port() {
    let server = ReloadServer::bind("127.0.0.1", 0).expect("bind");
    let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, server.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    write!(
        writer,
        "GET {DEV_EVENTS_PATH}?token={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        server.token()
    )
    .unwrap();

    let mut reader = BufReader::new(stream);
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    assert!(
        headers.contains("Access-Control-Allow-Origin: *"),
        "{headers}"
    );
    assert!(
        headers.contains("Content-Type: text/event-stream"),
        "{headers}"
    );
    assert!(headers.contains("Cache-Control: no-store"), "{headers}");
}

#[test]
fn readiness_probes_every_address_the_host_could_be_listening_on() {
    // A wildcard is a bind address, not a destination.
    assert_eq!(
        probe_addresses("0.0.0.0", 3000),
        [SocketAddr::from((Ipv4Addr::LOCALHOST, 3000))]
    );
    assert_eq!(
        probe_addresses("::", 3000),
        [SocketAddr::from((Ipv6Addr::LOCALHOST, 3000))]
    );
    assert_eq!(
        probe_addresses("127.0.0.1", 3000),
        [SocketAddr::from((Ipv4Addr::LOCALHOST, 3000))]
    );

    // `localhost` resolves to ::1 first on most systems, so guessing
    // 127.0.0.1 would probe an address the server never bound. Every
    // resolved address has to be tried.
    let resolved = probe_addresses("localhost", 3000);
    assert!(!resolved.is_empty());
    assert!(
        resolved.iter().all(|address| address.ip().is_loopback()),
        "{resolved:?}"
    );
    assert!(resolved.iter().all(|address| address.port() == 3000));

    // An unresolvable name must still produce a usable guess rather than
    // stalling the dev loop.
    assert_eq!(
        probe_addresses("proa.invalid", 3000),
        [SocketAddr::from((Ipv4Addr::LOCALHOST, 3000))]
    );
}

#[test]
fn a_host_header_this_server_never_answers_for_is_refused() {
    // The DNS-rebinding path: an attacker domain that has been re-pointed at
    // loopback still arrives carrying its own name.
    assert!(!host_is_expected(Some("evil.example"), "127.0.0.1"));
    assert!(!host_is_expected(Some("evil.example:3211"), "127.0.0.1"));
    assert!(!host_is_expected(None, "127.0.0.1"));
    // A name is only loopback if it parses as one. A textual prefix test would
    // wave through the very name a rebinding attack arrives with.
    assert!(!host_is_expected(Some("127.evil.example"), "127.0.0.1"));
    assert!(!host_is_expected(
        Some("127.0.0.1.evil.example"),
        "127.0.0.1"
    ));
    assert!(!host_is_expected(
        Some("localhost.evil.example"),
        "127.0.0.1"
    ));

    assert!(host_is_expected(Some("127.0.0.1:3211"), "127.0.0.1"));
    assert!(host_is_expected(Some("localhost:3211"), "127.0.0.1"));
    assert!(host_is_expected(Some("[::1]:3211"), "127.0.0.1"));
    assert!(host_is_expected(Some("site.localhost:3211"), "127.0.0.1"));
    // The whole 127.0.0.0/8 block is loopback, not just 127.0.0.1.
    assert!(host_is_expected(Some("127.0.0.53:3211"), "127.0.0.1"));

    // A site deliberately served to other machines has to accept whatever
    // name reached it.
    assert!(host_is_expected(Some("192.168.1.20:3211"), "0.0.0.0"));
    // A server pinned to one address answers for that address.
    assert!(host_is_expected(Some("192.168.1.20:3211"), "192.168.1.20"));
    assert!(!host_is_expected(Some("192.168.1.99:3211"), "192.168.1.20"));
}

#[test]
fn readiness_waits_for_a_listener_and_gives_up_on_a_dead_port() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert!(wait_until_serving(
        "127.0.0.1",
        port,
        Duration::from_secs(5),
        &mut || true
    ));
    // An unrelated listener must not make an exited server look ready.
    assert!(!wait_until_serving(
        "127.0.0.1",
        port,
        Duration::from_secs(5),
        &mut || false
    ));

    drop(listener);
    let started = Instant::now();
    assert!(!wait_until_serving(
        "127.0.0.1",
        port,
        Duration::from_millis(200),
        &mut || true
    ));
    assert!(started.elapsed() < Duration::from_secs(5));

    // A server that died on boot is never going to answer, so the wait ends
    // immediately rather than burning the whole timeout.
    let started = Instant::now();
    assert!(!wait_until_serving(
        "127.0.0.1",
        port,
        Duration::from_secs(30),
        &mut || false
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
}
