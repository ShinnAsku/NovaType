use novatype_core::{Candidate, Engine};
use novatype_model::{CommitRecord, UserModel};
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

fn main() {
    let data_dir = data_dir();
    if let Err(error) = std::fs::create_dir_all(&data_dir) {
        eprintln!("failed to create data dir {}: {error}", data_dir.display());
        return;
    }

    let model = match UserModel::open(&data_dir.join("user.redb")) {
        Ok(model) => model,
        Err(error) => {
            eprintln!("failed to open user model: {error}");
            return;
        }
    };

    let mut engine = Engine::new();
    for word in model.learned_words().unwrap_or_default() {
        engine.add_word(word.reading, word.text, word.frequency);
    }

    match env::args().nth(1) {
        Some(query) => {
            let candidates = suggest(&engine, &model, &query);
            print_candidates(&candidates, &query);
        }
        None => repl(&mut engine, &model),
    }
}

fn data_dir() -> PathBuf {
    env::var_os("NOVATYPE_DATA_DIR").map_or_else(|| PathBuf::from(".novatype"), PathBuf::from)
}

fn suggest(engine: &Engine, model: &UserModel, input: &str) -> Vec<Candidate> {
    let mut candidates = engine.suggest(input, 9);
    model.rerank(&mut candidates);
    candidates
}

fn repl(engine: &mut Engine, model: &UserModel) {
    println!("NovaType v0.2 CLI. Type pinyin, then a number to commit (learns). Empty line exits.");

    let mut last_candidates: Vec<Candidate> = Vec::new();
    let mut prev_commit: Option<CommitRecord> = None;

    loop {
        print!("> ");
        io::stdout().flush().expect("failed to flush stdout");

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            break;
        }

        if let Ok(index) = input.parse::<usize>() {
            if index >= 1 && index <= last_candidates.len() {
                let candidate = last_candidates[index - 1].clone();
                commit(engine, model, &mut prev_commit, &candidate);
                last_candidates.clear();
                continue;
            }
            println!("No candidate #{index}.");
            continue;
        }

        last_candidates = suggest(engine, model, input);
        print_candidates(&last_candidates, input);
    }
}

fn commit(
    engine: &mut Engine,
    model: &UserModel,
    prev_commit: &mut Option<CommitRecord>,
    candidate: &Candidate,
) {
    let record = CommitRecord {
        text: candidate.text.clone(),
        reading: candidate.reading.clone(),
    };

    match model.record_commit(prev_commit.as_ref(), &record) {
        Ok(Some(word)) => {
            println!("[learned new word: {}]", word.text);
            engine.add_word(word.reading, word.text, word.frequency);
        }
        Ok(None) => {}
        Err(error) => eprintln!("learning failed: {error}"),
    }

    println!("committed: {}", record.text);

    match model.predict_next(&record.text, 5) {
        Ok(predictions) if !predictions.is_empty() => {
            println!("next: {}", predictions.join(" | "));
        }
        Ok(_) => {}
        Err(error) => eprintln!("prediction failed: {error}"),
    }

    *prev_commit = Some(record);
}

fn print_candidates(candidates: &[Candidate], input: &str) {
    if candidates.is_empty() {
        println!("No candidates for `{input}`.");
        return;
    }

    for (index, candidate) in candidates.iter().enumerate() {
        println!(
            "{}. {}\t{}\t{:.2}",
            index + 1,
            candidate.text,
            candidate.reading.join(" "),
            candidate.score
        );
    }
}
