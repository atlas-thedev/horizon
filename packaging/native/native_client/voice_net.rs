//! `VoiceNet`: the UDP link to the Native voice relay (server/voice.js), the same packets native-mod sends:
//! `[0x4E][type][sid u32][seq u32][AES-128-GCM(payload) + tag]`, nonce `sid | seq | dir` (0 up, 1 down), the
//! 10-byte header as associated data. One socket serves the call; the sid says which room (and key) a packet is for.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Nonce};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const T_AUDIO: u8 = 1;
pub const T_PING: u8 = 2;
pub const T_BYE: u8 = 3;
const MAGIC: u8 = 0x4E;
const HEADER: usize = 10;
const TAG: usize = 16;

fn nonce(sid: u32, seq: u32, dir: u32) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[0..4].copy_from_slice(&sid.to_be_bytes());
    n[4..8].copy_from_slice(&seq.to_be_bytes());
    n[8..12].copy_from_slice(&dir.to_be_bytes());
    n
}

/// One packet: the header in the clear (and authenticated), the payload encrypted. `dir` 0 is to the server.
pub fn seal(kind: u8, sid: u32, seq: u32, key: &[u8; 16], plain: &[u8], dir: u32) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(HEADER + plain.len() + TAG);
    out.push(MAGIC);
    out.push(kind);
    out.extend_from_slice(&sid.to_be_bytes());
    out.extend_from_slice(&seq.to_be_bytes());
    let cipher = Aes128Gcm::new_from_slice(key).ok()?;
    let body = cipher.encrypt(Nonce::from_slice(&nonce(sid, seq, dir)), Payload { msg: plain, aad: &out[..HEADER] }).ok()?;
    out.extend_from_slice(&body);
    Some(out)
}

/// The header of a packet: (type, sid, seq), or None when it is not one of ours.
pub fn header(buf: &[u8]) -> Option<(u8, u32, u32)> {
    if buf.len() < HEADER + TAG || buf[0] != MAGIC {
        return None;
    }
    Some((buf[1], u32::from_be_bytes(buf[2..6].try_into().ok()?), u32::from_be_bytes(buf[6..10].try_into().ok()?)))
}

/// The payload of a packet sealed for `dir`, or None when it was not sealed with `key` (or was changed on the way).
pub fn open(buf: &[u8], key: &[u8; 16], dir: u32) -> Option<Vec<u8>> {
    let (_, sid, seq) = header(buf)?;
    let cipher = Aes128Gcm::new_from_slice(key).ok()?;
    cipher.decrypt(Nonce::from_slice(&nonce(sid, seq, dir)), Payload { msg: &buf[HEADER..], aad: &buf[..HEADER] }).ok()
}

/// The socket and its receiving thread. Dropping it stops the thread.
pub struct Net {
    pub host: String,
    pub port: u16,
    server: SocketAddr,
    socket: UdpSocket,
    running: Arc<AtomicBool>,
}

impl Net {
    /// Opens the link; every packet that arrives goes to `on_packet(type, sid, seq, raw packet)`.
    pub fn open(host: &str, port: u16, on_packet: fn(u8, u32, u32, &[u8])) -> std::io::Result<Net> {
        let server = (host, port)
            .to_socket_addrs()?
            .find(SocketAddr::is_ipv4) // the relay listens on IPv4
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("Can't find the voice server {host}.")))?;
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;
        let running = Arc::new(AtomicBool::new(true));
        let (rx, on) = (socket.try_clone()?, running.clone());
        std::thread::Builder::new().name("Native voice net".into()).spawn(move || {
            let mut buf = [0u8; 2048];
            while on.load(Ordering::Relaxed) {
                match rx.recv_from(&mut buf) {
                    Ok((n, from)) if from == server => {
                        if let Some((kind, sid, seq)) = header(&buf[..n]) {
                            on_packet(kind, sid, seq, &buf[..n]);
                        }
                    }
                    Ok(_) => {} // not from the relay
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                    Err(e) => {
                        if on.load(Ordering::Relaxed) {
                            log::debug!("Native voice: receive failed: {e}");
                            std::thread::sleep(Duration::from_millis(200));
                        }
                    }
                }
            }
        })?;
        Ok(Net { host: host.to_owned(), port, server, socket, running })
    }

    pub fn same_server(&self, host: &str, port: u16) -> bool {
        self.host == host && self.port == port
    }

    pub fn send(&self, kind: u8, sid: u32, seq: u32, key: &[u8; 16], plain: &[u8]) {
        if let Some(packet) = seal(kind, sid, seq, key, plain, 0)
            && let Err(e) = self.socket.send_to(&packet, self.server)
        {
            log::debug!("Native voice: send failed: {e}");
        }
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packet_opens_with_its_key_and_direction_only() {
        let key = [7u8; 16];
        let p = seal(T_AUDIO, 0xDEAD_BEEF, 42, &key, b"opus", 0).unwrap();
        assert_eq!(p.len(), HEADER + 4 + TAG);
        assert_eq!(header(&p), Some((T_AUDIO, 0xDEAD_BEEF, 42)));
        assert_eq!(open(&p, &key, 0).as_deref(), Some(&b"opus"[..]));
        assert_eq!(open(&p, &key, 1), None, "a packet to the server is not one from it");
        assert_eq!(open(&p, &[8u8; 16], 0), None, "another call's key");
        let mut changed = p.clone();
        changed[5] ^= 1; // a different sid in the clear header
        assert_eq!(open(&changed, &key, 0), None, "the header is authenticated");
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn a_packet_from_the_relay_opens() {
        // made by server/voice.js's seal() (node crypto, aes-128-gcm): forwarded audio from sid 5, its seq 9, "opus"
        let key: [u8; 16] = unhex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
        let p = unhex("4e01010203040000000777584b3a5e32cc020b33285f984cd302852578a564d759aafb47b1c720");
        assert_eq!(header(&p), Some((T_AUDIO, 0x0102_0304, 7)));
        let plain = open(&p, &key, 1).expect("the relay's packet opens");
        assert_eq!(plain, unhex("0000000501000000096f707573"));
        // and the other way round: what we seal is byte for byte what the relay would
        assert_eq!(seal(T_AUDIO, 0x0102_0304, 7, &key, &plain, 1).unwrap(), p);
    }

    #[test]
    fn the_packet_layout_is_the_relays() {
        // [0x4E][type][sid][seq] then the ciphertext: what server/voice.js reads with readUInt32BE(2) and (6)
        let p = seal(T_PING, 1, 2, &[0u8; 16], &[0u8; 8], 1).unwrap();
        assert_eq!(&p[..10], &[0x4E, 2, 0, 0, 0, 1, 0, 0, 0, 2]);
        assert_eq!(header(&p[..20]), None, "too short for a tag");
    }
}
