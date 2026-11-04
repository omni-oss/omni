//! The code catalog as MCP resources: `omni://codes/<code>`.

use rmcp::model::{
    ErrorData, ReadResourceResult, Resource, ResourceContents, ResourceTemplate,
};

const PREFIX: &str = "omni://codes/";
const MIME: &str = "application/json";

pub fn list() -> Vec<Resource> {
    let mut codes: Vec<_> = omni_codes::all().collect();
    codes.sort_by_key(|c| c.code);
    codes
        .into_iter()
        .map(|c| {
            Resource::new(format!("{PREFIX}{}", c.code), c.code)
                .with_description(c.summary)
                .with_mime_type(MIME)
        })
        .collect()
}

pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new(format!("{PREFIX}{{code}}"), "code")
            .with_description(
                "Registry entry for an error or notice code, as returned by the `explain` tool",
            )
            .with_mime_type(MIME),
    ]
}

pub fn read(uri: &str) -> Result<ReadResourceResult, ErrorData> {
    let Some(code) = uri.strip_prefix(PREFIX) else {
        return Err(ErrorData::resource_not_found(
            format!("unknown resource `{uri}`"),
            None,
        ));
    };
    let json = crate::tools::explain::explain_json(code)
        .map_err(|e| crate::error::report_to_mcp_error(eyre::Report::new(e)))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::TextResourceContents {
            uri: uri.to_string(),
            mime_type: Some(MIME.to_string()),
            text: serde_json::to_string_pretty(&json).unwrap_or_default(),
            meta: None,
        },
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_entries_are_listed_and_readable() {
        let listed = list();
        assert!(
            listed
                .iter()
                .any(|r| r.uri == "omni://codes/notices.truncated")
        );
        let r = read("omni://codes/notices.truncated").unwrap();
        let ResourceContents::TextResourceContents { text, .. } =
            &r.contents[0]
        else {
            panic!("text expected");
        };
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(v["code"], "notices.truncated");
        assert_eq!(v["kind"], "notice");
        assert_eq!(v["stable"], true);
    }

    #[test]
    fn resource_json_matches_the_explain_tool() {
        let a =
            crate::tools::explain::explain_json("notices.truncated").unwrap();
        let r = read("omni://codes/notices.truncated").unwrap();
        let ResourceContents::TextResourceContents { text, .. } =
            &r.contents[0]
        else {
            panic!()
        };
        assert_eq!(serde_json::from_str::<serde_json::Value>(text).unwrap(), a);
    }

    #[test]
    fn unknown_codes_and_uris_are_errors() {
        assert!(read("omni://codes/no.such").is_err());
        assert!(read("file:///etc/passwd").is_err());
    }
}
