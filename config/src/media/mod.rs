mod custom_fake_media;

pub use custom_fake_media::{
    FakeMediaFileOrUrl,
    FakeMediaFiles,
};

#[derive(Debug, Default, Clone, PartialEq)]
pub enum FakeMedia {
    #[default]
    None,
    Builtin,
    FileOrUrl(String),
}

const NONE: &str = "<none>";
const BUILTIN: &str = "<builtin>";

impl std::fmt::Display for FakeMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FakeMedia::None => write!(f, "{NONE}"),
            FakeMedia::Builtin => write!(f, "{BUILTIN}"),
            FakeMedia::FileOrUrl(file_or_url) => write!(f, "{file_or_url}"),
        }
    }
}

impl<T: AsRef<str>> From<T> for FakeMedia {
    fn from(arg: T) -> Self {
        match arg.as_ref() {
            NONE => FakeMedia::None,
            BUILTIN => FakeMedia::Builtin,
            arg => FakeMedia::FileOrUrl(arg.to_string()),
        }
    }
}

impl serde::Serialize for FakeMedia {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            FakeMedia::None => serializer.serialize_str(NONE),
            FakeMedia::Builtin => serializer.serialize_str(BUILTIN),
            FakeMedia::FileOrUrl(file_or_url) => serializer.serialize_str(file_or_url),
        }
    }
}

impl<'de> serde::Deserialize<'de> for FakeMedia {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(String::deserialize(deserializer)?.into())
    }
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FakeMediaWithDescription {
    fake_media: FakeMedia,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

impl FakeMediaWithDescription {
    pub fn new(fake_media: FakeMedia, description: Option<String>) -> Self {
        Self {
            fake_media,
            description,
        }
    }

    pub fn fake_media(&self) -> &FakeMedia {
        &self.fake_media
    }

    pub fn description(&self) -> &str {
        if let Some(description) = self.description.as_ref() {
            return description.as_str();
        }

        match self.fake_media {
            FakeMedia::None => NONE,
            FakeMedia::Builtin => BUILTIN,
            FakeMedia::FileOrUrl(_) => "",
        }
    }
}
