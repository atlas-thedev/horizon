//! Native Simple Voice Chat (`de.maxhenkel.voicechat`, compatibility version 18) client for Rust Edition (Horizon).
//!
//! Speaks the exact Minecraft plugin-channel handshake (`voicechat:request_secret`, `voicechat:secret`,
//! `voicechat:player_state`, `voicechat:player_states`, `voicechat:add_group`, `voicechat:remove_group`,
//! `voicechat:joined_group`, `voicechat:update_state`, `voicechat:create_group`, `voicechat:join_group`,
//! `voicechat:leave_group`) and AES-128-CBC encrypted UDP voice transport (`[0xFF][player_uuid][iv][AES-128-CBC-PKCS7]`)
//! so Rust Edition players can use proximity voice & server groups on any server running Simple Voice Chat, sharing
//! Native Voice's `cpal` + WebRTC (`sonora`) + Opus (`opus-rs`) audio engine.

use super::voice_audio::{self, FRAME, Stream};
use aes_gcm::aes::Aes128;
use aes_gcm::aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Simple Voice Chat compatibility version (matches SVC 2.5.x servers/plugins).
pub const COMPAT_VERSION: i32 = 18;

pub const CH_REQUEST_SECRET: &str = "voicechat:request_secret";
pub const CH_SECRET: &str = "voicechat:secret";
pub const CH_PLAYER_STATE: &str = "voicechat:player_state";
pub const CH_PLAYER_STATES: &str = "voicechat:player_states";
pub const CH_UPDATE_STATE: &str = "voicechat:update_state";
pub const CH_ADD_GROUP: &str = "voicechat:add_group";
pub const CH_REMOVE_GROUP: &str = "voicechat:remove_group";
pub const CH_JOINED_GROUP: &str = "voicechat:joined_group";
pub const CH_CREATE_GROUP: &str = "voicechat:create_group";
pub const CH_JOIN_GROUP: &str = "voicechat:join_group";
pub const CH_LEAVE_GROUP: &str = "voicechat:leave_group";

const UDP_MAGIC: u8 = 0xFF;
const PKT_MIC: u8 = 0x01;
const PKT_PLAYER_SOUND: u8 = 0x02;
const PKT_GROUP_SOUND: u8 = 0x03;
const PKT_LOCATION_SOUND: u8 = 0x04;
const PKT_AUTHENTICATE: u8 = 0x05;
const PKT_AUTHENTICATE_ACK: u8 = 0x06;
const PKT_PING: u8 = 0x07;
const PKT_KEEP_ALIVE: u8 = 0x08;
const PKT_CONNECTION_CHECK: u8 = 0x09;
const PKT_CONNECTION_CHECK_ACK: u8 = 0x0A;

#[derive(Clone, Debug, PartialEq)]
pub struct SecretConfig {
    pub secret: [u8; 16],
    pub server_port: i32,
    pub player_uuid: [u8; 16],
    pub codec: u8,
    pub mtu_size: i32,
    pub voice_chat_distance: f64,
    pub keep_alive_ms: i32,
    pub groups_enabled: bool,
    pub voice_host: String,
    pub allow_recording: bool,
}

#[derive(Clone, Debug)]
pub struct SvcGroup {
    pub id: [u8; 16],
    pub name: String,
    pub has_password: bool,
    pub persistent: bool,
    pub hidden: bool,
    pub group_type: i16,
}

#[derive(Clone, Debug)]
pub struct SvcPlayerState {
    pub uuid: [u8; 16],
    pub name: String,
    pub disabled: bool,
    pub disconnected: bool,
    pub group: Option<[u8; 16]>,
}

struct SvcStream {
    stream: Stream,
    gain: f32,
}

struct UdpHandle {
    socket: UdpSocket,
    server: SocketAddr,
    player_uuid: [u8; 16],
    secret: [u8; 16],
    running: Arc<AtomicBool>,
    seq: AtomicI64,
    authenticated: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
}

impl UdpHandle {
    fn send_packet(&self, inner: &[u8]) {
        if let Some(pkt) = seal_udp(&self.player_uuid, &self.secret, inner) {
            let _ = self.socket.send_to(&pkt, self.server);
        }
    }
}

impl Drop for UdpHandle {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct State {
    enabled: bool,
    auto_mute_in_call: bool,
    server_address: Option<String>,
    secret: Option<SecretConfig>,
    udp: Option<Arc<UdpHandle>>,
    outgoing: VecDeque<(String, Vec<u8>)>,
    groups: HashMap<[u8; 16], SvcGroup>,
    joined_group: Option<[u8; 16]>,
    players: HashMap<[u8; 16], SvcPlayerState>,
    listener_pos: [f64; 3],
    player_positions: HashMap<[u8; 16], [f64; 3]>,
    streams: HashMap<[u8; 16], SvcStream>,
    status: Option<String>,
    last_keepalive: Option<Instant>,
    sent_end: bool,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(State {
            enabled: true,
            auto_mute_in_call: true,
            ..Default::default()
        })
    })
}

fn lock() -> MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

// ── AES-128-CBC + PKCS7 (built on aes-gcm's bundled Aes128 block cipher: zero extra deps) ──

fn next_iv() -> [u8; 16] {
    static COUNTER: AtomicI64 = AtomicI64::new(1);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0u128, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) as u64;
    // Mix time + counter + stack address with splitmix64 for unique per-packet CBC IVs
    let mut x = (nanos as u64) ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut y = ((nanos >> 64) as u64) ^ (&n as *const u64 as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let mix = |mut z: u64| {
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    x = mix(x);
    y = mix(y ^ x);
    let mut iv = [0u8; 16];
    iv[..8].copy_from_slice(&x.to_be_bytes());
    iv[8..].copy_from_slice(&y.to_be_bytes());
    iv
}

pub fn aes_cbc_encrypt(key: &[u8; 16], iv: &[u8; 16], plain: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let pad = 16 - (plain.len() % 16);
    let total = plain.len() + pad;
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(plain);
    buf.resize(total, pad as u8);
    let mut prev = *iv;
    for chunk in buf.chunks_exact_mut(16) {
        for i in 0..16 {
            chunk[i] ^= prev[i];
        }
        let block = GenericArray::from_mut_slice(chunk);
        cipher.encrypt_block(block);
        prev.copy_from_slice(chunk);
    }
    buf
}

pub fn aes_cbc_decrypt(key: &[u8; 16], iv: &[u8; 16], ciphertext: &[u8]) -> Option<Vec<u8>> {
    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return None;
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut out = ciphertext.to_vec();
    let mut prev = *iv;
    for chunk in out.chunks_exact_mut(16) {
        let mut next_prev = [0u8; 16];
        next_prev.copy_from_slice(chunk);
        let block = GenericArray::from_mut_slice(chunk);
        cipher.decrypt_block(block);
        for i in 0..16 {
            chunk[i] ^= prev[i];
        }
        prev = next_prev;
    }
    let pad = *out.last()? as usize;
    if pad == 0 || pad > 16 || pad > out.len() {
        return None;
    }
    if !out[out.len() - pad..].iter().all(|&b| b as usize == pad) {
        return None;
    }
    out.truncate(out.len() - pad);
    Some(out)
}

// ── Minecraft FriendlyByteBuf wire helpers ────────────────────────────────

struct BufReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BufReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.data.len() {
            return None;
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn bool(&mut self) -> Option<bool> {
        Some(self.u8()? != 0)
    }

    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }

    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }

    fn uuid(&mut self) -> Option<[u8; 16]> {
        self.take(16)?.try_into().ok()
    }

    fn varint(&mut self) -> Option<i32> {
        let (mut result, mut shift) = (0i32, 0u32);
        loop {
            let b = self.u8()?;
            result |= i32::from(b & 0x7F) << shift;
            if b & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 35 {
                return None;
            }
        }
    }

    fn bytes(&mut self, max: usize) -> Option<Vec<u8>> {
        let len = usize::try_from(self.varint()?).ok()?;
        if len > max {
            return None;
        }
        Some(self.take(len)?.to_vec())
    }

    fn string(&mut self, max_bytes: usize) -> Option<String> {
        let raw = self.bytes(max_bytes)?;
        String::from_utf8(raw).ok()
    }
}

fn write_varint(out: &mut Vec<u8>, mut v: i32) {
    let mut u = v as u32;
    loop {
        if (u & !0x7F) == 0 {
            out.push(u as u8);
            return;
        }
        out.push(((u & 0x7F) | 0x80) as u8);
        u >>= 7;
        v = u as i32;
        let _ = v;
    }
}

fn write_bytes(out: &mut Vec<u8>, data: &[u8]) {
    write_varint(out, data.len() as i32);
    out.extend_from_slice(data);
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    write_bytes(out, s.as_bytes());
}

// ── Packet codecs ─────────────────────────────────────────────────────────

pub fn encode_request_secret(compat_version: i32) -> Vec<u8> {
    compat_version.to_be_bytes().to_vec()
}

pub fn decode_secret(data: &[u8]) -> Option<SecretConfig> {
    let mut r = BufReader::new(data);
    let secret = r.uuid()?;
    let server_port = r.i32()?;
    let player_uuid = r.uuid()?;
    let codec = r.u8()?;
    let mtu_size = r.i32()?;
    let voice_chat_distance = r.f64()?;
    let keep_alive_ms = r.i32()?;
    let groups_enabled = r.bool()?;
    let voice_host = r.string(32767)?;
    let allow_recording = r.bool()?;
    Some(SecretConfig {
        secret,
        server_port,
        player_uuid,
        codec,
        mtu_size,
        voice_chat_distance,
        keep_alive_ms,
        groups_enabled,
        voice_host,
        allow_recording,
    })
}

fn decode_group(r: &mut BufReader<'_>) -> Option<SvcGroup> {
    let id = r.uuid()?;
    let name = r.string(512)?;
    let has_password = r.bool()?;
    let persistent = r.bool()?;
    let hidden = r.bool()?;
    let group_type = r.i16()?;
    Some(SvcGroup { id, name, has_password, persistent, hidden, group_type })
}

fn decode_player_state(r: &mut BufReader<'_>) -> Option<SvcPlayerState> {
    let disabled = r.bool()?;
    let disconnected = r.bool()?;
    let uuid = r.uuid()?;
    let name = r.string(128)?;
    let group = if r.bool()? { Some(r.uuid()?) } else { None };
    Some(SvcPlayerState { uuid, name, disabled, disconnected, group })
}

/// Seals one inner SVC packet into `[0xFF][player_uuid][iv][AES-128-CBC-PKCS7(inner)]`.
pub fn seal_udp(player_uuid: &[u8; 16], secret: &[u8; 16], inner: &[u8]) -> Option<Vec<u8>> {
    let iv = next_iv();
    let cipher = aes_cbc_encrypt(secret, &iv, inner);
    let mut out = Vec::with_capacity(1 + 16 + 16 + cipher.len());
    out.push(UDP_MAGIC);
    out.extend_from_slice(player_uuid);
    out.extend_from_slice(&iv);
    out.extend_from_slice(&cipher);
    Some(out)
}

/// Opens a server-to-client SVC UDP packet (`[iv 16B][AES-128-CBC-PKCS7(inner)]` or with leading magic+uuid).
pub fn open_udp(raw: &[u8], secret: &[u8; 16]) -> Option<Vec<u8>> {
    // Simple Voice Chat server -> client UDP packets are [iv 16B][ciphertext] (no magic/uuid prefix on S->C).
    if raw.len() >= 32 && (raw.len() - 16) % 16 == 0 {
        let iv: [u8; 16] = raw[..16].try_into().ok()?;
        if let Some(plain) = aes_cbc_decrypt(secret, &iv, &raw[16..]) {
            return Some(plain);
        }
    }
    // Fallback if wrapped with [0xFF][uuid 16B][iv 16B][ciphertext]
    if raw.len() >= 49 && raw[0] == UDP_MAGIC && (raw.len() - 33) % 16 == 0 {
        let iv: [u8; 16] = raw[17..33].try_into().ok()?;
        return aes_cbc_decrypt(secret, &iv, &raw[33..]);
    }
    None
}

fn encode_authenticate(player_uuid: &[u8; 16], secret: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 16 + 16);
    out.push(PKT_AUTHENTICATE);
    out.extend_from_slice(player_uuid);
    out.extend_from_slice(secret);
    out
}

fn encode_mic(opus: &[u8], seq: i64, whispering: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 5 + opus.len() + 8 + 1);
    out.push(PKT_MIC);
    write_bytes(&mut out, opus);
    out.extend_from_slice(&seq.to_be_bytes());
    out.push(u8::from(whispering));
    out
}

// ── Public lifecycle API (called from Horizon Game + voice_modules + voice_audio) ──

pub fn configure(enabled: bool, auto_mute_in_call: bool) {
    let mut s = lock();
    let changed = s.enabled != enabled;
    s.enabled = enabled;
    s.auto_mute_in_call = auto_mute_in_call;
    if changed && !enabled {
        s.udp = None;
        s.secret = None;
        s.streams.clear();
        s.status = None;
    }
}

pub fn auto_mute_in_call() -> bool {
    lock().auto_mute_in_call
}

pub fn active() -> bool {
    let s = lock();
    s.enabled && s.udp.as_ref().is_some_and(|u| u.authenticated.load(Ordering::Relaxed))
}

pub fn status() -> Option<String> {
    let s = lock();
    if !s.enabled {
        return None;
    }
    if let Some(u) = &s.udp {
        if u.connected.load(Ordering::Relaxed) {
            return Some("Simple Voice Chat: Connected".to_owned());
        }
        if u.authenticated.load(Ordering::Relaxed) {
            return Some("Simple Voice Chat: Authenticating...".to_owned());
        }
        return Some("Simple Voice Chat: Connecting UDP...".to_owned());
    }
    s.status.clone()
}

pub fn groups() -> Vec<SvcGroup> {
    lock().groups.values().cloned().collect()
}

pub fn joined_group() -> Option<[u8; 16]> {
    lock().joined_group
}

/// Called when the client logs into a multiplayer server.
pub fn on_login(server_address: Option<&str>) {
    let mut s = lock();
    s.udp = None;
    s.secret = None;
    s.groups.clear();
    s.joined_group = None;
    s.players.clear();
    s.streams.clear();
    s.outgoing.clear();
    s.status = None;
    s.server_address = server_address.map(str::to_owned);
    if s.enabled && server_address.is_some() {
        s.outgoing.push_back((CH_REQUEST_SECRET.to_owned(), encode_request_secret(COMPAT_VERSION)));
    }
}

/// Called when disconnecting from a multiplayer server.
pub fn on_disconnect() {
    let mut s = lock();
    s.udp = None;
    s.secret = None;
    s.server_address = None;
    s.groups.clear();
    s.joined_group = None;
    s.players.clear();
    s.streams.clear();
    s.outgoing.clear();
    s.status = None;
}

/// Drains outgoing Minecraft `Serverbound::CustomPayload` packets queued for the server.
pub fn take_outgoing() -> Vec<(String, Vec<u8>)> {
    let mut s = lock();
    if s.outgoing.is_empty() {
        return Vec::new();
    }
    s.outgoing.drain(..).collect()
}

/// Updates local listener and nearby player positions for proximity attenuation.
pub fn update_positions(listener: [f64; 3], players: &[(u128, [f64; 3])]) {
    let mut s = lock();
    if s.udp.is_none() {
        return;
    }
    s.listener_pos = listener;
    s.player_positions.clear();
    for &(uuid, pos) in players {
        s.player_positions.insert(uuid.to_be_bytes(), pos);
    }
}

/// Periodic tick from the game loop: sends SVC UDP keep-alives & auth retries.
pub fn tick() {
    let (udp, ka_ms) = {
        let mut s = lock();
        let Some(u) = s.udp.clone() else { return };
        let ka = s.secret.as_ref().map_or(1000, |c| c.keep_alive_ms.clamp(250, 10_000)) as u64;
        if s.last_keepalive.is_some_and(|t| t.elapsed() < Duration::from_millis(ka)) {
            return;
        }
        s.last_keepalive = Some(Instant::now());
        (u, ka)
    };
    let _ = ka_ms;
    if !udp.authenticated.load(Ordering::Relaxed) {
        udp.send_packet(&encode_authenticate(&udp.player_uuid, &udp.secret));
    } else if !udp.connected.load(Ordering::Relaxed) {
        udp.send_packet(&[PKT_CONNECTION_CHECK]);
    } else {
        udp.send_packet(&[PKT_KEEP_ALIVE]);
    }
}

/// Create a server-side Simple Voice Chat group (requires `groups_enabled` on the server).
pub fn create_group(name: &str, password: Option<&str>, group_type: i16) {
    let mut out = Vec::new();
    write_string(&mut out, name.trim());
    match password.filter(|p| !p.is_empty()) {
        Some(pw) => {
            out.push(1);
            write_string(&mut out, pw);
        }
        None => out.push(0),
    }
    out.extend_from_slice(&group_type.to_be_bytes());
    lock().outgoing.push_back((CH_CREATE_GROUP.to_owned(), out));
}

/// Join a server-side Simple Voice Chat group.
pub fn join_group(group_id: [u8; 16], password: Option<&str>) {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(&group_id);
    match password.filter(|p| !p.is_empty()) {
        Some(pw) => {
            out.push(1);
            write_string(&mut out, pw);
        }
        None => out.push(0),
    }
    lock().outgoing.push_back((CH_JOIN_GROUP.to_owned(), out));
}

/// Leave the current server-side Simple Voice Chat group.
pub fn leave_group() {
    lock().outgoing.push_back((CH_LEAVE_GROUP.to_owned(), Vec::new()));
}

/// Handles an incoming Minecraft `Clientbound::CustomPayload` on any `voicechat:*` channel.
pub fn on_custom_payload(channel: &str, data: &[u8]) {
    let mut s = lock();
    if !s.enabled {
        return;
    }
    match channel {
        CH_SECRET => {
            let Some(cfg) = decode_secret(data) else {
                log::debug!("Simple Voice Chat: malformed secret payload ({} bytes)", data.len());
                return;
            };
            let host = resolve_voice_host(&cfg.voice_host, s.server_address.as_deref());
            let port = resolve_voice_port(cfg.server_port, s.server_address.as_deref());
            s.secret = Some(cfg.clone());
            match start_udp(&host, port, cfg.player_uuid, cfg.secret) {
                Ok(handle) => {
                    s.udp = Some(handle);
                    s.last_keepalive = None;
                    s.status = Some("Simple Voice Chat: Connecting...".to_owned());
                    // Tell server we are enabled & connected
                    s.outgoing.push_back((CH_UPDATE_STATE.to_owned(), vec![0u8]));
                    drop(s);
                    voice_audio::start();
                }
                Err(e) => {
                    s.status = Some(format!("Simple Voice Chat: {e}"));
                    log::debug!("Simple Voice Chat UDP open failed ({host}:{port}): {e}");
                }
            }
        }
        CH_PLAYER_STATE => {
            let mut r = BufReader::new(data);
            if let Some(ps) = decode_player_state(&mut r) {
                s.players.insert(ps.uuid, ps);
            }
        }
        CH_PLAYER_STATES => {
            let mut r = BufReader::new(data);
            if let Some(count) = r.i32() {
                for _ in 0..count.clamp(0, 4096) {
                    if let Some(ps) = decode_player_state(&mut r) {
                        s.players.insert(ps.uuid, ps);
                    } else {
                        break;
                    }
                }
            }
        }
        CH_ADD_GROUP => {
            let mut r = BufReader::new(data);
            if let Some(g) = decode_group(&mut r) {
                s.groups.insert(g.id, g);
            }
        }
        CH_REMOVE_GROUP => {
            let mut r = BufReader::new(data);
            if let Some(id) = r.uuid() {
                s.groups.remove(&id);
                if s.joined_group == Some(id) {
                    s.joined_group = None;
                }
            }
        }
        CH_JOINED_GROUP => {
            let mut r = BufReader::new(data);
            s.joined_group = if r.bool().unwrap_or(false) { r.uuid() } else { None };
        }
        _ => {}
    }
}

fn resolve_voice_host(voice_host: &str, server_address: Option<&str>) -> String {
    let trimmed = voice_host.trim();
    if !trimmed.is_empty() {
        return trimmed.split(':').next().unwrap_or(trimmed).to_owned();
    }
    let addr = server_address.unwrap_or("127.0.0.1").trim();
    if let Some(rest) = addr.strip_prefix('[') {
        rest.split(']').next().unwrap_or("127.0.0.1").to_owned()
    } else {
        addr.split(':').next().unwrap_or("127.0.0.1").to_owned()
    }
}

fn resolve_voice_port(server_port: i32, server_address: Option<&str>) -> u16 {
    if server_port > 0 && server_port <= 65535 {
        return server_port as u16;
    }
    if let Some(addr) = server_address
        && let Some((_, p)) = addr.rsplit_once(':')
        && let Ok(port) = p.parse::<u16>()
    {
        return port;
    }
    24454
}

fn start_udp(host: &str, port: u16, player_uuid: [u8; 16], secret: [u8; 16]) -> std::io::Result<Arc<UdpHandle>> {
    let server = (host, port)
        .to_socket_addrs()?
        .find(SocketAddr::is_ipv4)
        .or_else(|| (host, port).to_socket_addrs().ok()?.next())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("Cannot resolve {host}:{port}")))?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    let handle = Arc::new(UdpHandle {
        socket: socket.try_clone()?,
        server,
        player_uuid,
        secret,
        running: Arc::new(AtomicBool::new(true)),
        seq: AtomicI64::new(0),
        authenticated: Arc::new(AtomicBool::new(false)),
        connected: Arc::new(AtomicBool::new(false)),
    });
    // Send initial authenticate packet immediately
    handle.send_packet(&encode_authenticate(&player_uuid, &secret));
    let rx_handle = handle.clone();
    std::thread::Builder::new().name("Native SVC net".into()).spawn(move || {
        let mut buf = [0u8; 2048];
        while rx_handle.running.load(Ordering::Relaxed) {
            match socket.recv_from(&mut buf) {
                Ok((n, from)) if from == server => {
                    if let Some(plain) = open_udp(&buf[..n], &rx_handle.secret) {
                        on_udp_packet(&rx_handle, &plain);
                    }
                }
                Ok(_) => {}
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => {
                    if rx_handle.running.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        }
    })?;
    Ok(handle)
}

fn on_udp_packet(handle: &UdpHandle, plain: &[u8]) {
    let mut r = BufReader::new(plain);
    let Some(kind) = r.u8() else { return };
    match kind {
        PKT_AUTHENTICATE_ACK => {
            handle.authenticated.store(true, Ordering::Relaxed);
            handle.send_packet(&[PKT_CONNECTION_CHECK]);
        }
        PKT_CONNECTION_CHECK_ACK => {
            handle.authenticated.store(true, Ordering::Relaxed);
            handle.connected.store(true, Ordering::Relaxed);
        }
        PKT_PING => {
            // Echo ping straight back to the server
            handle.send_packet(plain);
        }
        PKT_KEEP_ALIVE => {
            handle.send_packet(&[PKT_KEEP_ALIVE]);
        }
        PKT_PLAYER_SOUND => {
            let Some(_channel_id) = r.uuid() else { return };
            let Some(sender) = r.uuid() else { return };
            let Some(opus) = r.bytes(2048) else { return };
            let Some(seq) = r.i64() else { return };
            let Some(distance) = r.f32() else { return };
            let mut s = lock();
            let max_dist = if distance > 0.0 { f64::from(distance) } else { s.secret.as_ref().map_or(48.0, |c| c.voice_chat_distance) };
            let gain = s
                .player_positions
                .get(&sender)
                .map(|p| {
                    let dx = p[0] - s.listener_pos[0];
                    let dy = p[1] - s.listener_pos[1];
                    let dz = p[2] - s.listener_pos[2];
                    let d = (dx * dx + dy * dy + dz * dz).sqrt();
                    (1.0 - (d / max_dist.max(1.0))).clamp(0.0, 1.0) as f32
                })
                .unwrap_or(1.0);
            push_svc_audio(&mut s, sender, seq as u32, opus, gain);
        }
        PKT_GROUP_SOUND => {
            let Some(_channel_id) = r.uuid() else { return };
            let Some(sender) = r.uuid() else { return };
            let Some(opus) = r.bytes(2048) else { return };
            let Some(seq) = r.i64() else { return };
            let mut s = lock();
            push_svc_audio(&mut s, sender, seq as u32, opus, 1.0);
        }
        PKT_LOCATION_SOUND => {
            let Some(_channel_id) = r.uuid() else { return };
            let Some(sender) = r.uuid() else { return };
            let (Some(x), Some(y), Some(z)) = (r.f64(), r.f64(), r.f64()) else { return };
            let Some(opus) = r.bytes(2048) else { return };
            let Some(seq) = r.i64() else { return };
            let Some(distance) = r.f32() else { return };
            let mut s = lock();
            let max_dist = if distance > 0.0 { f64::from(distance) } else { s.secret.as_ref().map_or(48.0, |c| c.voice_chat_distance) };
            let dx = x - s.listener_pos[0];
            let dy = y - s.listener_pos[1];
            let dz = z - s.listener_pos[2];
            let d = (dx * dx + dy * dy + dz * dz).sqrt();
            let gain = (1.0 - (d / max_dist.max(1.0))).clamp(0.0, 1.0) as f32;
            push_svc_audio(&mut s, sender, seq as u32, opus, gain);
        }
        _ => {}
    }
}

fn push_svc_audio(s: &mut State, sender: [u8; 16], seq: u32, opus: Vec<u8>, gain: f32) {
    if opus.is_empty() {
        // Empty Opus packet in Simple Voice Chat signals end-of-speech
        if let Some(st) = s.streams.get_mut(&sender) {
            st.stream.push(seq, voice_audio::END, Vec::new());
        }
        return;
    }
    if !s.streams.contains_key(&sender) {
        let Some(stream) = Stream::new() else { return };
        s.streams.insert(sender, SvcStream { stream, gain });
    }
    if let Some(st) = s.streams.get_mut(&sender) {
        st.gain = gain;
        st.stream.push(seq, 0, opus);
    }
}

/// Sends one encoded 20 ms Opus microphone frame (or empty slice at end of speech) over SVC UDP.
pub fn send_mic(opus: &[u8], whispering: bool) {
    let udp = {
        let mut s = lock();
        if opus.is_empty() {
            if s.sent_end {
                return;
            }
            s.sent_end = true;
        } else {
            s.sent_end = false;
        }
        s.udp.clone()
    };
    if let Some(u) = udp
        && u.connected.load(Ordering::Relaxed)
    {
        let seq = u.seq.fetch_add(1, Ordering::Relaxed);
        u.send_packet(&encode_mic(opus, seq, whispering));
    }
}

/// Mixes all active Simple Voice Chat proximity/group streams into `mix`.
pub fn mix_into(mix: &mut [f32; FRAME], out_volume: f32, deafened: bool) {
    let mut s = lock();
    if s.streams.is_empty() {
        return;
    }
    for st in s.streams.values_mut() {
        let Some(pcm) = st.stream.pull() else { continue };
        if deafened || st.gain <= 0.001 {
            continue;
        }
        let g = out_volume * st.gain / 32768.0;
        for (m, v) in mix.iter_mut().zip(pcm.iter()) {
            *m += f32::from(*v) * g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_cbc_pkcs7_roundtrip_and_padding_checks() {
        let key = [0x11u8; 16];
        let iv = [0x22u8; 16];
        for len in [0usize, 1, 15, 16, 17, 31, 32, 100] {
            let plain: Vec<u8> = (0..len).map(|i| (i * 37) as u8).collect();
            let enc = aes_cbc_encrypt(&key, &iv, &plain);
            assert_eq!(enc.len() % 16, 0);
            let dec = aes_cbc_decrypt(&key, &iv, &enc).expect("valid ciphertext decrypts");
            assert_eq!(dec, plain);
        }
        let mut bad = aes_cbc_encrypt(&key, &iv, b"hello voicechat");
        *bad.last_mut().unwrap() ^= 0x01;
        assert!(aes_cbc_decrypt(&key, &iv, &bad).is_none(), "corrupted padding rejected");
    }

    #[test]
    fn secret_payload_decodes_svc_v18_layout() {
        let mut buf = Vec::new();
        let secret = [0xAAu8; 16];
        let uuid = [0xBBu8; 16];
        buf.extend_from_slice(&secret);
        buf.extend_from_slice(&24454i32.to_be_bytes());
        buf.extend_from_slice(&uuid);
        buf.push(0); // VOIP codec
        buf.extend_from_slice(&1024i32.to_be_bytes());
        buf.extend_from_slice(&48.0f64.to_be_bytes());
        buf.extend_from_slice(&1000i32.to_be_bytes());
        buf.push(1); // groups_enabled
        write_string(&mut buf, "voice.example.net");
        buf.push(1); // allow_recording
        let cfg = decode_secret(&buf).expect("decodes secret");
        assert_eq!(cfg.secret, secret);
        assert_eq!(cfg.server_port, 24454);
        assert_eq!(cfg.player_uuid, uuid);
        assert_eq!(cfg.mtu_size, 1024);
        assert_eq!(cfg.voice_chat_distance, 48.0);
        assert_eq!(cfg.keep_alive_ms, 1000);
        assert!(cfg.groups_enabled);
        assert_eq!(cfg.voice_host, "voice.example.net");
        assert!(cfg.allow_recording);
    }

    #[test]
    fn udp_seal_and_open_roundtrip() {
        let uuid = [0x42u8; 16];
        let secret = [0x7Fu8; 16];
        let inner = encode_mic(b"opus-bytes", 123, false);
        let pkt = seal_udp(&uuid, &secret, &inner).unwrap();
        assert_eq!(pkt[0], UDP_MAGIC);
        assert_eq!(&pkt[1..17], &uuid);
        let opened = open_udp(&pkt, &secret).expect("opens wrapped packet");
        assert_eq!(opened, inner);
        // Server->client format without magic/uuid prefix also opens
        let s2c = &pkt[17..];
        assert_eq!(open_udp(s2c, &secret).as_deref(), Some(inner.as_slice()));
    }
}
