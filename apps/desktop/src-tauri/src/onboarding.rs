//! In-app onboarding agent — chat + seed-doc generation via the
//! Claude API (Haiku 4.5).
//!
//! Why raw HTTP: there is no official Anthropic Rust SDK, so we post
//! to `/v1/messages` with `reqwest`. Per the claude-api skill, this is
//! the sanctioned fallback when no SDK exists for the host language.
//!
//! Scope cuts (see implementation-status.md Known gaps):
//!   - no streaming (full response per turn)
//!   - no tool use (agent drafts markdown, we don't let it call
//!     `context.*` or `doc_write` directly)
//!   - single session, no cross-conversation memory

use serde::{Deserialize, Serialize};

const MODEL_ID: &str = "claude-haiku-4-5";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: &'a [ChatMessage],
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

pub const SYSTEM_PROMPT_CHAT: &str = r#"You are Ourtex's onboarding assistant.

Ourtex is a local-first personal-context vault: a folder of markdown files organized by type (identity, roles, goals, relationships, memories, tools, preferences, domains, decisions). AI agents read these files via MCP with scoped access so they can know the user without the user re-explaining themselves every conversation.

Your job: run a short, friendly interview (6–10 turns total) to gather enough to seed a new vault. Ask about one area at a time. Be concise — 1–3 sentences plus a focused question per turn. Do not lecture about ourtex; just ask useful questions.

Cover (in any order that feels natural): who the user is (identity), what they do (roles), what they're working on or care about (goals), a few key relationships, and any strong preferences or tools worth remembering. Skip anything that doesn't feel relevant.

Never draft documents mid-interview. When the user clicks "Finish", a separate turn will ask you to emit the seed docs as JSON — stay in conversational mode until then."#;

pub const SYSTEM_PROMPT_FINALIZE: &str = r##"You are Ourtex's onboarding assistant, now finalizing the vault seed.

Based on the conversation so far, output a JSON array of seed documents. Return ONLY the JSON — no prose, no markdown code fences, no explanation.

Schema per document:
{
  "id": "kebab-case-id",        // unique within type; letters/digits/dashes
  "type": "identity|roles|goals|relationships|memories|tools|preferences|domains|decisions",
  "visibility": "public|work|personal|private",
  "body": "# Title\n\nMarkdown body..."
}

Guidance:
- 4–10 documents total.
- Default visibility: "personal". Use "work" for job/role content, "private" only if the user clearly signaled secrecy.
- Every body starts with a level-1 heading (# Title).
- Keep bodies short and specific — a few sentences to a short list.
- IDs must be unique and descriptive (e.g., "role-staff-engineer", "rel-manager-alex").
- If the conversation didn't cover an area, skip it rather than invent."##;

#[derive(Deserialize)]
pub struct SeedDocDraft {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub visibility: String,
    pub body: String,
}

pub async fn chat(
    api_key: &str,
    system: &str,
    history: &[ChatMessage],
) -> Result<String, String> {
    let body = MessagesRequest {
        model: MODEL_ID,
        max_tokens: 2048,
        system,
        messages: history,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("claude api {status}: {text}"));
    }

    let parsed: MessagesResponse = resp
        .json()
        .await
        .map_err(|e| format!("decode response: {e}"))?;

    let mut out = String::new();
    for block in parsed.content {
        if let ContentBlock::Text { text } = block {
            out.push_str(&text);
        }
    }
    Ok(out)
}

/// Strip common wrappings an LLM might add around a JSON array
/// (```json fences, leading prose) and return the array substring.
pub fn extract_json_array(text: &str) -> Option<&str> {
    let start = text.find('[')?;
    let end = text.rfind(']')?;
    if end <= start {
        return None;
    }
    Some(&text[start..=end])
}
