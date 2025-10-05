use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalarOptions {
    pub openapi_document_route_template: String,

    pub servers: Option<Vec<ScalarServer>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(default = "default_theme")]
    pub theme: String,

    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default)]
    pub hide_download_button: bool,
    pub show_side_bar: Option<bool>,
    pub with_default_fonts: Option<bool>,
    pub layout: Option<String>,
    #[serde(default)]
    pub custom_css: String,
    pub search_hotkey: Option<String>,

    pub metadata: Option<HashMap<String, String>>,

    pub authentication: Option<ScalarAuthenticationOptions>,
}

fn default_theme() -> String {
    "purple".to_string()
}

impl Default for ScalarOptions {
    fn default() -> Self {
        Self {
            servers: None,
            openapi_document_route_template: "/openapi/{version}/{format}"
                .to_string(),
            title: None,
            theme: default_theme(),
            dark_mode: true,
            hide_download_button: false,
            show_side_bar: None,
            with_default_fonts: None,
            layout: None,
            custom_css: "".to_string(),
            search_hotkey: None,
            metadata: None,
            authentication: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScalarAuthenticationOptions {
    pub preferred_security_scheme: Option<String>,
    pub api_key: Option<ScalarAuthenticationApiKey>,
    // If you want OAuth2 later, you can add:
    pub oauth2: Option<ScalarAuthenticationOauth2>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScalarAuthenticationApiKey {
    pub token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScalarAuthenticationOauth2 {
    pub client_id: Option<String>,
    pub scopes: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScalarServer {
    pub url: String,
    pub description: Option<String>,
    pub variables: Option<HashMap<String, ScalarServerVariableValue>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScalarServerVariableValue {
    pub default: String,
}
