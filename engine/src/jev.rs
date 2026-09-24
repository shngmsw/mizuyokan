use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::Duration,
};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct JevConfig {
    pub api_key: String,
    /// Default: https://api.typesafe.ai/v1/systemone
    pub endpoint: String,
    /// Pinned so that probability thresholds do not drift with `jev-latest`.
    pub model: String,
    /// Called synchronously from the TSF thread when the user presses Space,
    /// so a slow network must fall back to the offline reading instead of blocking input.
    pub timeout: Duration,
}

impl Default for JevConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            endpoint: "https://api.typesafe.ai/v1/systemone".to_string(),
            model: "jev-1.13.0".to_string(),
            timeout: Duration::from_millis(1500),
        }
    }
}

#[derive(Debug, Error)]
pub enum JevError {
    #[error("http error: {0}")]
    Http(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("missing api key")]
    MissingKey,
}

#[derive(Debug, Serialize)]
struct SystemOneRequest<'a> {
    model: &'a str,
    state: serde_json::Value,
    questions: serde_json::Map<String, serde_json::Value>,
}

pub struct JevClient {
    cfg: JevConfig,
}

/// Shared across calls so the TLS connection to the gateway is reused;
/// a fresh handshake costs a few hundred milliseconds on every Space.
fn agent(timeout: Duration) -> ureq::Agent {
    static AGENTS: LazyLock<Mutex<HashMap<Duration, ureq::Agent>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let build = || ureq::AgentBuilder::new().timeout(timeout).build();
    match AGENTS.lock() {
        Ok(mut agents) => agents.entry(timeout).or_insert_with(build).clone(),
        Err(_) => build(),
    }
}

const QUESTION: &str = "reading";
const WORD_QUESTION: &str = "word";

impl JevClient {
    pub fn new(cfg: JevConfig) -> Self {
        Self { cfg }
    }

    /// Ask which rendering of an IME keystroke buffer the user most likely meant.
    /// Returns one probability per option, in the order given.
    pub fn choose_reading(&self, raw: &str, options: &[String]) -> Result<Vec<f64>, JevError> {
        if self.cfg.api_key.is_empty() {
            return Err(JevError::MissingKey);
        }
        if options.len() < 2 {
            return Ok(vec![1.0; options.len()]);
        }
        self.ask_choice(
            QUESTION,
            "A user typed the keystrokes in `raw` into a Japanese IME without switching between Japanese and English.                 Romaji meant as Japanese is converted to kana/kanji; English words are kept as typed.                 Which option is the text the user most likely intended?",
            serde_json::json!({ "raw": raw }),
            options,
        )
    }

    /// Probability that `word` (lowercase, as committed through the IME) is a
    /// real English word or an established technical term / abbreviation,
    /// rather than a typo, key mashing or a fragment of Japanese romaji.
    /// Used to decide whether the IME should learn the word.
    pub fn real_word_probability(&self, word: &str) -> Result<f64, JevError> {
        if self.cfg.api_key.is_empty() {
            return Err(JevError::MissingKey);
        }
        let options = [
            "A real word: a common English word, or a technical term, command, abbreviation                 or product name widely used in software and IT (for example: ssh, pc, npm,                 kubectl, github, docker, figma, slack)."
                .to_string(),
            "Not a real word: a typo, random key mashing, or a fragment of Japanese romaji                 typed without converting (for example: att, ltu, okik, dstry)."
                .to_string(),
        ];
        let probs = self.ask_choice(
            WORD_QUESTION,
            "A user of a Japanese IME typed `word` in Latin letters and committed it as English.                 The IME will remember it as an English word only if it really is one.                 Which option describes `word`?",
            serde_json::json!({ "word": word }),
            &options,
        )?;
        Ok(probs[0])
    }

    /// One "choice" question; returns one probability per option, in order.
    fn ask_choice(
        &self,
        key: &str,
        instructions: &str,
        state: serde_json::Value,
        options: &[String],
    ) -> Result<Vec<f64>, JevError> {
        let mut criteria = serde_json::Map::new();
        for (i, option) in options.iter().enumerate() {
            criteria.insert(format!("o{i}"), serde_json::Value::String(option.clone()));
        }
        let mut questions = serde_json::Map::new();
        questions.insert(
            key.to_string(),
            serde_json::json!({
                "type": "choice",
                "instructions": instructions,
                "criteria": criteria,
            }),
        );

        let body = SystemOneRequest {
            model: &self.cfg.model,
            state,
            questions,
        };

        let resp = agent(self.cfg.timeout)
            .post(&self.cfg.endpoint)
            .set("Authorization", &format!("Bearer {}", self.cfg.api_key))
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| JevError::Http(e.to_string()))?;

        let parsed: serde_json::Value = resp
            .into_json()
            .map_err(|e| JevError::Parse(e.to_string()))?;
        parse_choice_for(&parsed, key, options.len())
    }
}

#[cfg(test)]
fn parse_choice(response: &serde_json::Value, n: usize) -> Result<Vec<f64>, JevError> {
    parse_choice_for(response, QUESTION, n)
}

fn parse_choice_for(response: &serde_json::Value, key: &str, n: usize) -> Result<Vec<f64>, JevError> {
    let answer = response
        .get("answers")
        .and_then(|a| a.get(key))
        .ok_or_else(|| JevError::Parse(format!("no answer in {response}")))?;

    if let Some(probs) = answer.get("probabilities").and_then(|p| p.as_object()) {
        return Ok((0..n)
            .map(|i| {
                probs
                    .get(&format!("o{i}"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            })
            .collect());
    }
    let winner = answer
        .get("choice")
        .and_then(|c| c.as_str())
        .ok_or_else(|| JevError::Parse(format!("no choice in {answer}")))?;
    Ok((0..n)
        .map(|i| if winner == format!("o{i}") { 1.0 } else { 0.0 })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_choice_answer() {
        let response = serde_json::json!({
            "model": "jev-1.13.0",
            "answers": {
                "reading": {
                    "type": "choice",
                    "choice": "o1",
                    "probabilities": { "o0": 0.2, "o1": 0.7, "o2": 0.1 },
                    "confidence": 0.6
                }
            },
            "usage": { "input_tokens": 100, "output_tokens": 10 }
        });
        assert_eq!(parse_choice(&response, 3).unwrap(), vec![0.2, 0.7, 0.1]);
    }

    #[test]
    fn falls_back_to_winner_without_probabilities() {
        let response = serde_json::json!({
            "answers": { "reading": { "type": "choice", "choice": "o0" } }
        });
        assert_eq!(parse_choice(&response, 2).unwrap(), vec![1.0, 0.0]);
    }

    #[test]
    fn missing_key_is_an_error() {
        let client = JevClient::new(JevConfig::default());
        assert!(matches!(
            client.choose_reading("abc", &["a".into(), "b".into()]),
            Err(JevError::MissingKey)
        ));
        assert!(matches!(client.real_word_probability("ssh"), Err(JevError::MissingKey)));
    }

    #[test]
    fn parses_word_answer() {
        let response = serde_json::json!({
            "answers": { "word": { "type": "choice", "choice": "o0",
                "probabilities": { "o0": 0.9, "o1": 0.1 } } }
        });
        assert_eq!(parse_choice_for(&response, WORD_QUESTION, 2).unwrap(), vec![0.9, 0.1]);
        // The reading question's answer is not taken for the word question.
        assert!(parse_choice_for(&response, QUESTION, 2).is_err());
    }
}
