//! Dictionary loading pipeline: parses TSV lexicons
//! (`word<TAB>pinyin syllables<TAB>frequency`) and loads them into the engine.
//!
//! This is the integration point for real dictionaries (e.g. converted
//! rime-essay data). The FST-backed storage planned for v1.0 will live here
//! as well, keeping the same [`DictEntry`] surface.

use novatype_core::Engine;
use std::fmt;
use std::path::Path;

/// One parsed dictionary entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictEntry {
    pub text: String,
    pub reading: Vec<String>,
    pub frequency: u32,
}

/// Errors produced while parsing a dictionary source.
#[derive(Debug)]
pub struct DictError {
    line: usize,
    message: String,
}

impl fmt::Display for DictError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "dictionary line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for DictError {}

/// Parses TSV dictionary text.
///
/// Empty lines and lines starting with `#` are skipped. Each data line must
/// be `word<TAB>syllable[ syllable...]<TAB>frequency`.
///
/// # Errors
///
/// Returns an error naming the first malformed line.
pub fn parse_tsv(source: &str) -> Result<Vec<DictEntry>, DictError> {
    let mut entries = Vec::new();

    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut fields = line.split('\t');
        let (Some(text), Some(reading), Some(frequency)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return Err(DictError {
                line: index + 1,
                message: "expected 3 tab-separated fields".to_string(),
            });
        };

        let frequency: u32 = frequency.trim().parse().map_err(|_| DictError {
            line: index + 1,
            message: format!("invalid frequency `{frequency}`"),
        })?;

        let reading: Vec<String> = reading.split_whitespace().map(str::to_string).collect();
        if reading.is_empty() || text.is_empty() {
            return Err(DictError {
                line: index + 1,
                message: "empty word or reading".to_string(),
            });
        }

        entries.push(DictEntry {
            text: text.to_string(),
            reading,
            frequency,
        });
    }

    Ok(entries)
}

/// Loads a TSV dictionary file into the engine.
///
/// Returns the number of entries loaded.
///
/// # Errors
///
/// Returns an error when the file cannot be read or contains malformed lines.
pub fn load_tsv_file(
    engine: &mut Engine,
    path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(path)?;
    Ok(load_tsv(engine, &source)?)
}

/// Loads TSV dictionary text into the engine.
///
/// # Errors
///
/// Returns an error when the text contains malformed lines.
pub fn load_tsv(engine: &mut Engine, source: &str) -> Result<usize, DictError> {
    let entries = parse_tsv(source)?;
    let count = entries.len();
    for entry in entries {
        engine.add_word(entry.reading, entry.text, entry.frequency);
    }
    Ok(count)
}

/// Parses a Rime `.dict.yaml` dictionary into normalized entries.
///
/// The parser consumes lines after the `...` marker. Supported data rows are:
/// `word<TAB>syllables<TAB>frequency`; frequency is optional and defaults to 1.
/// Inline comments beginning with `#` are ignored.
///
/// # Errors
///
/// Returns an error naming the first malformed data line.
pub fn parse_rime_dict(source: &str) -> Result<Vec<DictEntry>, DictError> {
    let mut entries = Vec::new();
    let mut in_body = false;

    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if !in_body {
            if line == "..." {
                in_body = true;
            }
            continue;
        }

        let Some(line) = line.split('#').next().map(str::trim) else {
            continue;
        };
        if line.is_empty() {
            continue;
        }

        let fields = line.split('\t').map(str::trim).collect::<Vec<_>>();
        if fields.len() < 2 {
            return Err(DictError {
                line: index + 1,
                message: "expected at least word and reading fields".to_string(),
            });
        }

        let frequency =
            fields
                .get(2)
                .filter(|field| !field.is_empty())
                .map_or(Ok(1_u32), |field| {
                    field.parse::<u32>().map_err(|_| DictError {
                        line: index + 1,
                        message: format!("invalid frequency `{field}`"),
                    })
                })?;

        let reading = fields[1]
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if fields[0].is_empty() || reading.is_empty() {
            return Err(DictError {
                line: index + 1,
                message: "empty word or reading".to_string(),
            });
        }

        entries.push(DictEntry {
            text: fields[0].to_string(),
            reading,
            frequency,
        });
    }

    Ok(entries)
}

/// Serializes dictionary entries as `NovaType` TSV.
#[must_use]
pub fn entries_to_tsv(entries: &[DictEntry]) -> String {
    let mut output = String::new();
    for entry in entries {
        output.push_str(&entry.text);
        output.push('\t');
        output.push_str(&entry.reading.join(" "));
        output.push('\t');
        output.push_str(&entry.frequency.to_string());
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{entries_to_tsv, load_tsv, parse_rime_dict, parse_tsv};
    use novatype_core::Engine;

    const SAMPLE: &str = "\
# comment line
你好\tni hao\t18000
测试\tce shi\t9000
";

    #[test]
    fn parses_tsv_entries() {
        let entries = parse_tsv(SAMPLE).expect("parse");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].text, "测试");
        assert_eq!(entries[1].reading, vec!["ce", "shi"]);
        assert_eq!(entries[1].frequency, 9_000);
    }

    #[test]
    fn rejects_malformed_line() {
        let error = parse_tsv("你好 ni hao 18000").expect_err("should fail");
        assert!(error.to_string().contains("line 1"));
    }

    #[test]
    fn loads_entries_into_engine() {
        let mut engine = Engine::new();
        let count = load_tsv(&mut engine, SAMPLE).expect("load");

        assert_eq!(count, 2);
        let candidates = engine.suggest("ceshi", 3);
        assert_eq!(
            candidates.first().map(|candidate| candidate.text.as_str()),
            Some("测试")
        );
    }

    #[test]
    fn parses_rime_dict_body() {
        let source = "---\nname: demo\n...\n你好\tni hao\t18000\n测试\tce shi\t9000 # comment\n默认\tmo ren\n";
        let entries = parse_rime_dict(source).expect("parse rime");

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].text, "你好");
        assert_eq!(entries[1].reading, vec!["ce", "shi"]);
        assert_eq!(entries[2].frequency, 1);
    }

    #[test]
    fn writes_tsv() {
        let entries = parse_tsv(SAMPLE).expect("parse");
        let tsv = entries_to_tsv(&entries);

        assert!(tsv.contains("你好\tni hao\t18000"));
        assert!(tsv.contains("测试\tce shi\t9000"));
    }
}
