use directories::ProjectDirs;
use serde::Deserialize;
use std::{
    env,
    path::PathBuf,
};

#[derive(Clone, Debug, Deserialize, Default)]
pub(super) struct AppConfig {
    #[expect(unused)]
    #[serde(default)]
    pub(super) data_dir: PathBuf,
    #[serde(default)]
    pub(super) config_dir: PathBuf,
}

lazy_static::lazy_static! {
    pub(crate)static ref PROJECT_NAME: String = env!("CARGO_CRATE_NAME").to_uppercase().to_string();
    static ref DATA_FOLDER: Option<PathBuf> = env::var(format!("{}_DATA", PROJECT_NAME.clone()))
        .ok()
        .map(PathBuf::from);
    static ref CONFIG_FOLDER: Option<PathBuf> = env::var(format!("{}_CONFIG", PROJECT_NAME.clone()))
        .ok()
        .map(PathBuf::from);
}

pub(crate) fn get_data_dir() -> PathBuf {
    let directory = if let Some(s) = DATA_FOLDER.clone() {
        s
    } else if let Some(proj_dirs) = project_directory() {
        proj_dirs.data_local_dir().to_path_buf()
    } else {
        PathBuf::from(".").join(".data")
    };
    directory
}

pub(crate) fn get_config_dir() -> PathBuf {
    let directory = if let Some(s) = CONFIG_FOLDER.clone() {
        s
    } else if let Some(proj_dirs) = project_directory() {
        proj_dirs.config_local_dir().to_path_buf()
    } else {
        PathBuf::from(".").join(".config")
    };
    directory
}

fn project_directory() -> Option<ProjectDirs> {
    ProjectDirs::from("video", "hyper", env!("CARGO_PKG_NAME"))
}
