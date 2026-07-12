//! Agent command parsing and prompt templating.
//!
//! Commands use a `//` prefix in the input buffer, e.g. `//翻译 hello world`
//! or `//润色 这段话有点生硬`. Parsing is pure and independent of any LLM;
//! execution delegates to a [`novatype_llm::LlmBackend`].

use novatype_llm::{LlmBackend, LlmResult};

/// A parsed agent command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCommand {
    pub action: AgentAction,
    pub payload: String,
}

/// Supported agent actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentAction {
    Translate,
    Polish,
    Reply,
    Summarize,
}

impl AgentAction {
    /// The command keyword as typed by the user.
    #[must_use]
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Translate => "翻译",
            Self::Polish => "润色",
            Self::Reply => "回复",
            Self::Summarize => "总结",
        }
    }

    fn from_keyword(keyword: &str) -> Option<Self> {
        match keyword {
            "翻译" | "translate" => Some(Self::Translate),
            "润色" | "polish" => Some(Self::Polish),
            "回复" | "reply" => Some(Self::Reply),
            "总结" | "summarize" => Some(Self::Summarize),
            _ => None,
        }
    }

    /// Builds the prompt for this action.
    #[must_use]
    pub fn prompt(self, payload: &str) -> String {
        match self {
            Self::Translate => format!(
                "你是输入法里的翻译助手。把下面的内容在中英文之间互译，只输出译文，不要解释：\n{payload}"
            ),
            Self::Polish => format!(
                "你是输入法里的润色助手。把下面这句话润色得自然、简洁，只输出润色后的文本：\n{payload}"
            ),
            Self::Reply => format!(
                "你是输入法里的回复助手。根据以下意图，写一句得体的中文回复，只输出回复文本：\n{payload}"
            ),
            Self::Summarize => {
                format!("你是输入法里的总结助手。用一句话总结以下内容，只输出总结：\n{payload}")
            }
        }
    }
}

/// Parses an input buffer into an agent command.
///
/// Returns `None` when the input is not an agent command (no `//` prefix,
/// unknown keyword, or empty payload).
#[must_use]
pub fn parse(input: &str) -> Option<AgentCommand> {
    let rest = input.strip_prefix("//")?;
    let mut parts = rest.splitn(2, [' ', '\u{3000}']);
    let keyword = parts.next()?.trim();
    let payload = parts.next().unwrap_or("").trim();

    let action = AgentAction::from_keyword(keyword)?;
    if payload.is_empty() {
        return None;
    }

    Some(AgentCommand {
        action,
        payload: payload.to_string(),
    })
}

/// Executes a parsed command against a backend.
///
/// # Errors
///
/// Propagates backend errors; callers must degrade gracefully (drop the
/// agent candidate and keep normal candidates).
pub fn execute(command: &AgentCommand, backend: &dyn LlmBackend) -> LlmResult<String> {
    backend.complete(&command.action.prompt(&command.payload))
}

#[cfg(test)]
mod tests {
    use super::{AgentAction, parse};

    #[test]
    fn parses_translate_command() {
        let command = parse("//翻译 hello world").expect("command");

        assert_eq!(command.action, AgentAction::Translate);
        assert_eq!(command.payload, "hello world");
    }

    #[test]
    fn parses_english_keyword() {
        let command = parse("//polish make this nicer").expect("command");
        assert_eq!(command.action, AgentAction::Polish);
    }

    #[test]
    fn rejects_normal_input() {
        assert!(parse("nihao").is_none());
        assert!(parse("//未知 内容").is_none());
        assert!(parse("//翻译").is_none());
        assert!(parse("//翻译   ").is_none());
    }

    #[test]
    fn prompt_contains_payload() {
        let prompt = AgentAction::Summarize.prompt("很长的一段话");
        assert!(prompt.contains("很长的一段话"));
    }
}
