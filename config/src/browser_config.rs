use std::path::{
    Path,
    PathBuf,
};
use temp_dir::TempDir;

#[derive(Clone, Debug)]
pub enum UserDataDir {
    /// Use a temporary directory for user data. Will be deleted on drop.
    Temp { temp_dir: TempDir, user_data_dir: PathBuf },
    /// Use a custom directory for user data.
    Custom(PathBuf),
}

impl AsRef<Path> for UserDataDir {
    fn as_ref(&self) -> &Path {
        match self {
            UserDataDir::Temp { user_data_dir, .. } => user_data_dir,
            UserDataDir::Custom(user_data_dir) => user_data_dir,
        }
    }
}

impl Default for UserDataDir {
    fn default() -> Self {
        let temp_dir = temp_dir::TempDir::with_prefix("hyper-browser-simulator").expect("Failed to create temp dir");
        let user_data_dir = temp_dir.path().to_path_buf();
        Self::Temp {
            temp_dir,
            user_data_dir,
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct BrowserConfig {
    pub user_data_dir: UserDataDir,
    pub cache_dir: PathBuf,
    pub app_config: super::Config,
}

impl From<&super::ParticipantConfig> for BrowserConfig {
    fn from(config: &super::ParticipantConfig) -> Self {
        Self {
            user_data_dir: Default::default(),
            cache_dir: super::app_config::cache_dir(),
            app_config: config.app_config.clone(),
        }
    }
}
