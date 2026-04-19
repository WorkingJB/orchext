use crate::error::{McpError, Result};

pub const SCHEME_PREFIX: &str = "mytex://vault/";

/// What a `mytex://vault/...` URI points at.
pub enum Parsed {
    Root,
    Type(String),
    Document { type_: String, id: String },
}

pub fn parse_uri(uri: &str) -> Result<Parsed> {
    // Accept `mytex://vault` and `mytex://vault/` as equivalent roots.
    if uri == "mytex://vault" || uri == SCHEME_PREFIX {
        return Ok(Parsed::Root);
    }
    let rest = uri
        .strip_prefix(SCHEME_PREFIX)
        .ok_or_else(|| McpError::InvalidArgument(format!("not a mytex vault URI: {uri}")))?;

    if rest.is_empty() {
        return Ok(Parsed::Root);
    }

    let trimmed = rest.trim_end_matches('/');
    let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();

    match segments.as_slice() {
        [t] if rest.ends_with('/') => Ok(Parsed::Type((*t).to_string())),
        [t] => Ok(Parsed::Type((*t).to_string())),
        [t, id] => Ok(Parsed::Document {
            type_: (*t).to_string(),
            id: (*id).to_string(),
        }),
        _ => Err(McpError::InvalidArgument(format!(
            "malformed mytex vault URI: {uri}"
        ))),
    }
}

pub mod resource_definitions {
    use mytex_vault::{Document, Entry};
    use serde_json::{json, Value};

    pub fn document(entry: &Entry, doc: &Document) -> Value {
        let uri = format!("mytex://vault/{}/{}", entry.type_, entry.id);
        let title = crate::title::derive_title(&doc.body, entry.id.as_str());
        json!({
            "uri": uri,
            "name": title,
            "description": format!("{} · visibility:{}", entry.type_, doc.frontmatter.visibility),
            "mimeType": "text/markdown"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_root() {
        assert!(matches!(parse_uri("mytex://vault/").unwrap(), Parsed::Root));
        assert!(matches!(parse_uri("mytex://vault").unwrap(), Parsed::Root));
    }

    #[test]
    fn parses_type_listing() {
        match parse_uri("mytex://vault/relationships/").unwrap() {
            Parsed::Type(t) => assert_eq!(t, "relationships"),
            _ => panic!("expected Type"),
        }
    }

    #[test]
    fn parses_document() {
        match parse_uri("mytex://vault/relationships/rel-jane").unwrap() {
            Parsed::Document { type_, id } => {
                assert_eq!(type_, "relationships");
                assert_eq!(id, "rel-jane");
            }
            _ => panic!("expected Document"),
        }
    }

    #[test]
    fn rejects_foreign_scheme() {
        assert!(parse_uri("https://example.com/").is_err());
    }
}
