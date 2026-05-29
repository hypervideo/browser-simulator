use super::driver::BrowserDriver;
use client_simulator_config::{
    NoiseSuppression,
    WebcamResolution,
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
const CAMERA_RES_GET: &str = "return hyper.settings.videoCodec.videoResolutionForWebcamEncoder.name;";
const CAMERA_RES_SET: &str = "hyper.settings.videoCodec.actions.setVideoResolutionForWebcamEncoder(arguments[0]);";
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

pub(super) async fn get_outgoing_camera_resolution(driver: &dyn BrowserDriver) -> Result<WebcamResolution> {
    get_string(driver, CAMERA_RES_GET)
        .await?
        .parse::<WebcamResolution>()
        .context("failed to parse WebcamResolution from string")
}

pub(super) async fn set_outgoing_camera_resolution(driver: &dyn BrowserDriver, value: &WebcamResolution) -> Result<()> {
    set_value(driver, CAMERA_RES_SET, value.to_string()).await
}

pub(super) async fn get_force_webrtc(driver: &dyn BrowserDriver) -> Result<bool> {
    get_bool(driver, FORCE_WEBRTC_GET).await
}

pub(super) async fn set_force_webrtc(driver: &dyn BrowserDriver, value: bool) -> Result<()> {
    set_value(driver, FORCE_WEBRTC_SET, value).await
}
