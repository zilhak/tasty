//! Raw HTTP exercises the real tiny_http body readers, including incomplete chunks.

use super::{AckStatus, Screened};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const LIMIT: usize = 2048;

fn read_body(body: &[u8], framing: &str, finish: bool) -> Result<Value, ()> {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr().to_ip().unwrap();
    let worker = std::thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let mut request = Screened(request);
        let result = request.read_json_body(LIMIT).map_err(|_| ());
        request.respond(if result.is_err() {
            AckStatus::PayloadTooLarge
        } else {
            AckStatus::Received
        });
        result
    });
    let mut client = TcpStream::connect(addr).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        client,
        "POST / HTTP/1.1\r\nHost: localhost\r\n{framing}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    if framing.starts_with("Transfer-Encoding") {
        // An unfinished oversized chunk must not make the reader await its tail.
        let size = body.len() + if finish { 0 } else { 100 };
        write!(client, "{size:x}\r\n").unwrap();
        client.write_all(body).unwrap();
        if finish {
            client.write_all(b"\r\n0\r\n\r\n").unwrap();
        }
    } else {
        client.write_all(body).unwrap();
    }
    let mut response = [0; 128];
    assert!(client.read(&mut response).unwrap() > 0);
    // Release the peer after observing the response, before joining cleanup.
    drop(client);
    worker.join().unwrap()
}

#[test]
fn raw_chunked_utf8_boundaries() {
    for size in [LIMIT - 1, LIMIT, LIMIT + 1] {
        let mut valid = b"{}".to_vec();
        valid.resize(size, b' ');
        for invalid in [false, true] {
            let mut body = valid.clone();
            if invalid {
                body[0] = 0xff;
            }
            let actual = read_body(&body, "Transfer-Encoding: chunked", size <= LIMIT);
            let expected = if size > LIMIT {
                Err(())
            } else if invalid {
                Ok(Value::Null)
            } else {
                Ok(json!({}))
            };
            assert_eq!(actual, expected, "size={size}, invalid={invalid}");
        }
    }
}

#[test]
fn raw_chunked_cap_can_split_a_valid_multibyte_character() {
    let mut body = vec![b' '; LIMIT];
    body.extend_from_slice("한".as_bytes());
    assert!(read_body(&body, "Transfer-Encoding: chunked", false).is_err());
}

#[test]
fn raw_content_length_rejects_before_waiting_for_body() {
    assert!(read_body(&[], &format!("Content-Length: {}", LIMIT + 1), true).is_err());
}

#[test]
fn raw_content_length_preserves_json_and_invalid_utf8_policy() {
    assert_eq!(read_body(b"{}", "Content-Length: 2", true), Ok(json!({})));
    assert_eq!(
        read_body(b"\xff", "Content-Length: 1", true),
        Ok(Value::Null)
    );
}
