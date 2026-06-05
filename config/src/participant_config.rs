use eyre::{
    OptionExt as _,
    Result,
};
use names::{
    Generator,
    Name,
};
use url::Url;

#[derive(Debug, Clone)]
pub struct ParticipantConfig {
    pub username: String,
    pub session_url: Url,
    pub app_config: super::Config,
}

impl ParticipantConfig {
    pub fn new(config: &super::Config, name: Option<impl ToString>) -> Result<Self> {
        let name = if let Some(name) = name {
            name.to_string()
        } else {
            format!("{}{}", backend_name_prefix(config.backend), generate_random_name())
        };
        let url = config.url.clone().ok_or_eyre("No session URL provided")?;
        Ok(Self {
            username: name,
            session_url: url,
            app_config: config.clone(),
        })
    }

    pub fn base_url(&self) -> Url {
        let mut url = self.session_url.clone();
        url.set_path("/");
        url
    }
}

pub fn generate_random_name() -> String {
    let mut generator = Generator::with_naming(Name::Numbered);
    generator.next().unwrap()
}

const fn backend_name_prefix(backend: super::ParticipantBackendKind) -> &'static str {
    match backend {
        super::ParticipantBackendKind::Local => "local-",
        super::ParticipantBackendKind::RemoteStub => "stub-",
        super::ParticipantBackendKind::Cloudflare => "cf-",
        super::ParticipantBackendKind::AwsDeviceFarm => "aws-",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParticipantBackendKind;

    fn config_with_backend(backend: ParticipantBackendKind) -> super::super::Config {
        super::super::Config {
            url: Some(Url::parse("https://example.com/space/test").expect("valid URL")),
            backend,
            ..Default::default()
        }
    }

    #[test]
    fn generated_names_are_prefixed_by_backend() {
        let cases = [
            (ParticipantBackendKind::Local, "local-"),
            (ParticipantBackendKind::RemoteStub, "stub-"),
            (ParticipantBackendKind::Cloudflare, "cf-"),
            (ParticipantBackendKind::AwsDeviceFarm, "aws-"),
        ];

        for (backend, prefix) in cases {
            let config = config_with_backend(backend);
            let participant = ParticipantConfig::new(&config, None::<String>).expect("participant config");

            assert!(
                participant.username.starts_with(prefix),
                "expected {:?} generated name {:?} to start with {:?}",
                backend,
                participant.username,
                prefix
            );
        }
    }

    #[test]
    fn provided_names_are_not_prefixed() {
        let config = config_with_backend(ParticipantBackendKind::Cloudflare);
        let participant = ParticipantConfig::new(&config, Some("Ada")).expect("participant config");

        assert_eq!(participant.username, "Ada");
    }
}
