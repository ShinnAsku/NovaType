//! User habit model: decayed frequency learning, next-word prediction, and
//! automatic word creation, persisted in a single-file `redb` database.

use novatype_core::Candidate;
use redb::{Database, ReadableTable, TableDefinition};
use std::fmt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const UNIGRAMS: TableDefinition<&str, &[u8]> = TableDefinition::new("unigrams");
const BIGRAMS: TableDefinition<&str, &[u8]> = TableDefinition::new("bigrams");
const WORDS: TableDefinition<&str, &str> = TableDefinition::new("words");

const SEP: char = '\u{1F}';
const FIELD_SEP: char = '\u{1E}';
const HALF_LIFE_SECONDS: f64 = 2_592_000.0; // 30 days
const BOOST_WEIGHT: f64 = 0.8;
const AUTO_WORD_THRESHOLD: f64 = 3.0;
const AUTO_WORD_MAX_CHARS: usize = 6;
const LEARNED_WORD_FREQUENCY: u32 = 6_000;

#[derive(Debug)]
pub struct ModelError(String);

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "user model error: {}", self.0)
    }
}

impl std::error::Error for ModelError {}

impl From<std::io::Error> for ModelError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

fn db_err(error: impl Into<redb::Error>) -> ModelError {
    ModelError(error.into().to_string())
}

pub type ModelResult<T> = Result<T, ModelError>;

/// A committed word together with the pinyin reading it was typed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRecord {
    pub text: String,
    pub reading: Vec<String>,
}

/// A word the model created automatically from repeated adjacent commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedWord {
    pub text: String,
    pub reading: Vec<String>,
    pub frequency: u32,
}

pub struct UserModel {
    db: Database,
}

impl UserModel {
    /// Opens (or creates) the user model database at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the database file cannot be created or opened.
    pub fn open(path: &Path) -> ModelResult<Self> {
        let db = Database::create(path).map_err(db_err)?;
        Ok(Self { db })
    }

    /// Records a committed word, updating unigram and bigram statistics.
    ///
    /// Returns a [`LearnedWord`] when the adjacent pair crossed the
    /// auto-word threshold for the first time.
    ///
    /// # Errors
    ///
    /// Returns an error if the database transaction fails.
    pub fn record_commit(
        &self,
        prev: Option<&CommitRecord>,
        current: &CommitRecord,
    ) -> ModelResult<Option<LearnedWord>> {
        let now = unix_now();
        let txn = self.db.begin_write().map_err(db_err)?;
        let mut learned = None;

        {
            let mut unigrams = txn.open_table(UNIGRAMS).map_err(db_err)?;
            let previous = unigrams
                .get(current.text.as_str())
                .map_err(db_err)?
                .map(|guard| decode_pair(guard.value()));
            let updated = bump(previous, now);
            unigrams
                .insert(current.text.as_str(), encode_pair(updated).as_slice())
                .map_err(db_err)?;
        }

        if let Some(prev) = prev {
            let key = format!("{}{SEP}{}", prev.text, current.text);
            let updated = {
                let mut bigrams = txn.open_table(BIGRAMS).map_err(db_err)?;
                let previous = bigrams
                    .get(key.as_str())
                    .map_err(db_err)?
                    .map(|guard| decode_pair(guard.value()));
                let updated = bump(previous, now);
                bigrams
                    .insert(key.as_str(), encode_pair(updated).as_slice())
                    .map_err(db_err)?;
                updated
            };

            if updated.0 >= AUTO_WORD_THRESHOLD {
                learned = maybe_create_word(&txn, prev, current)?;
            }
        }

        txn.commit().map_err(db_err)?;
        Ok(learned)
    }

    /// Returns the user-history score boost for `text`.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be read.
    pub fn boost(&self, text: &str) -> ModelResult<f64> {
        let txn = self.db.begin_read().map_err(db_err)?;
        let table = match txn.open_table(UNIGRAMS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0.0),
            Err(error) => return Err(db_err(error)),
        };

        let now = unix_now();
        let effective = table
            .get(text)
            .map_err(db_err)?
            .map(|guard| decode_pair(guard.value()))
            .map_or(0.0, |(count, last)| decayed(count, last, now));
        Ok((1.0 + effective).ln() * BOOST_WEIGHT)
    }

    /// Applies user-history boosts to candidates and re-sorts them.
    pub fn rerank(&self, candidates: &mut [Candidate]) {
        for candidate in candidates.iter_mut() {
            candidate.score += self.boost(&candidate.text).unwrap_or_default();
        }
        candidates.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Predicts the most likely next words after `prev`.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be read.
    pub fn predict_next(&self, prev: &str, limit: usize) -> ModelResult<Vec<String>> {
        let txn = self.db.begin_read().map_err(db_err)?;
        let table = match txn.open_table(BIGRAMS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(db_err(error)),
        };

        let start = format!("{prev}{SEP}");
        let end = format!("{prev}{SEP}\u{10FFFF}");
        let now = unix_now();
        let mut scored = Vec::new();

        for item in table.range(start.as_str()..end.as_str()).map_err(db_err)? {
            let (key, value) = item.map_err(db_err)?;
            let Some(next) = key.value().split(SEP).nth(1) else {
                continue;
            };
            let (count, last) = decode_pair(value.value());
            scored.push((next.to_string(), decayed(count, last, now)));
        }

        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);
        Ok(scored.into_iter().map(|(text, _)| text).collect())
    }

    /// Returns all automatically learned words.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be read.
    pub fn learned_words(&self) -> ModelResult<Vec<LearnedWord>> {
        let txn = self.db.begin_read().map_err(db_err)?;
        let table = match txn.open_table(WORDS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(db_err(error)),
        };

        let mut words = Vec::new();
        for item in table.iter().map_err(db_err)? {
            let (key, value) = item.map_err(db_err)?;
            if let Some(word) = parse_word(key.value(), value.value()) {
                words.push(word);
            }
        }
        Ok(words)
    }
}

fn maybe_create_word(
    txn: &redb::WriteTransaction,
    prev: &CommitRecord,
    current: &CommitRecord,
) -> ModelResult<Option<LearnedWord>> {
    if prev.reading.is_empty() || current.reading.is_empty() {
        return Ok(None);
    }

    let text = format!("{}{}", prev.text, current.text);
    if text.chars().count() > AUTO_WORD_MAX_CHARS {
        return Ok(None);
    }

    let mut words = txn.open_table(WORDS).map_err(db_err)?;
    if words.get(text.as_str()).map_err(db_err)?.is_some() {
        return Ok(None);
    }

    let reading: Vec<String> = prev
        .reading
        .iter()
        .chain(current.reading.iter())
        .cloned()
        .collect();
    let encoded = format!(
        "{}{FIELD_SEP}{}",
        reading.join(&SEP.to_string()),
        LEARNED_WORD_FREQUENCY
    );
    words
        .insert(text.as_str(), encoded.as_str())
        .map_err(db_err)?;

    Ok(Some(LearnedWord {
        text,
        reading,
        frequency: LEARNED_WORD_FREQUENCY,
    }))
}

fn parse_word(text: &str, encoded: &str) -> Option<LearnedWord> {
    let (reading, frequency) = encoded.split_once(FIELD_SEP)?;
    Some(LearnedWord {
        text: text.to_string(),
        reading: reading.split(SEP).map(str::to_string).collect(),
        frequency: frequency.parse().ok()?,
    })
}

fn bump(previous: Option<(f64, u64)>, now: u64) -> (f64, u64) {
    let effective = previous.map_or(0.0, |(count, last)| decayed(count, last, now));
    (effective + 1.0, now)
}

#[allow(clippy::cast_precision_loss)]
fn decayed(count: f64, last: u64, now: u64) -> f64 {
    let delta = now.saturating_sub(last) as f64;
    count * 0.5_f64.powf(delta / HALF_LIFE_SECONDS)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn encode_pair((count, last): (f64, u64)) -> [u8; 16] {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&count.to_le_bytes());
    bytes[8..].copy_from_slice(&last.to_le_bytes());
    bytes
}

fn decode_pair(bytes: &[u8]) -> (f64, u64) {
    if bytes.len() != 16 {
        return (0.0, 0);
    }
    let count = f64::from_le_bytes(bytes[..8].try_into().unwrap_or_default());
    let last = u64::from_le_bytes(bytes[8..].try_into().unwrap_or_default());
    (count, last)
}

#[cfg(test)]
mod tests {
    use super::{CommitRecord, UserModel};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_model() -> UserModel {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "novatype-model-test-{}-{nanos}.redb",
            std::process::id()
        ));
        UserModel::open(&path).expect("temp model")
    }

    fn record(text: &str, reading: &[&str]) -> CommitRecord {
        CommitRecord {
            text: text.to_string(),
            reading: reading.iter().map(|item| (*item).to_string()).collect(),
        }
    }

    #[test]
    fn boost_grows_with_commits() {
        let model = temp_model();
        let word = record("你好", &["ni", "hao"]);

        let before = model.boost("你好").expect("boost");
        model.record_commit(None, &word).expect("commit");
        let after = model.boost("你好").expect("boost");

        assert!(after > before);
    }

    #[test]
    fn predicts_next_word_from_bigrams() {
        let model = temp_model();
        let first = record("我们", &["wo", "men"]);
        let second = record("学习", &["xue", "xi"]);

        model.record_commit(None, &first).expect("commit");
        model.record_commit(Some(&first), &second).expect("commit");

        let predictions = model.predict_next("我们", 5).expect("predict");
        assert_eq!(predictions, vec!["学习".to_string()]);
    }

    #[test]
    fn creates_word_after_repeated_pairs() {
        let model = temp_model();
        let first = record("智能", &["zhi", "neng"]);
        let second = record("输入", &["shu", "ru"]);

        let mut learned = None;
        for _ in 0..3 {
            model.record_commit(None, &first).expect("commit");
            learned = model.record_commit(Some(&first), &second).expect("commit");
        }

        let word = learned.expect("word should be created on third pair");
        assert_eq!(word.text, "智能输入");
        assert_eq!(word.reading, vec!["zhi", "neng", "shu", "ru"]);

        let words = model.learned_words().expect("learned words");
        assert_eq!(words.len(), 1);

        let again = model.record_commit(Some(&first), &second).expect("commit");
        assert!(again.is_none(), "word must only be created once");
    }
}
