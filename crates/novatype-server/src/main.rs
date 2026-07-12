use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName, prelude::*};
use novatype_core::Engine;
use novatype_model::{CommitRecord, UserModel};
use novatype_protocol::{
    CandidateDto, Request, Response, default_data_dir, read_message, resolve_endpoint, tcp_addr,
    write_message,
};
use std::error::Error;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

struct Runtime {
    engine: Engine,
    model: UserModel,
    prev_commit: Option<CommitRecord>,
}

impl Runtime {
    fn open(data_dir: &Path) -> Result<Self, Box<dyn Error>> {
        std::fs::create_dir_all(data_dir)?;
        let model = UserModel::open(&data_dir.join("user.redb"))?;

        let mut engine = Engine::new();
        for word in model.learned_words().unwrap_or_default() {
            engine.add_word(word.reading, word.text, word.frequency);
        }

        Ok(Self {
            engine,
            model,
            prev_commit: None,
        })
    }

    fn handle(&mut self, request: Request) -> Response {
        match request {
            Request::Suggest { input, limit } => self.suggest(&input, limit),
            Request::Commit { text, reading } => self.commit(&text, reading),
            Request::Ping => Response::Pong,
            Request::Status => self.status(),
            Request::SetFuzzy(enabled) => {
                self.engine.set_fuzzy(enabled);
                Response::Ack
            }
            Request::LearnedWords => self.learned_words(),
        }
    }

    fn status(&self) -> Response {
        Response::Status(novatype_protocol::StatusDto {
            version: env!("CARGO_PKG_VERSION").to_string(),
            fuzzy: self.engine.fuzzy(),
            learned_words: self
                .model
                .learned_words()
                .map(|words| words.len())
                .unwrap_or(0),
        })
    }

    fn learned_words(&self) -> Response {
        match self.model.learned_words() {
            Ok(words) => Response::Words(
                words
                    .into_iter()
                    .map(|word| novatype_protocol::WordDto {
                        text: word.text,
                        reading: word.reading,
                        frequency: word.frequency,
                    })
                    .collect(),
            ),
            Err(error) => Response::Error(error.to_string()),
        }
    }

    fn suggest(&self, input: &str, limit: usize) -> Response {
        let mut candidates = self.engine.suggest(input, limit.clamp(1, 20));
        self.model.rerank(&mut candidates);
        Response::Candidates(
            candidates
                .into_iter()
                .map(|candidate| CandidateDto {
                    text: candidate.text,
                    reading: candidate.reading,
                    score: candidate.score,
                })
                .collect(),
        )
    }

    fn commit(&mut self, text: &str, reading: Vec<String>) -> Response {
        let record = CommitRecord {
            text: text.to_string(),
            reading,
        };

        match self.model.record_commit(self.prev_commit.as_ref(), &record) {
            Ok(Some(word)) => self
                .engine
                .add_word(word.reading, word.text, word.frequency),
            Ok(None) => {}
            Err(error) => return Response::Error(error.to_string()),
        }

        let predictions = match self.model.predict_next(&record.text, 5) {
            Ok(predictions) => predictions,
            Err(error) => return Response::Error(error.to_string()),
        };
        self.prev_commit = Some(record);
        Response::Predictions(predictions)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let endpoint = resolve_endpoint();
    let data_dir = default_data_dir();
    let mut runtime = Runtime::open(&data_dir)?;

    if let Some(addr) = tcp_addr(&endpoint) {
        return serve_tcp(&mut runtime, addr);
    }

    serve_local_socket(&mut runtime, &endpoint)
}

fn serve_tcp(runtime: &mut Runtime, addr: &str) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(addr)?;
    println!("novatyped listening on tcp://{addr}");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => handle_stream(runtime, &mut stream),
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }

    Ok(())
}

fn serve_local_socket(runtime: &mut Runtime, endpoint: &str) -> Result<(), Box<dyn Error>> {
    let name = endpoint.to_ns_name::<GenericNamespaced>()?;
    let listener = match ListenerOptions::new().name(name).create_sync() {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!("novatyped already running on `{endpoint}`; exiting");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };

    println!("novatyped listening on local socket `{endpoint}`");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => handle_stream(runtime, &mut stream),
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }

    Ok(())
}

fn handle_stream(runtime: &mut Runtime, stream: &mut (impl Read + Write)) {
    let response = match read_message::<Request>(stream) {
        Ok(request) => runtime.handle(request),
        Err(error) => Response::Error(error.to_string()),
    };

    if let Err(error) = write_message(stream, &response) {
        eprintln!("failed to write response: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::Runtime;
    use novatype_protocol::{Request, Response};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_runtime() -> Runtime {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "novatype-server-test-{}-{nanos}",
            std::process::id()
        ));
        Runtime::open(&dir).expect("runtime")
    }

    #[test]
    fn handles_suggest_request() {
        let mut runtime = temp_runtime();
        let response = runtime.handle(Request::Suggest {
            input: "nihao".to_string(),
            limit: 3,
        });

        let Response::Candidates(candidates) = response else {
            panic!("expected candidates");
        };
        assert_eq!(
            candidates.first().map(|candidate| candidate.text.as_str()),
            Some("你好")
        );
    }

    #[test]
    fn commit_request_creates_predictions() {
        let mut runtime = temp_runtime();
        runtime.handle(Request::Commit {
            text: "我们".to_string(),
            reading: vec!["wo".to_string(), "men".to_string()],
        });
        let response = runtime.handle(Request::Commit {
            text: "学习".to_string(),
            reading: vec!["xue".to_string(), "xi".to_string()],
        });

        assert!(matches!(response, Response::Predictions(_)));
        let predictions = runtime.model.predict_next("我们", 5).expect("predict");
        assert_eq!(predictions, vec!["学习".to_string()]);
    }
}
