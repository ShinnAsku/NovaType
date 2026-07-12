use novatype_dict::{entries_to_tsv, parse_rime_dict};
use std::env;
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: rime-to-tsv <input.dict.yaml> [output.tsv]");
        std::process::exit(2);
    };

    let source = std::fs::read_to_string(&input)?;
    let entries = parse_rime_dict(&source)?;
    let output = entries_to_tsv(&entries);

    match args.next() {
        Some(path) => std::fs::write(PathBuf::from(path), output)?,
        None => print!("{output}"),
    }

    Ok(())
}
