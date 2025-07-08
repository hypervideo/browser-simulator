#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemoteUrlOption {
    url: url::Url,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

impl RemoteUrlOption {
    pub fn new(url: url::Url, description: Option<String>) -> Self {
        Self { url, description }
    }

    pub fn url(&self) -> &url::Url {
        &self.url
    }

    pub fn description(&self) -> &str {
        if let Some(description) = self.description.as_ref() {
            return description.as_str();
        }

        ""
    }
}
