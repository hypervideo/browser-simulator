use aws_sdk_devicefarm::types::{
    TestGridSession,
    TestGridSessionStatus,
};
use client_simulator_config::{
    device_farm_aws_access_key_id,
    device_farm_aws_secret_access_key,
    DEVICE_FARM_AWS_ACCESS_KEY_ID_ENV,
    DEVICE_FARM_AWS_PROFILE,
    DEVICE_FARM_AWS_SECRET_ACCESS_KEY_ENV,
};
use eyre::{
    bail,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
};
use std::{
    env::VarError,
    error::Error,
};

type DeviceFarmCredentials = aws_sdk_devicefarm::config::Credentials;

/// Minimal seam over the Device Farm Test Grid control plane so the session
/// logic can be tested without real AWS calls.
pub trait TestGridApi: Send + Sync {
    /// Create a short-lived Selenium Remote WebDriver URL for `project_arn`.
    fn create_test_grid_url(&self, project_arn: &str, expires_seconds: u64) -> BoxFuture<'_, Result<String>>;

    fn list_test_grid_sessions(
        &self,
        _project_arn: &str,
        _status: Option<TestGridSessionStatus>,
    ) -> BoxFuture<'_, Result<Vec<TestGridSession>>> {
        async move { bail!("ListTestGridSessions is not implemented by this TestGridApi") }.boxed()
    }

    fn get_test_grid_session(
        &self,
        _project_arn: &str,
        _session_id: &str,
    ) -> BoxFuture<'_, Result<Option<TestGridSession>>> {
        async move { bail!("GetTestGridSession is not implemented by this TestGridApi") }.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_farm_env_credentials_are_absent_when_custom_env_pair_is_absent() {
        let credentials =
            device_farm_env_credentials_from_vars(Err(VarError::NotPresent), Err(VarError::NotPresent)).unwrap();

        assert!(credentials.is_none());
    }

    #[test]
    fn device_farm_env_credentials_require_access_key_and_secret_pair() {
        let error = match device_farm_env_credentials_from_vars(Ok("access-key".to_owned()), Err(VarError::NotPresent))
        {
            Ok(_) => panic!("partial custom credentials should fail"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains(DEVICE_FARM_AWS_SECRET_ACCESS_KEY_ENV));
        assert!(error.contains(DEVICE_FARM_AWS_ACCESS_KEY_ID_ENV));
    }

    #[test]
    fn device_farm_env_credentials_use_custom_env_pair_when_present() {
        let credentials =
            device_farm_env_credentials_from_vars(Ok("access-key".to_owned()), Ok("secret-key".to_owned()))
                .unwrap()
                .expect("credentials should be built");

        assert_eq!(credentials.access_key_id(), "access-key");
        assert_eq!(credentials.secret_access_key(), "secret-key");
    }

    #[test]
    fn aws_device_farm_error_mentions_setup_auth_for_credential_errors() {
        let error = aws_device_farm_error(
            "CreateTestGridUrl failed",
            std::io::Error::other("CredentialsNotLoaded"),
        );
        let error = format!("{error:?}");

        assert!(error.contains("aws setup-auth"));
        assert!(error.contains(DEVICE_FARM_AWS_PROFILE));
    }
}

/// Real implementation backed by `aws-sdk-devicefarm`.
pub struct AwsTestGrid {
    region: String,
    client: tokio::sync::OnceCell<aws_sdk_devicefarm::Client>,
}

impl AwsTestGrid {
    pub fn new(region: &str) -> Self {
        Self {
            region: region.to_owned(),
            client: tokio::sync::OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<&aws_sdk_devicefarm::Client> {
        let region = self.region.clone();
        self.client
            .get_or_try_init(|| async move {
                let region = aws_sdk_devicefarm::config::Region::new(region);
                let config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(region);
                let config_loader = match device_farm_env_credentials()? {
                    Some(credentials) => config_loader.credentials_provider(credentials),
                    None => config_loader.profile_name(DEVICE_FARM_AWS_PROFILE),
                };
                let config = config_loader.load().await;
                Ok(aws_sdk_devicefarm::Client::new(&config))
            })
            .await
    }
}

fn device_farm_env_credentials() -> Result<Option<DeviceFarmCredentials>> {
    device_farm_env_credentials_from_vars(device_farm_aws_access_key_id(), device_farm_aws_secret_access_key())
}

fn device_farm_env_credentials_from_vars(
    access_key_id: std::result::Result<String, VarError>,
    secret_access_key: std::result::Result<String, VarError>,
) -> Result<Option<DeviceFarmCredentials>> {
    match (access_key_id, secret_access_key) {
        (Ok(access_key_id), Ok(secret_access_key)) => Ok(Some(DeviceFarmCredentials::new(
            access_key_id,
            secret_access_key,
            None,
            None,
            "device-farm-env",
        ))),
        (Err(VarError::NotPresent), Err(VarError::NotPresent)) => Ok(None),
        (Ok(_), Err(VarError::NotPresent)) => {
            bail!(
                "{}. {}",
                format_args!(
                    "{DEVICE_FARM_AWS_SECRET_ACCESS_KEY_ENV} is not set while {DEVICE_FARM_AWS_ACCESS_KEY_ID_ENV} is set"
                ),
                credential_setup_hint()
            )
        }
        (Err(VarError::NotPresent), Ok(_)) => {
            bail!(
                "{}. {}",
                format_args!(
                    "{DEVICE_FARM_AWS_ACCESS_KEY_ID_ENV} is not set while {DEVICE_FARM_AWS_SECRET_ACCESS_KEY_ENV} is set"
                ),
                credential_setup_hint()
            )
        }
        (Err(error), _) => bail!("{DEVICE_FARM_AWS_ACCESS_KEY_ID_ENV} could not be read: {error}"),
        (_, Err(error)) => bail!("{DEVICE_FARM_AWS_SECRET_ACCESS_KEY_ENV} could not be read: {error}"),
    }
}

fn aws_device_farm_error(operation: &'static str, error: impl Error + Send + Sync + 'static) -> eyre::Report {
    let is_credential_error = format!("{error:?} {error}").to_ascii_lowercase().contains("credential");
    let error = eyre::Report::new(error).wrap_err(operation);
    if is_credential_error {
        error.wrap_err(credential_setup_hint())
    } else {
        error
    }
}

fn credential_setup_hint() -> String {
    format!("Run `hyper-client-simulator aws setup-auth` to configure the `{DEVICE_FARM_AWS_PROFILE}` AWS profile")
}

impl TestGridApi for AwsTestGrid {
    fn create_test_grid_url(&self, project_arn: &str, expires_seconds: u64) -> BoxFuture<'_, Result<String>> {
        let project_arn = project_arn.to_owned();
        async move {
            let output = self
                .client()
                .await?
                .create_test_grid_url()
                .project_arn(project_arn)
                .expires_in_seconds(expires_seconds as i32)
                .send()
                .await
                .map_err(|error| aws_device_farm_error("CreateTestGridUrl failed", error))?;
            output
                .url()
                .map(str::to_owned)
                .ok_or_else(|| eyre::eyre!("CreateTestGridUrl returned no url"))
        }
        .boxed()
    }

    fn list_test_grid_sessions(
        &self,
        project_arn: &str,
        status: Option<TestGridSessionStatus>,
    ) -> BoxFuture<'_, Result<Vec<TestGridSession>>> {
        let project_arn = project_arn.to_owned();
        async move {
            let mut builder = self.client().await?.list_test_grid_sessions().project_arn(project_arn);
            if let Some(status) = status {
                builder = builder.status(status);
            }

            let mut pages = builder.into_paginator().send();
            let mut sessions = Vec::new();
            while let Some(page) = pages.next().await {
                let page = page.map_err(|error| aws_device_farm_error("ListTestGridSessions failed", error))?;
                sessions.extend(page.test_grid_sessions().iter().cloned());
            }
            Ok(sessions)
        }
        .boxed()
    }

    fn get_test_grid_session(
        &self,
        project_arn: &str,
        session_id: &str,
    ) -> BoxFuture<'_, Result<Option<TestGridSession>>> {
        let project_arn = project_arn.to_owned();
        let session_id = session_id.to_owned();
        async move {
            let output = self
                .client()
                .await?
                .get_test_grid_session()
                .project_arn(project_arn)
                .session_id(session_id)
                .send()
                .await
                .map_err(|error| aws_device_farm_error("GetTestGridSession failed", error))?;
            Ok(output.test_grid_session().cloned())
        }
        .boxed()
    }
}
