use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub text: String,
    pub reading: Vec<String>,
    pub score: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct SeedEntry {
    pub reading: &'static [&'static str],
    pub text: &'static str,
    pub frequency: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexiconEntry {
    pub reading: Vec<String>,
    pub text: String,
    pub frequency: u32,
}

#[derive(Debug, Clone)]
pub struct Engine {
    syllables: Vec<String>,
    seg_syllables: Vec<String>,
    lexicon: Vec<LexiconEntry>,
    bigrams: HashMap<String, HashMap<String, f64>>,
    fuzzy: bool,
}

#[derive(Debug, Clone)]
struct Path {
    text: String,
    reading: Vec<String>,
    last_word: String,
    score: f64,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    #[must_use]
    pub fn new() -> Self {
        let syllables = seed_syllables();
        Self {
            seg_syllables: syllables.clone(),
            syllables,
            lexicon: seed_lexicon(),
            bigrams: seed_bigrams(),
            fuzzy: false,
        }
    }

    /// Enables or disables fuzzy pinyin matching
    /// (`zh↔z`, `ch↔c`, `sh↔s`, `ang↔an`, `eng↔en`, `ing↔in`).
    pub fn set_fuzzy(&mut self, enabled: bool) {
        self.fuzzy = enabled;
        self.rebuild_seg_syllables();
    }

    /// Returns whether fuzzy pinyin matching is enabled.
    #[must_use]
    pub fn fuzzy(&self) -> bool {
        self.fuzzy
    }

    fn rebuild_seg_syllables(&mut self) {
        self.seg_syllables.clone_from(&self.syllables);
        if self.fuzzy {
            for syllable in &self.syllables {
                for variant in fuzzy_variants(syllable) {
                    if !self.seg_syllables.contains(&variant) {
                        self.seg_syllables.push(variant);
                    }
                }
            }
        }
    }

    /// Adds a word to the lexicon at runtime (e.g. user-learned words).
    ///
    /// Unknown syllables in `reading` are registered so segmentation can
    /// find the word. Duplicate entries are ignored.
    pub fn add_word(&mut self, reading: Vec<String>, text: impl Into<String>, frequency: u32) {
        let text = text.into();
        if reading.is_empty() || text.is_empty() {
            return;
        }
        if self
            .lexicon
            .iter()
            .any(|entry| entry.text == text && entry.reading == reading)
        {
            return;
        }

        for syllable in &reading {
            if !self.syllables.contains(syllable) {
                self.syllables.push(syllable.clone());
            }
        }
        self.lexicon.push(LexiconEntry {
            reading,
            text,
            frequency,
        });
        self.rebuild_seg_syllables();
    }

    #[must_use]
    pub fn suggest(&self, input: &str, limit: usize) -> Vec<Candidate> {
        let normalized = normalize_input(input);
        if normalized.is_empty() || limit == 0 {
            return Vec::new();
        }

        let segmentations = self.segment(&normalized);
        let mut candidates = segmentations
            .iter()
            .flat_map(|reading| self.decode(reading))
            .collect::<Vec<_>>();

        candidates.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.text.cmp(&right.text))
        });
        candidates.dedup_by(|left, right| left.text == right.text);
        candidates.truncate(limit);
        candidates
    }

    fn segment(&self, input: &str) -> Vec<Vec<String>> {
        let mut states: Vec<Vec<Vec<String>>> = vec![Vec::new(); input.len() + 1];
        states[0].push(Vec::new());

        for start in 0..input.len() {
            if states[start].is_empty() || !input.is_char_boundary(start) {
                continue;
            }

            for syllable in &self.seg_syllables {
                let end = start + syllable.len();
                if end <= input.len() && &input[start..end] == syllable.as_str() {
                    let previous = states[start].clone();
                    for mut path in previous {
                        path.push(syllable.clone());
                        states[end].push(path);
                    }
                }
            }
        }

        states[input.len()].clone()
    }

    fn decode(&self, reading: &[String]) -> Vec<Candidate> {
        let mut beams: Vec<Vec<Path>> = vec![Vec::new(); reading.len() + 1];
        beams[0].push(Path {
            text: String::new(),
            reading: Vec::new(),
            last_word: String::new(),
            score: 0.0,
        });

        for index in 0..reading.len() {
            if beams[index].is_empty() {
                continue;
            }

            let current = beams[index].clone();
            for path in current {
                for entry in self.entries_at(reading, index) {
                    let next_index = index + entry.reading.len();
                    let mut next = path.clone();
                    next.text.push_str(&entry.text);
                    next.reading.extend(entry.reading.iter().cloned());
                    next.score += score_entry(entry);
                    next.score += self
                        .bigrams
                        .get(&path.last_word)
                        .and_then(|followers| followers.get(&entry.text))
                        .copied()
                        .unwrap_or_default();
                    next.last_word.clone_from(&entry.text);
                    beams[next_index].push(next);
                }
            }

            prune(&mut beams[index + 1], 8);
        }

        beams[reading.len()]
            .iter()
            .map(|path| Candidate {
                text: path.text.clone(),
                reading: path.reading.clone(),
                score: path.score,
            })
            .collect()
    }

    fn entries_at(&self, reading: &[String], start: usize) -> Vec<&LexiconEntry> {
        self.lexicon
            .iter()
            .filter(|entry| {
                let end = start + entry.reading.len();
                end <= reading.len()
                    && entry
                        .reading
                        .iter()
                        .zip(&reading[start..end])
                        .all(|(left, right)| left == right || (self.fuzzy && fuzzy_eq(left, right)))
            })
            .collect()
    }
}

fn prune(paths: &mut Vec<Path>, limit: usize) {
    paths.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
    });
    paths.truncate(limit);
}

fn normalize_input(input: &str) -> String {
    input
        .chars()
        .filter(char::is_ascii_alphabetic)
        .flat_map(char::to_lowercase)
        .collect()
}

const FUZZY_INITIALS: [(&str, &str); 3] = [("zh", "z"), ("ch", "c"), ("sh", "s")];
const FUZZY_FINALS: [(&str, &str); 3] = [("ang", "an"), ("eng", "en"), ("ing", "in")];

/// Returns fuzzy spellings of `syllable` (excluding itself).
fn fuzzy_variants(syllable: &str) -> Vec<String> {
    let mut variants = Vec::new();

    for (full, short) in FUZZY_INITIALS {
        if let Some(rest) = syllable.strip_prefix(full) {
            variants.push(format!("{short}{rest}"));
        } else if let Some(rest) = syllable.strip_prefix(short)
            && !syllable.starts_with(full)
        {
            variants.push(format!("{full}{rest}"));
        }
    }

    let base = variants.clone();
    for candidate in std::iter::once(syllable.to_string()).chain(base) {
        for (full, short) in FUZZY_FINALS {
            if let Some(head) = candidate.strip_suffix(full) {
                let variant = format!("{head}{short}");
                if variant != syllable && !variants.contains(&variant) {
                    variants.push(variant);
                }
            } else if let Some(head) = candidate.strip_suffix(short) {
                let variant = format!("{head}{full}");
                if variant != syllable && !variants.contains(&variant) {
                    variants.push(variant);
                }
            }
        }
    }

    variants.retain(|variant| variant != syllable);
    variants
}

/// Whether two syllables match under fuzzy rules.
fn fuzzy_eq(left: &str, right: &str) -> bool {
    left == right
        || fuzzy_variants(left).iter().any(|variant| variant == right)
        || fuzzy_variants(right).iter().any(|variant| variant == left)
}

fn score_entry(entry: &LexiconEntry) -> f64 {
    let reading_len = u32::try_from(entry.reading.len()).expect("reading length fits in u32");
    f64::from(entry.frequency).ln() + f64::from(reading_len) * 0.15
}

fn seed_syllables() -> Vec<String> {
    [
        "ai", "de", "fa", "guo", "hao", "jie", "jin", "men", "ming", "neng", "ni", "ren", "ru",
        "shi", "shu", "wo", "xi", "xian", "xin", "xing", "xue", "yi", "you", "zhi", "zhong",
    ]
    .iter()
    .map(|syllable| (*syllable).to_string())
    .collect()
}

fn seed_lexicon() -> Vec<LexiconEntry> {
    SEED_LEXICON
        .iter()
        .map(|seed| LexiconEntry {
            reading: seed
                .reading
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
            text: seed.text.to_string(),
            frequency: seed.frequency,
        })
        .collect()
}

const SEED_LEXICON: &[SeedEntry] = &[
    SeedEntry {
        reading: &["ni"],
        text: "你",
        frequency: 8_000,
    },
    SeedEntry {
        reading: &["hao"],
        text: "好",
        frequency: 7_500,
    },
    SeedEntry {
        reading: &["ni", "hao"],
        text: "你好",
        frequency: 18_000,
    },
    SeedEntry {
        reading: &["wo"],
        text: "我",
        frequency: 12_000,
    },
    SeedEntry {
        reading: &["men"],
        text: "们",
        frequency: 6_000,
    },
    SeedEntry {
        reading: &["wo", "men"],
        text: "我们",
        frequency: 16_000,
    },
    SeedEntry {
        reading: &["shi"],
        text: "是",
        frequency: 15_000,
    },
    SeedEntry {
        reading: &["de"],
        text: "的",
        frequency: 20_000,
    },
    SeedEntry {
        reading: &["zhong"],
        text: "中",
        frequency: 7_000,
    },
    SeedEntry {
        reading: &["guo"],
        text: "国",
        frequency: 6_500,
    },
    SeedEntry {
        reading: &["ren"],
        text: "人",
        frequency: 9_000,
    },
    SeedEntry {
        reading: &["zhong", "guo"],
        text: "中国",
        frequency: 17_000,
    },
    SeedEntry {
        reading: &["zhong", "guo", "ren"],
        text: "中国人",
        frequency: 13_000,
    },
    SeedEntry {
        reading: &["shu"],
        text: "输",
        frequency: 3_000,
    },
    SeedEntry {
        reading: &["ru"],
        text: "入",
        frequency: 3_000,
    },
    SeedEntry {
        reading: &["fa"],
        text: "法",
        frequency: 4_000,
    },
    SeedEntry {
        reading: &["shu", "ru"],
        text: "输入",
        frequency: 12_000,
    },
    SeedEntry {
        reading: &["shu", "ru", "fa"],
        text: "输入法",
        frequency: 15_000,
    },
    SeedEntry {
        reading: &["xue"],
        text: "学",
        frequency: 4_000,
    },
    SeedEntry {
        reading: &["xi"],
        text: "习",
        frequency: 3_600,
    },
    SeedEntry {
        reading: &["xue", "xi"],
        text: "学习",
        frequency: 11_000,
    },
    SeedEntry {
        reading: &["xin"],
        text: "新",
        frequency: 3_500,
    },
    SeedEntry {
        reading: &["xing"],
        text: "星",
        frequency: 2_700,
    },
    SeedEntry {
        reading: &["xin", "xing"],
        text: "新星",
        frequency: 7_000,
    },
    SeedEntry {
        reading: &["zhi", "neng"],
        text: "智能",
        frequency: 10_000,
    },
];

fn seed_bigrams() -> HashMap<String, HashMap<String, f64>> {
    let pairs = [
        ("中国", "人", 1.8),
        ("输入", "法", 2.0),
        ("我们", "学习", 1.5),
        ("你好", "世界", 1.2),
    ];

    let mut bigrams: HashMap<String, HashMap<String, f64>> = HashMap::new();
    for (prev, next, weight) in pairs {
        bigrams
            .entry(prev.to_string())
            .or_default()
            .insert(next.to_string(), weight);
    }
    bigrams
}

#[cfg(test)]
mod tests {
    use super::Engine;

    #[test]
    fn suggests_common_word() {
        let engine = Engine::new();
        let candidates = engine.suggest("nihao", 5);

        assert_eq!(
            candidates.first().map(|candidate| candidate.text.as_str()),
            Some("你好")
        );
    }

    #[test]
    fn decodes_multi_word_phrase() {
        let engine = Engine::new();
        let candidates = engine.suggest("zhongguoren", 5);

        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.text == "中国人")
        );
    }

    #[test]
    fn ignores_non_letters() {
        let engine = Engine::new();
        let candidates = engine.suggest("shu'ru fa", 3);

        assert_eq!(
            candidates.first().map(|candidate| candidate.text.as_str()),
            Some("输入法")
        );
    }

    #[test]
    fn fuzzy_matches_zh_z() {
        let mut engine = Engine::new();
        engine.set_fuzzy(true);

        let candidates = engine.suggest("zongguo", 5);

        assert!(
            candidates.iter().any(|candidate| candidate.text == "中国"),
            "fuzzy z->zh should surface 中国, got {candidates:?}"
        );
    }

    #[test]
    fn fuzzy_disabled_by_default() {
        let engine = Engine::new();
        let candidates = engine.suggest("zongguo", 5);

        assert!(
            !candidates.iter().any(|candidate| candidate.text == "中国"),
            "fuzzy must be opt-in"
        );
    }

    #[test]
    fn add_word_makes_new_syllables_segmentable() {
        let mut engine = Engine::new();
        engine.add_word(vec!["ce".to_string(), "shi".to_string()], "测试", 9_000);

        let candidates = engine.suggest("ceshi", 3);

        assert_eq!(
            candidates.first().map(|candidate| candidate.text.as_str()),
            Some("测试")
        );
    }
}
