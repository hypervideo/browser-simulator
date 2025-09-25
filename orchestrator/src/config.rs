use client_simulator_config::{
    Config as ClientConfig,
    NoiseSuppression,
    TransportMode,
    WebcamResolution,
};
use eyre::{
    eyre,
    Result,
};
use serde::{
    Deserialize,
    Serialize,
};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerUrl {
    pub url: Url,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParticipantDefaults {
    #[serde(default)]
    pub headless: Option<bool>,
    #[serde(default)]
    pub audio_enabled: Option<bool>,
    #[serde(default)]
    pub video_enabled: Option<bool>,
    #[serde(default)]
    pub screenshare_enabled: Option<bool>,
    #[serde(default)]
    pub noise_suppression: Option<NoiseSuppression>,
    #[serde(default)]
    pub transport: Option<TransportMode>,
    #[serde(default)]
    pub resolution: Option<WebcamResolution>,
    #[serde(default)]
    pub blur: Option<bool>,
    #[serde(default)]
    pub fake_media: Option<String>, // "none", "builtin", or URL
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParticipantInitial {
    #[serde(default)]
    pub audio_enabled: Option<bool>,
    #[serde(default)]
    pub video_enabled: Option<bool>,
    #[serde(default)]
    pub screenshare_enabled: Option<bool>,
    #[serde(default)]
    pub blur: Option<bool>,
    #[serde(default)]
    pub noise_suppression: Option<NoiseSuppression>,
    #[serde(default)]
    pub resolution: Option<WebcamResolution>,
    #[serde(default)]
    pub fake_media: Option<String>, // "none", "builtin", or URL
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParticipantSpec {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub wait_to_join_seconds: Option<u64>,
    #[serde(default)]
    pub initial: Option<ParticipantInitial>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    pub session_url: Url,
    pub workers: Vec<WorkerUrl>,
    #[serde(default)]
    pub defaults: Option<ParticipantDefaults>,
    #[serde(default)]
    pub participants_specs: Option<Vec<ParticipantSpec>>,
    #[serde(default)]
    pub run_seconds: Option<u64>,
}

pub fn parse_config(path: &std::path::Path) -> color_eyre::Result<OrchestratorConfig> {
    let bytes = std::fs::read(path)?;
    let content = String::from_utf8(bytes)?;
    let cfg = serde_yml::from_str::<OrchestratorConfig>(&content)?;
    Ok(cfg)
}

// -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
// Helpers to derive effective participant configs

#[derive(Debug, Clone)]
pub struct EffectiveParticipantConfig {
    pub username: String,
    pub client: ClientConfig,
    pub remote_url: url::Url,
    pub fake_media: Option<String>,
}

impl OrchestratorConfig {
    pub fn total_participants(&self) -> usize {
        match self.participants_specs.as_ref() {
            Some(specs) if !specs.is_empty() => specs.len(),
            _ => 0,
        }
    }

    pub fn participant_spec(&self, index: usize) -> ParticipantSpec {
        self.participants_specs
            .as_ref()
            .and_then(|v| v.get(index).cloned())
            .unwrap_or_default()
    }

    pub fn effective_participant(&self, index: usize) -> Result<EffectiveParticipantConfig> {
        // Choose worker URL in round-robin fashion
        let workers_len = self.workers.len();
        let remote_url = self
            .workers
            .get(index % workers_len)
            .ok_or_else(|| eyre!("workers must be non-empty; validate config before use"))?
            .url
            .clone();

        // Start from base ClientConfig with session URL
        let mut client = ClientConfig {
            url: Some(self.session_url.clone()),
            ..ClientConfig::default()
        };

        // Apply global defaults
        if let Some(d) = &self.defaults {
            if let Some(v) = d.audio_enabled {
                client.audio_enabled = v;
            }
            if let Some(v) = d.video_enabled {
                client.video_enabled = v;
            }
            if let Some(v) = d.screenshare_enabled {
                client.screenshare_enabled = v;
            }
            if let Some(v) = d.headless {
                client.headless = v;
            }
            if let Some(v) = d.noise_suppression {
                client.noise_suppression = v;
            }
            if let Some(v) = d.transport {
                client.transport = v;
            }
            if let Some(v) = d.resolution {
                client.resolution = v;
            }
            if let Some(v) = d.blur {
                client.blur = v;
            }
        }

        // Apply spec overrides
        let spec = self.participant_spec(index);
        if let Some(init) = &spec.initial {
            if let Some(v) = init.audio_enabled {
                client.audio_enabled = v;
            }
            if let Some(v) = init.video_enabled {
                client.video_enabled = v;
            }
            if let Some(v) = init.screenshare_enabled {
                client.screenshare_enabled = v;
            }
            if let Some(v) = init.noise_suppression {
                client.noise_suppression = v;
            }
            if let Some(v) = init.resolution {
                client.resolution = v;
            }
            if let Some(v) = init.blur {
                client.blur = v;
            }
        }

        let username = spec.username.unwrap_or_else(|| format!("orch-{}", index));

        // Determine fake_media setting (spec override > global default > None)
        let fake_media = if let Some(init) = &spec.initial {
            init.fake_media.clone()
        } else {
            self.defaults.as_ref().and_then(|d| d.fake_media.clone())
        };

        Ok(EffectiveParticipantConfig {
            username,
            client,
            remote_url,
            fake_media,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.workers.is_empty() {
            return Err(eyre!("config.workers must be non-empty"));
        }
        if self.total_participants() == 0 {
            return Err(eyre!(
                "configure either non-empty participants_specs or participants > 0"
            ));
        }
        Ok(())
    }
}
