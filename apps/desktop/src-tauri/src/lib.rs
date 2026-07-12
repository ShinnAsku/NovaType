use novatype_core::Engine;
use novatype_model::{CommitRecord, ModelError, UserModel};
use novatype_protocol::{
    CandidateDto, Request, Response, StatusDto, WordDto, default_data_dir, resolve_endpoint,
    send_request,
};
use std::path::PathBuf;
use std::sync::Mutex;

struct AppState {
    engine: Mutex<Engine>,
    model: UserModel,
    prev_commit: Mutex<Option<CommitRecord>>,
    server_addr: String,
}

impl AppState {
    fn new(data_dir: &PathBuf) -> Result<Self, ModelError> {
        std::fs::create_dir_all(data_dir)?;
        let model = UserModel::open(&data_dir.join("user.redb"))?;

        let mut engine = Engine::new();
        for word in model.learned_words().unwrap_or_default() {
            engine.add_word(word.reading, word.text, word.frequency);
        }

        Ok(Self {
            engine: Mutex::new(engine),
            model,
            prev_commit: Mutex::new(None),
            server_addr: resolve_endpoint(),
        })
    }
}

/// Best-effort: make sure a daemon is reachable, spawning a sibling
/// `novatype-server` binary when it is not. Fallback to the in-process
/// engine keeps working when neither succeeds.
fn ensure_daemon(endpoint: &str) {
    if send_request(endpoint, &Request::Ping).is_ok() {
        return;
    }

    let Some(server) = sibling_server_path() else {
        return;
    };
    if let Err(error) = std::process::Command::new(&server).spawn() {
        eprintln!("failed to spawn {}: {error}", server.display());
    }
}

fn sibling_server_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) {
        "novatype-server.exe"
    } else {
        "novatype-server"
    };
    let path = dir.join(name);
    path.exists().then_some(path)
}

fn do_suggest(state: &AppState, input: &str, limit: usize) -> Vec<CandidateDto> {
    if let Ok(Response::Candidates(candidates)) = send_request(
        &state.server_addr,
        &Request::Suggest {
            input: input.to_string(),
            limit,
        },
    ) {
        return candidates;
    }

    let limit = limit.clamp(1, 20);
    let mut candidates = state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .suggest(input, limit);
    state.model.rerank(&mut candidates);

    candidates
        .into_iter()
        .map(|candidate| CandidateDto {
            text: candidate.text,
            reading: candidate.reading,
            score: candidate.score,
        })
        .collect()
}

fn do_commit(
    state: &AppState,
    text: &str,
    reading: Vec<String>,
) -> Result<Vec<String>, ModelError> {
    if let Ok(Response::Predictions(predictions)) = send_request(
        &state.server_addr,
        &Request::Commit {
            text: text.to_string(),
            reading: reading.clone(),
        },
    ) {
        return Ok(predictions);
    }

    let record = CommitRecord {
        text: text.to_string(),
        reading,
    };

    let prev = state
        .prev_commit
        .lock()
        .expect("commit lock poisoned")
        .clone();

    if let Some(word) = state.model.record_commit(prev.as_ref(), &record)? {
        state.engine.lock().expect("engine lock poisoned").add_word(
            word.reading,
            word.text,
            word.frequency,
        );
    }

    let predictions = state.model.predict_next(&record.text, 5)?;
    *state.prev_commit.lock().expect("commit lock poisoned") = Some(record);
    Ok(predictions)
}

fn do_status(state: &AppState) -> StatusDto {
    if let Ok(Response::Status(status)) = send_request(&state.server_addr, &Request::Status) {
        return status;
    }

    StatusDto {
        version: format!("{} (local)", env!("CARGO_PKG_VERSION")),
        fuzzy: state.engine.lock().expect("engine lock poisoned").fuzzy(),
        learned_words: state
            .model
            .learned_words()
            .map(|words| words.len())
            .unwrap_or(0),
    }
}

fn do_set_fuzzy(state: &AppState, enabled: bool) {
    let _ = send_request(&state.server_addr, &Request::SetFuzzy(enabled));
    state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .set_fuzzy(enabled);
}

fn do_learned_words(state: &AppState) -> Vec<WordDto> {
    if let Ok(Response::Words(words)) = send_request(&state.server_addr, &Request::LearnedWords) {
        return words;
    }

    state
        .model
        .learned_words()
        .unwrap_or_default()
        .into_iter()
        .map(|word| WordDto {
            text: word.text,
            reading: word.reading,
            frequency: word.frequency,
        })
        .collect()
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn status(state: tauri::State<'_, AppState>) -> StatusDto {
    do_status(&state)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn set_fuzzy(state: tauri::State<'_, AppState>, enabled: bool) {
    do_set_fuzzy(&state, enabled);
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn learned_words(state: tauri::State<'_, AppState>) -> Vec<WordDto> {
    do_learned_words(&state)
}

/// Runs an agent command (`//翻译 ...`) against the local Ollama backend.
/// Runs on a blocking thread so the UI stays responsive; errors degrade to a
/// user-visible message and never affect normal candidates.
#[tauri::command]
async fn agent_run(input: String, model: Option<String>) -> Result<String, String> {
    let Some(command) = novatype_agent::parse(&input) else {
        return Err("不是有效的指令（示例：//翻译 hello）".to_string());
    };

    tauri::async_runtime::spawn_blocking(move || {
        let backend =
            novatype_llm::OllamaBackend::local(model.unwrap_or_else(|| "qwen2:0.5b".to_string()));
        novatype_agent::execute(&command, &backend).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn suggest(state: tauri::State<'_, AppState>, input: String, limit: usize) -> Vec<CandidateDto> {
    do_suggest(&state, &input, limit)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn commit(
    state: tauri::State<'_, AppState>,
    text: String,
    reading: Vec<String>,
) -> Result<Vec<String>, String> {
    do_commit(&state, &text, reading).map_err(|error| error.to_string())
}

fn data_dir() -> PathBuf {
    default_data_dir()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the `NovaType` desktop application.
///
/// # Errors
///
/// Returns an error if the user model cannot be opened or if Tauri cannot
/// create or run the application runtime.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState::new(&data_dir())?;
    ensure_daemon(&state.server_addr);
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            suggest,
            commit,
            status,
            set_fuzzy,
            learned_words,
            agent_run
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AppState, do_commit, do_suggest};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_state() -> AppState {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "novatype-desktop-test-{}-{nanos}",
            std::process::id()
        ));
        let mut state = AppState::new(&dir).expect("temp state");
        state.server_addr = "127.0.0.1:9".to_string();
        state
    }

    #[test]
    fn suggest_returns_core_candidates() {
        let state = temp_state();
        let candidates = do_suggest(&state, "nihao", 3);

        assert_eq!(
            candidates.first().map(|candidate| candidate.text.as_str()),
            Some("你好")
        );
    }

    #[test]
    fn suggest_falls_back_when_daemon_is_unavailable() {
        let mut state = temp_state();
        state.server_addr = "127.0.0.1:9".to_string();

        let candidates = do_suggest(&state, "shurufa", 3);

        assert_eq!(
            candidates.first().map(|candidate| candidate.text.as_str()),
            Some("输入法")
        );
    }

    #[test]
    fn clamps_candidate_limit() {
        let state = temp_state();
        let candidates = do_suggest(&state, "nihao", 0);

        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn commit_learns_and_predicts() {
        let state = temp_state();

        do_commit(&state, "我们", vec!["wo".into(), "men".into()]).expect("commit");
        do_commit(&state, "学习", vec!["xue".into(), "xi".into()]).expect("commit");

        let predictions = state.model.predict_next("我们", 5).expect("predict");
        assert_eq!(predictions, vec!["学习".to_string()]);
    }
}
