//! Servers page backend: GERA's own server records, read-only import of
//! Horizon's `servers.dat`, server icons/banners and the Server List Ping.
//!
//! Horizon stores the multiplayer list as NBT in `<game dir>/servers.dat`
//! (uncompressed on Linux, gzip on Windows; both are accepted here). We only
//! ever read that file: every GERA record is stored in `state.json`, and
//! imported pictures are copied into `<data>/servers`.

use serde::Serialize;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::paths::Paths;
use crate::store::{now_ms, ServerEntry, Store};

pub const DEFAULT_PORT: u16 = 25565;
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;

pub fn servers_dir(paths: &Paths) -> PathBuf {
    paths.data.join("servers")
}

// ---------------------------------------------------------------- addresses

/// Split `host:port` the way servers.dat stores it. IPv6 without brackets has
/// no reliable port, so it is kept whole.
pub fn split_address(value: &str) -> (String, u16) {
    let text = value.trim();
    if let Some(rest) = text.strip_prefix('[') {
        if let Some((host, port)) = rest.split_once("]:") {
            if let Ok(port) = port.trim().parse::<u16>() {
                return (host.to_string(), port);
            }
        }
        return (
            text.trim_matches(|c| c == '[' || c == ']').to_string(),
            DEFAULT_PORT,
        );
    }
    if let Some((host, port)) = text.rsplit_once(':') {
        if !host.is_empty() && !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(port) = port.parse::<u16>() {
                return (host.to_string(), port);
            }
        }
    }
    (text.trim_end_matches(':').to_string(), DEFAULT_PORT)
}

/// `host:port`, always with the port, for `--server`.
pub fn join_address(host: &str, port: u16) -> String {
    let host = host.trim();
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

pub fn display_address(host: &str, port: u16) -> String {
    if port == DEFAULT_PORT {
        host.to_string()
    } else {
        format!("{host}:{port}")
    }
}

pub fn normalise_host(host: &str) -> String {
    host.trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase()
}

pub fn validate(name: &str, host: &str, port: u16) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("Название сервера: от 1 до 60 символов.".into());
    }
    let host = host.trim();
    if host.is_empty() || host.chars().count() > 253 {
        return Err("Адрес сервера: от 1 до 253 символов.".into());
    }
    if host
        .chars()
        .any(|c| c.is_whitespace() || c == '/' || c == '\\' || c == '"' || c == '\'')
    {
        return Err("Адрес не должен содержать пробелы и кавычки.".into());
    }
    if port == 0 {
        return Err("Порт должен быть от 1 до 65535.".into());
    }
    Ok(())
}

// ------------------------------------------------------------------- images

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerImages {
    pub banner: Option<String>,
    pub icon: Option<String>,
}

fn kind_prefix(kind: &str) -> Result<&'static str, String> {
    match kind {
        "banner" => Ok("banner"),
        "icon" => Ok("icon"),
        _ => Err("Неизвестный тип изображения.".into()),
    }
}

fn image_mime(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(("png", "image/png"))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("jpg", "image/jpeg"))
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("webp", "image/webp"))
    } else if bytes.starts_with(b"GIF8") {
        Some(("gif", "image/gif"))
    } else {
        None
    }
}

/// Save a `data:image/...;base64,…` string. Returns the file name stored in the
/// entry. Previous files of the same kind are removed.
pub fn save_image(paths: &Paths, id: &str, kind: &str, data_url: &str) -> Result<String, String> {
    let prefix = kind_prefix(kind)?;
    let payload = data_url
        .split_once("base64,")
        .map(|(_, rest)| rest)
        .unwrap_or(data_url);
    let bytes = b64_decode(payload.trim())?;
    if bytes.is_empty() {
        return Err("Пустое изображение.".into());
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("Изображение больше 4 МБ.".into());
    }
    let (ext, _) = image_mime(&bytes).ok_or("Поддерживаются PNG, JPEG, WebP и GIF.")?;
    let dir = servers_dir(paths);
    fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку серверов: {e}"))?;
    remove_image_files(&dir, id, prefix);
    let name = format!("{id}-{prefix}.{ext}");
    fs::write(dir.join(&name), &bytes).map_err(|e| format!("Не удалось сохранить изображение: {e}"))?;
    Ok(name)
}

fn remove_image_files(dir: &Path, id: &str, prefix: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!("{id}-{prefix}.")) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn clear_image(paths: &Paths, id: &str, kind: &str) {
    if let Ok(prefix) = kind_prefix(kind) {
        remove_image_files(&servers_dir(paths), id, prefix);
    }
}

pub fn read_image(paths: &Paths, file: &str) -> Option<String> {
    let path = servers_dir(paths).join(file);
    let bytes = fs::read(path).ok()?;
    let (_, mime) = image_mime(&bytes)?;
    Some(format!("data:{mime};base64,{}", b64_encode(&bytes)))
}

pub fn images_for(paths: &Paths, server: &ServerEntry) -> ServerImages {
    ServerImages {
        banner: server.banner.as_deref().and_then(|file| read_image(paths, file)),
        icon: server.icon.as_deref().and_then(|file| read_image(paths, file)),
    }
}

// ---------------------------------------------------------------- base64

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn b64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' | b' ' | b'\t' => continue,
            _ => return Err("Некорректные данные изображения.".into()),
        } as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

// --------------------------------------------------------------- NBT import

#[derive(Debug, Clone)]
struct RawServer {
    name: String,
    host: String,
    port: u16,
    icon: Option<String>,
}

#[derive(Debug, Clone)]
enum Tag {
    String(String),
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    Other,
}

struct NbtReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> NbtReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        if self.pos + count > self.data.len() {
            return Err("Файл серверов обрывается.".into());
        }
        let slice = &self.data[self.pos..self.pos + count];
        self.pos += count;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn string(&mut self) -> Result<String, String> {
        let len = self.i16()?;
        if len < 0 {
            return Err("Строка в файле серверов повреждена.".into());
        }
        let bytes = self.take(len as usize)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn payload(&mut self, tag: u8, depth: usize) -> Result<Tag, String> {
        if depth > 64 {
            return Err("Слишком глубокая структура в файле серверов.".into());
        }
        Ok(match tag {
            1 => {
                self.take(1)?;
                Tag::Other
            }
            2 => {
                self.take(2)?;
                Tag::Other
            }
            3 => {
                self.take(4)?;
                Tag::Other
            }
            4 => {
                self.take(8)?;
                Tag::Other
            }
            5 => {
                self.take(4)?;
                Tag::Other
            }
            6 => {
                self.take(8)?;
                Tag::Other
            }
            7 => {
                let len = self.i32()?;
                if len < 0 {
                    return Err("Массив в файле серверов повреждён.".into());
                }
                self.take(len as usize)?;
                Tag::Other
            }
            8 => Tag::String(self.string()?),
            9 => {
                let kind = self.u8()?;
                let len = self.i32()?;
                if len < 0 || len > 100_000 {
                    return Err("Список в файле серверов повреждён.".into());
                }
                let mut items = Vec::with_capacity(len.min(1024) as usize);
                for _ in 0..len {
                    items.push(self.payload(kind, depth + 1)?);
                }
                Tag::List(items)
            }
            10 => {
                let mut fields = Vec::new();
                loop {
                    let kind = self.u8()?;
                    if kind == 0 {
                        break;
                    }
                    let name = self.string()?;
                    let value = self.payload(kind, depth + 1)?;
                    fields.push((name, value));
                }
                Tag::Compound(fields)
            }
            11 => {
                let len = self.i32()?;
                if len < 0 {
                    return Err("Массив в файле серверов повреждён.".into());
                }
                self.take(len as usize * 4)?;
                Tag::Other
            }
            12 => {
                let len = self.i32()?;
                if len < 0 {
                    return Err("Массив в файле серверов повреждён.".into());
                }
                self.take(len as usize * 8)?;
                Tag::Other
            }
            other => return Err(format!("Неизвестный тип NBT {other} в файле серверов.")),
        })
    }
}

fn field<'a>(fields: &'a [(String, Tag)], name: &str) -> Option<&'a Tag> {
    fields.iter().find(|(key, _)| key == name).map(|(_, value)| value)
}

fn field_string(fields: &[(String, Tag)], name: &str) -> Option<String> {
    match field(fields, name) {
        Some(Tag::String(value)) => Some(value.clone()),
        _ => None,
    }
}

/// Read a Horizon / Minecraft server list. Handles plain and gzip NBT.
pub fn read_servers_dat(path: &Path) -> Result<Vec<RawServer>, String> {
    let raw = fs::read(path).map_err(|e| format!("Не удалось прочитать {}: {e}", path.display()))?;
    let bytes = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&raw[..])
            .read_to_end(&mut out)
            .map_err(|e| format!("Не удалось распаковать {}: {e}", path.display()))?;
        out
    } else {
        raw
    };

    let mut reader = NbtReader::new(&bytes);
    let root_type = reader.u8()?;
    if root_type != 10 {
        return Err("Файл серверов Horizon: ожидался NBT-объект.".into());
    }
    let _root_name = reader.string()?;
    let root = reader.payload(10, 0)?;
    let Tag::Compound(root_fields) = root else {
        return Err("Файл серверов Horizon повреждён.".into());
    };
    let Some(Tag::List(items)) = field(&root_fields, "servers") else {
        return Ok(Vec::new());
    };

    let mut servers = Vec::new();
    for item in items {
        let Tag::Compound(fields) = item else {
            continue;
        };
        let (host, port) = split_address(&field_string(fields, "ip").unwrap_or_default());
        let host = normalise_host(&host);
        if host.is_empty() {
            continue;
        }
        let name = clean_motd(&field_string(fields, "name").unwrap_or_default());
        let name = if name.is_empty() {
            display_address(&host, port)
        } else {
            name
        };
        let icon = field_string(fields, "icon")
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                if value.starts_with("data:") {
                    value
                } else {
                    format!("data:image/png;base64,{}", value.trim())
                }
            });
        servers.push(RawServer {
            name,
            host,
            port,
            icon,
        });
    }
    Ok(servers)
}

/// All `servers.dat` files GERA can see: every profile's game folder plus the
/// default Horizon folder. Saved worlds, mods and options are untouched.
pub fn server_dat_files(store: &Store) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = store
        .profiles
        .iter()
        .map(|profile| PathBuf::from(&profile.game_dir))
        .collect();
    dirs.push(crate::paths::horizon_default_game_dir());
    let mut seen = Vec::new();
    let mut files = Vec::new();
    for dir in dirs {
        let file = dir.join("servers.dat");
        if !file.is_file() {
            continue;
        }
        let canonical = fs::canonicalize(&file).unwrap_or_else(|_| file.clone());
        if seen.contains(&canonical) {
            continue;
        }
        seen.push(canonical);
        files.push(file);
    }
    files
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedServer {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub icon: Option<String>,
    pub already_added: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerImportScan {
    /// The servers.dat that was read, for the UI ("откуда").
    pub source: Option<String>,
    pub servers: Vec<ImportedServer>,
    /// Extra files that were read as well (several profiles).
    pub extra_sources: Vec<String>,
    pub error: Option<String>,
}

/// Read-only scan of Horizon's multiplayer list, marked against GERA's own
/// records. Nothing is written here.
pub fn scan_import(store: &Store) -> ServerImportScan {
    let files = server_dat_files(store);
    let mut result = ServerImportScan {
        source: None,
        servers: Vec::new(),
        extra_sources: Vec::new(),
        error: None,
    };
    if files.is_empty() {
        result.error = Some(
            "Файл серверов Horizon не найден. Добавьте сервер в мультиплеере Horizon или \
             создайте запись вручную."
                .into(),
        );
        return result;
    }

    let mut errors = Vec::new();
    let mut seen: Vec<(String, u16)> = Vec::new();
    for file in &files {
        match read_servers_dat(file) {
            Ok(list) => {
                if result.source.is_none() {
                    result.source = Some(file.to_string_lossy().into_owned());
                } else {
                    result
                        .extra_sources
                        .push(file.to_string_lossy().into_owned());
                }
                for server in list {
                    let key = (normalise_host(&server.host), server.port);
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.push(key.clone());
                    let already_added = store
                        .servers
                        .iter()
                        .any(|own| normalise_host(&own.host) == key.0 && own.port == key.1);
                    result.servers.push(ImportedServer {
                        name: server.name,
                        host: server.host,
                        port: server.port,
                        icon: server.icon,
                        already_added,
                    });
                }
            }
            Err(error) => errors.push(format!("{}: {error}", file.display())),
        }
    }
    if result.servers.is_empty() && result.source.is_none() {
        result.error = Some(errors.join(" "));
    } else if !errors.is_empty() {
        result.error = Some(errors.join(" "));
    }
    result
}

// ------------------------------------------------------------- server ping

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub online: bool,
    pub online_players: Option<u32>,
    pub max_players: Option<u32>,
    pub motd: Option<String>,
    pub version: Option<String>,
    pub latency_ms: Option<u32>,
    /// `data:image/png;base64,…` when the server offers a favicon.
    pub favicon: Option<String>,
    pub error: Option<String>,
}

fn offline(error: impl Into<String>) -> ServerStatus {
    ServerStatus {
        online: false,
        online_players: None,
        max_players: None,
        motd: None,
        version: None,
        latency_ms: None,
        favicon: None,
        error: Some(error.into()),
    }
}

fn write_varint(out: &mut Vec<u8>, mut value: i32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value = ((value as u32) >> 7) as i32;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn write_string(out: &mut Vec<u8>, text: &str) {
    write_varint(out, text.len() as i32);
    out.extend_from_slice(text.as_bytes());
}

fn read_varint<R: Read>(reader: &mut R) -> Result<i32, String> {
    let mut value: u32 = 0;
    let mut shift = 0;
    loop {
        let mut byte = [0u8; 1];
        reader
            .read_exact(&mut byte)
            .map_err(|_| "Сервер не ответил.".to_string())?;
        value |= ((byte[0] & 0x7f) as u32) << shift;
        if byte[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 35 {
            return Err("Некорректный ответ сервера.".into());
        }
    }
    Ok(value as i32)
}

/// Minecraft Server List Ping (1.7+). Never blocks the UI thread: call it from
/// `spawn_blocking`.
pub fn ping(host: &str, port: u16) -> ServerStatus {
    let host = host.trim().trim_matches(|c| c == '[' || c == ']');
    if host.is_empty() || port == 0 {
        return offline("Адрес сервера не задан.");
    }
    let started = Instant::now();
    let address = (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next());
    let Some(address) = address else {
        return offline(format!("Не удалось найти адрес {host}."));
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_secs(3)) else {
        return offline("Сервер не отвечает.");
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(4)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(4)));

    // Handshake: protocol 769 is Minecraft 26.3; servers answer with their own.
    let mut handshake = Vec::new();
    write_varint(&mut handshake, 0);
    write_varint(&mut handshake, 769);
    write_string(&mut handshake, host);
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake, 1);
    let mut frame = Vec::new();
    write_varint(&mut frame, handshake.len() as i32);
    frame.extend_from_slice(&handshake);
    // Status request.
    frame.extend_from_slice(&[1, 0]);
    if stream.write_all(&frame).is_err() {
        return offline("Сервер закрыл соединение.");
    }

    let mut reader = BufReader::new(&stream);
    let length = match read_varint(&mut reader) {
        Ok(value) if value > 0 && value < 4 * 1024 * 1024 => value as usize,
        _ => return offline("Сервер не ответил на запрос статуса."),
    };
    let mut payload = vec![0u8; length];
    if reader.read_exact(&mut payload).is_err() {
        return offline("Ответ сервера оборвался.");
    }
    let mut cursor = std::io::Cursor::new(&payload[..]);
    if read_varint(&mut cursor).is_err() {
        return offline("Некорректный ответ сервера.");
    }
    let text_len = match read_varint(&mut cursor) {
        Ok(value) if value >= 0 && value as usize <= payload.len() => value as usize,
        _ => return offline("Некорректный ответ сервера."),
    };
    let start = cursor.position() as usize;
    let json_text = String::from_utf8_lossy(&payload[start..start + text_len]).into_owned();
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_text) else {
        return offline("Сервер вернул нечитаемый статус.");
    };

    let players = json.get("players");
    let online_players = players
        .and_then(|p| p.get("online"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let max_players = players
        .and_then(|p| p.get("max"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let version = json
        .get("version")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .map(|v| clean_motd(v))
        .filter(|v| !v.is_empty());
    let mut motd = String::new();
    if let Some(description) = json.get("description") {
        flatten_text(description, &mut motd, 0);
    }
    let motd = clean_motd(&motd);
    let favicon = json
        .get("favicon")
        .and_then(|v| v.as_str())
        .filter(|v| v.starts_with("data:image/"))
        .map(|v| v.to_string());

    ServerStatus {
        online: true,
        online_players,
        max_players,
        motd: (!motd.is_empty()).then_some(motd),
        version,
        latency_ms: Some(started.elapsed().as_millis().min(60000) as u32),
        favicon,
        error: None,
    }
}

fn flatten_text(value: &serde_json::Value, out: &mut String, depth: usize) {
    if depth > 16 {
        return;
    }
    match value {
        serde_json::Value::String(text) => out.push_str(text),
        serde_json::Value::Array(items) => {
            for item in items {
                flatten_text(item, out, depth + 1);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                out.push_str(text);
            }
            if let Some(extra) = map.get("extra") {
                flatten_text(extra, out, depth + 1);
            }
        }
        _ => {}
    }
}

/// Drop Minecraft formatting codes (§a, §l, …) and squeeze the newlines the
/// list shows as separate lines.
pub fn clean_motd(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
            continue;
        }
        out.push(c);
    }
    out.trim().to_string()
}

// ------------------------------------------------------------- new records

pub fn next_id(store: &Store) -> String {
    let mut stamp = now_ms();
    loop {
        let id = format!("server-{stamp}");
        if store.servers.iter().all(|s| s.id != id) {
            return id;
        }
        stamp += 1;
    }
}

pub fn next_position(store: &Store) -> u32 {
    store
        .servers
        .iter()
        .map(|s| s.position)
        .max()
        .unwrap_or(0)
        + 1
}

pub fn fallback_letters(name: &str) -> String {
    let letters: Vec<char> = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .collect();
    letters.into_iter().collect::<String>().to_uppercase()
}

/// Accept any #rrggbb colour; fall back to the seed palette colour.
pub fn clean_color(value: &str) -> String {
    let text = value.trim();
    if crate::store::is_hex_color(text) {
        text.to_ascii_lowercase()
    } else {
        "#6750a4".into()
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    fn nbt_string(out: &mut Vec<u8>, text: &str) {
        out.extend_from_slice(&(text.len() as u16).to_be_bytes());
        out.extend_from_slice(text.as_bytes());
    }

    fn servers_dat(names: &[(&str, &str)]) -> Vec<u8> {
        let mut out = vec![0x0a, 0x00, 0x00];
        out.push(0x09);
        nbt_string(&mut out, "servers");
        out.push(0x0a);
        out.extend_from_slice(&(names.len() as i32).to_be_bytes());
        for (name, ip) in names {
            out.push(0x08);
            nbt_string(&mut out, "name");
            nbt_string(&mut out, name);
            out.push(0x08);
            nbt_string(&mut out, "ip");
            nbt_string(&mut out, ip);
            out.push(0x00);
        }
        out.push(0x00);
        out
    }

    #[test]
    fn splits_addresses() {
        assert_eq!(split_address("play.example.com"), ("play.example.com".into(), 25565));
        assert_eq!(split_address("play.example.com:25566"), ("play.example.com".into(), 25566));
        assert_eq!(split_address("[::1]:25567"), ("::1".into(), 25567));
        assert_eq!(split_address("example.com:"), ("example.com".into(), 25565));
        assert_eq!(join_address("example.com", 25565), "example.com:25565");
        assert_eq!(join_address("::1", 25565), "[::1]:25565");
    }

    #[test]
    fn base64_roundtrip() {
        let bytes = vec![0u8, 1, 2, 250, 251, 252, 253, 254, 255];
        let encoded = b64_encode(&bytes);
        assert_eq!(encoded, "AAEC+vv8/f7/");
        assert_eq!(b64_decode(&encoded).unwrap(), bytes);
    }

    #[test]
    fn strips_formatting_codes() {
        assert_eq!(clean_motd("§aHello §lWorld§r\n"), "Hello World");
        assert_eq!(fallback_letters("Anarchy Server"), "AN");
    }

    #[test]
    fn reads_plain_nbt() {
        let dir = std::env::temp_dir().join(format!("gera-servers-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("servers.dat");
        fs::write(
            &file,
            servers_dat(&[("Anarchy", "play.example.com:25566"), ("SkyBlock", "[::1]")]),
        )
        .unwrap();
        let list = read_servers_dat(&file).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Anarchy");
        assert_eq!(list[0].host, "play.example.com");
        assert_eq!(list[0].port, 25566);
        assert_eq!(list[1].port, DEFAULT_PORT);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stores_a_server_and_its_banner() {
        let root = std::env::temp_dir().join(format!("gera-store-servers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = Paths {
            data: root.clone(),
            logs: root.join("logs"),
            profiles: root.join("profiles"),
            cache: root.join("cache"),
        };
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        let url = format!("data:image/png;base64,{}", b64_encode(&png));
        let file = save_image(&paths, "server-x", "banner", &url).unwrap();
        assert!(file.starts_with("server-x-banner."));

        let mut store = Store::default();
        store.servers.push(ServerEntry {
            id: "server-x".into(),
            name: "Anarchy".into(),
            host: "play.example.com".into(),
            port: 25566,
            description: "test".into(),
            banner: Some(file),
            icon: None,
            fallback_text: "AN".into(),
            fallback_color: "#6750a4".into(),
            profile_id: None,
            pinned: true,
            position: 1,
            created_at: 1,
            last_played: None,
        });
        crate::store::save(&paths, &store).unwrap();
        let loaded = crate::store::load_or_init(&paths).unwrap();
        assert_eq!(loaded.servers.len(), 1);
        assert_eq!(loaded.servers[0].host, "play.example.com");
        assert!(loaded.servers[0].pinned);

        let images = images_for(&paths, &loaded.servers[0]);
        assert!(images.banner.unwrap_or_default().starts_with("data:image/png;base64,"));
        clear_image(&paths, "server-x", "banner");
        assert!(images_for(&paths, &loaded.servers[0]).banner.is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reads_the_real_horizon_file_when_present() {
        let path = crate::paths::horizon_default_game_dir().join("servers.dat");
        if !path.is_file() {
            return;
        }
        let list = read_servers_dat(&path).unwrap();
        assert!(!list.is_empty(), "Horizon file parsed but empty");
        assert!(list.iter().all(|server| !server.host.is_empty()));
    }

    #[test]
    fn ping_reads_a_status_response() {
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let json = r#"{"version":{"name":"Horizon 26.3"},"players":{"online":3,"max":20},"description":{"text":"Test server"},"favicon":"data:image/png;base64,AAAA"}"#;
            let mut payload = Vec::new();
            write_varint(&mut payload, 0);
            write_string(&mut payload, json);
            let mut frame = Vec::new();
            write_varint(&mut frame, payload.len() as i32);
            frame.extend_from_slice(&payload);
            let _ = stream.write_all(&frame);
        });

        let status = ping("127.0.0.1", port);
        assert!(status.online, "offline: {:?}", status.error);
        assert_eq!(status.online_players, Some(3));
        assert_eq!(status.max_players, Some(20));
        assert_eq!(status.motd.as_deref(), Some("Test server"));
        assert_eq!(status.version.as_deref(), Some("Horizon 26.3"));
        assert!(status.favicon.is_some());
    }
}
