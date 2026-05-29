use eyre::{
    Context as _,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
};

/// Minimal seam over the Device Farm Test Grid control plane so the session
/// logic can be tested without real AWS calls.
pub(crate) trait TestGridApi: Send + Sync {
    /// Create a short-lived Selenium Remote WebDriver URL for `project_arn`.
    fn create_test_grid_url(&self, project_arn: &str, expires_seconds: u64) -> BoxFuture<'_, Result<String>>;
}

/// Real implementation backed by `aws-sdk-devicefarm`.
pub(crate) struct AwsTestGrid {
    region: String,
    client: tokio::sync::OnceCell<aws_sdk_devicefarm::Client>,
}

impl AwsTestGrid {
    pub(crate) fn new(region: &str) -> Self {
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
                let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
                    .region(region)
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
}
