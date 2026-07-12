use novatype_core::Engine;
use novatype_model::{CommitRecord, ModelError, UserModel};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

struct AppState {
    engine: Mutex<Engine>,
    model: UserModel,
    prev_commit: Mutex<Option<CommitRecord>>,
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
        })
    }
}

#[derive(Debug, Serialize)]
struct CandidateDto {
    text: String,
    reading: Vec<String>,
    score: f64,
}

fn do_suggest(state: &AppState, input: &str, limit: usize) -> Vec<CandidateDto> {
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
    std::env::var_os("NOVATYPE_DATA_DIR").map_or_else(|| PathBuf::from(".novatype"), PathBuf::from)
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
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![suggest, commit])
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
        AppState::new(&dir).expect("temp state")
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
