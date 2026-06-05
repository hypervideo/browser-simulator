use aws_sdk_devicefarm::types::{
    TestGridSession,
    TestGridSessionStatus,
};
use client_simulator_config::{
    DEVICE_FARM_AWS_ACCESS_KEY_ID,
    DEVICE_FARM_AWS_SECRET_ACCESS_KEY,
};
use eyre::{
    bail,
    Context as _,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
};

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

    async fn client(&self) -> &aws_sdk_devicefarm::Client {
        let region = self.region.clone();
        self.client
            .get_or_init(|| async move {
                let region = aws_sdk_devicefarm::config::Region::new(region);
                let credentials = aws_sdk_devicefarm::config::Credentials::new(
                    DEVICE_FARM_AWS_ACCESS_KEY_ID,
                    DEVICE_FARM_AWS_SECRET_ACCESS_KEY,
                    None,
                    None,
                    "embedded-device-farm-env",
                );
                let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
                    .region(region)
                    .credentials_provider(credentials)
                    .load()
                    .await;
                aws_sdk_devicefarm::Client::new(&config)
            })
            .await
    }
}

impl TestGridApi for AwsTestGrid {
    fn create_test_grid_url(&self, project_arn: &str, expires_seconds: u64) -> BoxFuture<'_, Result<String>> {
        let project_arn = project_arn.to_owned();
        async move {
            let output = self
                .client()
                .await
                .create_test_grid_url()
                .project_arn(project_arn)
                .expires_in_seconds(expires_seconds as i32)
                .send()
                .await
                .context("CreateTestGridUrl failed")?;
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
            let mut builder = self.client().await.list_test_grid_sessions().project_arn(project_arn);
            if let Some(status) = status {
                builder = builder.status(status);
            }

            let mut pages = builder.into_paginator().send();
            let mut sessions = Vec::new();
            while let Some(page) = pages.next().await {
                let page = page.context("ListTestGridSessions failed")?;
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
                .await
                .get_test_grid_session()
                .project_arn(project_arn)
                .session_id(session_id)
                .send()
                .await
                .context("GetTestGridSession failed")?;
            Ok(output.test_grid_session().cloned())
        }
        .boxed()
    }
}
