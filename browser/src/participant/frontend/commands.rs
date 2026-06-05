use super::driver::BrowserDriver;
use client_simulator_config::{
    NoiseSuppression,
    VideoConstraint,
};
use eyre::{
    Context as _,
    Result,
};

// Getters return a value via `return <expr>;`. Setters mutate and ignore the result.
const NOISE_SUPPRESSION_GET: &str = "return hyper.settings.media.noiseSuppression;";
const NOISE_SUPPRESSION_SET: &str = "hyper.settings.media.actions.setNoiseSuppression(arguments[0]);";
const AUTO_GAIN_GET: &str = "return hyper.settings.media.autoGainControl;";
const AUTO_GAIN_SET: &str = "hyper.settings.media.actions.setAutoGainControl(arguments[0]);";
const BACKGROUND_BLUR_GET: &str = "return hyper.settings.media.backgroundBlur;";
const BACKGROUND_BLUR_SET: &str = "hyper.settings.media.actions.setBackgroundBlur(arguments[0]);";
const VIDEO_CONSTRAINT_PUBLISH_GET: &str = "return hyper.settings.media.videoConstraintPublishWebcam;";
const VIDEO_CONSTRAINT_PUBLISH_SET: &str =
    "hyper.settings.media.actions.setVideoConstraintPublishWebcam(arguments[0]);";
const VIDEO_CONSTRAINT_SUBSCRIBE_GET: &str = "return hyper.settings.media.videoConstraintSubscribe;";
const VIDEO_CONSTRAINT_SUBSCRIBE_SET: &str = "hyper.settings.media.actions.setVideoConstraintSubscribe(arguments[0]);";
const VIDEO_MAX_CONCURRENT_TRACKS_GET: &str = "return hyper.settings.media.videoMaxConcurrentTracks;";
const VIDEO_MAX_CONCURRENT_TRACKS_SET: &str = "hyper.settings.media.actions.setVideoMaxConcurrentTracks(arguments[0]);";
const FORCE_WEBRTC_GET: &str = "return hyper.settings.sessionDebug.forceWebrtc;";
const FORCE_WEBRTC_SET: &str = "hyper.settings.sessionDebug.actions.setForceWebrtc(arguments[0]);";

async fn get_string(driver: &dyn BrowserDriver, js: &str) -> Result<String> {
    let value = driver.eval(js, None).await?;
    serde_json::from_value(value).context("failed to read string from eval result")
}

async fn get_bool(driver: &dyn BrowserDriver, js: &str) -> Result<bool> {
    let value = driver.eval(js, None).await?;
    serde_json::from_value(value).context("failed to read bool from eval result")
}

async fn set_value<S: serde::Serialize>(driver: &dyn BrowserDriver, js: &str, value: S) -> Result<()> {
    let arg = serde_json::to_value(value).context("failed to serialize eval argument")?;
    driver.eval(js, Some(arg)).await?;
    Ok(())
}

pub(super) async fn get_noise_suppression(driver: &dyn BrowserDriver) -> Result<NoiseSuppression> {
    get_string(driver, NOISE_SUPPRESSION_GET)
        .await?
        .parse::<NoiseSuppression>()
        .context("failed to parse NoiseSuppression from string")
}

pub(super) async fn set_noise_suppression(driver: &dyn BrowserDriver, value: NoiseSuppression) -> Result<()> {
    set_value(driver, NOISE_SUPPRESSION_SET, value.to_string()).await
}

pub(super) async fn get_auto_gain_control(driver: &dyn BrowserDriver) -> Result<bool> {
    get_bool(driver, AUTO_GAIN_GET).await
}

pub(super) async fn set_auto_gain_control(driver: &dyn BrowserDriver, value: bool) -> Result<()> {
    set_value(driver, AUTO_GAIN_SET, value).await
}

pub(super) async fn get_background_blur(driver: &dyn BrowserDriver) -> Result<bool> {
    get_bool(driver, BACKGROUND_BLUR_GET).await
}

pub(super) async fn set_background_blur(driver: &dyn BrowserDriver, value: bool) -> Result<()> {
    set_value(driver, BACKGROUND_BLUR_SET, value).await
}

pub(super) async fn get_video_constraint_publish_webcam(driver: &dyn BrowserDriver) -> Result<VideoConstraint> {
    get_string(driver, VIDEO_CONSTRAINT_PUBLISH_GET)
        .await?
        .parse::<VideoConstraint>()
        .context("failed to parse videoConstraintPublishWebcam from string")
}

pub(super) async fn set_video_constraint_publish_webcam(
    driver: &dyn BrowserDriver,
    value: VideoConstraint,
) -> Result<()> {
    set_value(driver, VIDEO_CONSTRAINT_PUBLISH_SET, value.to_string()).await
}

pub(super) async fn get_video_constraint_subscribe(driver: &dyn BrowserDriver) -> Result<VideoConstraint> {
    get_string(driver, VIDEO_CONSTRAINT_SUBSCRIBE_GET)
        .await?
        .parse::<VideoConstraint>()
        .context("failed to parse videoConstraintSubscribe from string")
}

pub(super) async fn set_video_constraint_subscribe(driver: &dyn BrowserDriver, value: VideoConstraint) -> Result<()> {
    set_value(driver, VIDEO_CONSTRAINT_SUBSCRIBE_SET, value.to_string()).await
}

pub(super) async fn get_video_max_concurrent_tracks(driver: &dyn BrowserDriver) -> Result<Option<usize>> {
    // Read as a nullable float to tolerate the page returning e.g. `2.0`, then cast.
    let value = driver.eval(VIDEO_MAX_CONCURRENT_TRACKS_GET, None).await?;
    let as_f64: Option<f64> =
        serde_json::from_value(value).context("failed to read videoMaxConcurrentTracks from eval result")?;
    Ok(as_f64.map(|value| value as usize))
}

pub(super) async fn set_video_max_concurrent_tracks(driver: &dyn BrowserDriver, value: Option<usize>) -> Result<()> {
    set_value(driver, VIDEO_MAX_CONCURRENT_TRACKS_SET, value).await
}

pub(super) async fn get_force_webrtc(driver: &dyn BrowserDriver) -> Result<bool> {
    get_bool(driver, FORCE_WEBRTC_GET).await
}

pub(super) async fn set_force_webrtc(driver: &dyn BrowserDriver, value: bool) -> Result<()> {
    set_value(driver, FORCE_WEBRTC_SET, value).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_simulator_config::VideoConstraint;
    use eyre::Result;
    use futures::{
        future::BoxFuture,
        FutureExt as _,
    };
    use serde_json::json;
    use std::{
        sync::Mutex,
        time::Duration,
    };

    #[derive(Default)]
    struct RecordingDriver {
        calls: Mutex<Vec<(String, Option<serde_json::Value>)>>,
        next_result: Mutex<serde_json::Value>,
    }

    impl RecordingDriver {
        fn with_result(value: serde_json::Value) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                next_result: Mutex::new(value),
            }
        }

        fn calls(&self) -> Vec<(String, Option<serde_json::Value>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl BrowserDriver for RecordingDriver {
        fn goto(&self, _url: &str) -> BoxFuture<'_, Result<()>> {
            async { Ok(()) }.boxed()
        }

        fn exists(&self, _selector: &str) -> BoxFuture<'_, Result<bool>> {
            async { Ok(false) }.boxed()
        }

        fn wait_for(&self, _selector: &str, _timeout: Duration) -> BoxFuture<'_, Result<()>> {
            async { Ok(()) }.boxed()
        }

        fn click(&self, _selector: &str) -> BoxFuture<'_, Result<()>> {
            async { Ok(()) }.boxed()
        }

        fn fill(&self, _selector: &str, _text: &str) -> BoxFuture<'_, Result<()>> {
            async { Ok(()) }.boxed()
        }

        fn attribute(&self, _selector: &str, _name: &str) -> BoxFuture<'_, Result<Option<String>>> {
            async { Ok(None) }.boxed()
        }

        fn eval(&self, js_body: &str, arg: Option<serde_json::Value>) -> BoxFuture<'_, Result<serde_json::Value>> {
            self.calls.lock().unwrap().push((js_body.to_string(), arg));
            let value = self.next_result.lock().unwrap().clone();
            async move { Ok(value) }.boxed()
        }

        fn set_cookie(&self, _domain: &str, _name: &str, _value: &str) -> BoxFuture<'_, Result<()>> {
            async { Ok(()) }.boxed()
        }
    }

    #[tokio::test]
    async fn sets_video_constraints_through_media_settings_api() {
        let driver = RecordingDriver::default();

        set_video_constraint_publish_webcam(&driver, VideoConstraint::P480)
            .await
            .unwrap();
        set_video_constraint_subscribe(&driver, VideoConstraint::P720)
            .await
            .unwrap();
        set_video_max_concurrent_tracks(&driver, Some(2)).await.unwrap();
        set_video_max_concurrent_tracks(&driver, None).await.unwrap();

        assert_eq!(
            driver.calls(),
            vec![
                (
                    "hyper.settings.media.actions.setVideoConstraintPublishWebcam(arguments[0]);".to_string(),
                    Some(json!("480p")),
                ),
                (
                    "hyper.settings.media.actions.setVideoConstraintSubscribe(arguments[0]);".to_string(),
                    Some(json!("720p")),
                ),
                (
                    "hyper.settings.media.actions.setVideoMaxConcurrentTracks(arguments[0]);".to_string(),
                    Some(json!(2)),
                ),
                (
                    "hyper.settings.media.actions.setVideoMaxConcurrentTracks(arguments[0]);".to_string(),
                    Some(serde_json::Value::Null),
                ),
            ],
        );
    }

    #[tokio::test]
    async fn gets_video_constraints_from_media_settings_api() {
        let driver = RecordingDriver::with_result(json!("360p"));

        let value = get_video_constraint_publish_webcam(&driver).await.unwrap();

        assert_eq!(value, VideoConstraint::P360);
        assert_eq!(
            driver.calls(),
            vec![(
                "return hyper.settings.media.videoConstraintPublishWebcam;".to_string(),
                None,
            )],
        );
    }

    #[tokio::test]
    async fn gets_nullable_video_max_concurrent_tracks() {
        let driver = RecordingDriver::with_result(serde_json::Value::Null);

        let value = get_video_max_concurrent_tracks(&driver).await.unwrap();

        assert_eq!(value, None);
        assert_eq!(
            driver.calls(),
            vec![(
                "return hyper.settings.media.videoMaxConcurrentTracks;".to_string(),
                None,
            )],
        );
    }

    #[tokio::test]
    async fn reads_integer_valued_max_concurrent_tracks_even_as_float() {
        // The page may hand back a JSON number that serde sees as a float (e.g. 2.0).
        let driver = RecordingDriver::with_result(json!(2.0));

        let value = get_video_max_concurrent_tracks(&driver).await.unwrap();

        assert_eq!(value, Some(2));
    }
}
