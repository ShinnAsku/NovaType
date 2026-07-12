use novatype_core::{Candidate, Engine};
use novatype_model::{CommitRecord, UserModel};
use novatype_protocol::{
    CandidateDto, Request, Response, default_data_dir, resolve_endpoint, send_request,
};
use std::env;
use std::io::{self, Write};

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "--server") {
        query_server(args.get(1).map(String::as_str));
        return;
    }

    let data_dir = default_data_dir();
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

    match args.first() {
        Some(query) => {
            let candidates = suggest(&engine, &model, query);
            print_candidates(&candidates, query);
        }
        None => repl(&mut engine, &model),
    }
}

fn query_server(input: Option<&str>) {
    let Some(input) = input else {
        eprintln!("usage: novatype-cli --server <pinyin>");
        return;
    };
    let endpoint = resolve_endpoint();
    match send_request(
        &endpoint,
        &Request::Suggest {
            input: input.to_string(),
            limit: 9,
        },
    ) {
        Ok(Response::Candidates(candidates)) => print_candidate_dtos(&candidates, input),
        Ok(Response::Error(error)) => eprintln!("server error: {error}"),
        Ok(other) => eprintln!("unexpected server response: {other:?}"),
        Err(error) => eprintln!("failed to query server at {endpoint}: {error}"),
    }
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

fn print_candidate_dtos(candidates: &[CandidateDto], input: &str) {
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
