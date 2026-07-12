use interprocess::local_socket::{GenericNamespaced, Stream, ToNsName, prelude::*};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};
use std::net::TcpStream;

/// Default local socket endpoint for the daemon transport.
pub const DEFAULT_ENDPOINT: &str = "novatype";

/// Legacy loopback address for explicit TCP development mode.
pub const DEFAULT_ADDR: &str = "127.0.0.1:48931";

const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Candidate payload shared by daemon clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateDto {
    pub text: String,
    pub reading: Vec<String>,
    pub score: f64,
}

/// Requests accepted by `novatyped`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    Suggest { input: String, limit: usize },
    Commit { text: String, reading: Vec<String> },
    Ping,
    Status,
    SetFuzzy(bool),
    LearnedWords,
}

/// A learned word payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordDto {
    pub text: String,
    pub reading: Vec<String>,
    pub frequency: u32,
}

/// Daemon status payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusDto {
    pub version: String,
    pub fuzzy: bool,
    pub learned_words: usize,
}

/// Responses returned by `novatyped`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    Candidates(Vec<CandidateDto>),
    Predictions(Vec<String>),
    Pong,
    Status(StatusDto),
    Words(Vec<WordDto>),
    Ack,
    Error(String),
}

/// Writes a length-prefixed bincode message.
///
/// # Errors
///
/// Returns an error when serialization fails, the message is too large, or the
/// writer fails.
pub fn write_message<T: Serialize>(writer: &mut impl Write, value: &T) -> io::Result<()> {
    let payload = bincode::serialize(value).map_err(invalid_data)?;
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message exceeds size limit",
        ));
    }

    let len = u32::try_from(payload.len()).map_err(invalid_data)?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)
}

/// Reads a length-prefixed bincode message.
///
/// # Errors
///
/// Returns an error when the input is incomplete, too large, or cannot be
/// deserialized.
pub fn read_message<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut len_bytes = [0_u8; 4];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message exceeds size limit",
        ));
    }

    let mut payload = vec![0_u8; len];
    reader.read_exact(&mut payload)?;
    bincode::deserialize(&payload).map_err(invalid_data)
}

/// Sends one request to a running daemon and reads one response.
///
/// # Errors
///
/// Returns an error when the daemon cannot be reached or the protocol exchange
/// fails.
pub fn send_request(endpoint: &str, request: &Request) -> io::Result<Response> {
    if let Some(addr) = tcp_addr(endpoint) {
        let mut stream = TcpStream::connect(addr)?;
        write_message(&mut stream, request)?;
        return read_message(&mut stream);
    }

    let name = endpoint
        .to_ns_name::<GenericNamespaced>()
        .map_err(invalid_data)?;
    let mut stream = Stream::connect(name)?;
    write_message(&mut stream, request)?;
    read_message(&mut stream)
}

/// Returns the TCP address when `endpoint` is an explicit TCP endpoint.
#[must_use]
pub fn tcp_addr(endpoint: &str) -> Option<&str> {
    endpoint
        .strip_prefix("tcp://")
        .or_else(|| endpoint.contains(':').then_some(endpoint))
}

/// Resolves the daemon endpoint from environment or default.
#[must_use]
pub fn resolve_endpoint() -> String {
    std::env::var("NOVATYPE_ENDPOINT")
        .or_else(|_| std::env::var("NOVATYPE_SERVER_ADDR"))
        .unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string())
}

/// Resolves the user data directory.
///
/// Priority: `NOVATYPE_DATA_DIR` env var, then the platform data directory
/// (e.g. `%APPDATA%` on Windows, `~/.local/share` on Linux, and
/// `~/Library/Application Support` on macOS), then `.novatype` as fallback.
#[must_use]
pub fn default_data_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("NOVATYPE_DATA_DIR") {
        return std::path::PathBuf::from(dir);
    }
    dirs::data_dir().map_or_else(
        || std::path::PathBuf::from(".novatype"),
        |base| base.join("NovaType"),
    )
}

fn invalid_data(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::{CandidateDto, Request, read_message, write_message};
    use std::io::Cursor;

    #[test]
    fn round_trips_request() {
        let request = Request::Suggest {
            input: "nihao".to_string(),
            limit: 5,
        };
        let mut buffer = Vec::new();

        write_message(&mut buffer, &request).expect("write");
        let decoded: Request = read_message(&mut Cursor::new(buffer)).expect("read");

        assert_eq!(decoded, request);
    }

    #[test]
    fn round_trips_candidate() {
        let candidate = CandidateDto {
            text: "你好".to_string(),
            reading: vec!["ni".to_string(), "hao".to_string()],
            score: 1.0,
        };
        let mut buffer = Vec::new();

        write_message(&mut buffer, &candidate).expect("write");
        let decoded: CandidateDto = read_message(&mut Cursor::new(buffer)).expect("read");

        assert_eq!(decoded, candidate);
    }
}
