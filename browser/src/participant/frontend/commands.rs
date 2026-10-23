use super::driver::BrowserDriver;
use client_simulator_config::{
    NoiseSuppression,
    OwnOrDefault,
    TransportMode,
    VideoConstraint,
    VideoMaxConcurrentTracks,
};
use eyre::{
    eyre,
    Context as _,
    Result,
};
use serde::{
    de::DeserializeOwned,
    Deserialize,
    Serialize,
};
use serde_json::json;

// Getters return a value via `return <expr>;`. Setters mutate and ignore the result.
//
// `hyper.settings.client` holds hyper.video's report of the client's settings and edits the layers that the client
// owns. Getters read the value that applies from the report, which is null until the core first reports. Setters edit
// the participant layer, where a participant keeps the settings it chooses for itself. The SDK queues an edit made
// before its runtime starts and sends it once the runtime is up.
const SETTING_GET: &str = "return hyper.settings.client.report.effective[arguments[0]];";
const PARTICIPANT_EDIT: &str = "hyper.settings.client.edit('participant', arguments[0]);";
// Turns volume normalization on with the participant's own levels, else the default ones, as the web app's
// "Automatically adjust volume" switch does.
const VOLUME_NORMALIZATION_ON: &str = "const { report, edit } = hyper.settings.client; \
    const levels = report.participant.audioSendVolumeNormalization ?? report.default.audioSendVolumeNormalization; \
    edit('participant', { set: { audioSendVolumeNormalization: levels } });";

// The `SettingKey`s of the settings that the simulator reads and sets.
const NOISE_SUPPRESSION: &str = "audioSendNoiseSuppression";
const VOLUME_NORMALIZATION: &str = "audioSendVolumeNormalization";
const BACKGROUND_BLUR: &str = "videoSendWebcamBackgroundBlur";
const WEBCAM_MAX_RESOLUTION_LEVEL: &str = "videoSendWebcamMaxResolutionLevel";
const RECEIVE_MAX_RESOLUTION_LEVEL: &str = "videoReceiveMaxResolutionLevel";
const MAX_CONCURRENT_TRACKS: &str = "videoSendMaxConcurrentTracks";
const TRANSPORT: &str = "transport";

/// A value of hyper.video's `transport` setting, its `TransportPreference`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum TransportPreference {
    /// hyper.video's default, which connects over WebTransport where the browser has it, and over WebRTC elsewhere.
    Automatic,
    /// WebTransport only, with no fallback to WebRTC.
    WebTransport,
    /// WebRTC data channels.
    WebRtc,
}

impl From<TransportMode> for TransportPreference {
    fn from(mode: TransportMode) -> Self {
        match mode {
            TransportMode::WebTransport => Self::WebTransport,
            TransportMode::WebRTC => Self::WebRtc,
        }
    }
}

/// Reads the value of the setting `key` that applies.
async fn get_setting<T: DeserializeOwned>(driver: &dyn BrowserDriver, key: &str) -> Result<T> {
    let value = driver.eval(SETTING_GET, Some(json!(key))).await?;
    serde_json::from_value(value).with_context(|| format!("failed to read setting {key} from eval result"))
}

/// Sets the setting `key` to `value` in the participant layer.
async fn set_setting(driver: &dyn BrowserDriver, key: &str, value: impl Serialize) -> Result<()> {
    let value = serde_json::to_value(value).context("failed to serialize setting value")?;
    driver
        .eval(PARTICIPANT_EDIT, Some(json!({ "set": { key: value } })))
        .await?;
    Ok(())
}

/// Unsets the setting `key` in the participant layer, so that the layers below it decide, by default hyper.video's
/// default.
async fn unset_setting(driver: &dyn BrowserDriver, key: &str) -> Result<()> {
    driver.eval(PARTICIPANT_EDIT, Some(json!({ "unset": [key] }))).await?;
    Ok(())
}

/// Sets the setting `key` to the participant's own value, or unsets it for [`OwnOrDefault::Default`].
async fn edit_setting(driver: &dyn BrowserDriver, key: &str, value: OwnOrDefault<impl Serialize>) -> Result<()> {
    match value {
        OwnOrDefault::Own(value) => set_setting(driver, key, value).await,
        OwnOrDefault::Default => unset_setting(driver, key).await,
    }
}

async fn get_video_constraint(driver: &dyn BrowserDriver, key: &str) -> Result<VideoConstraint> {
    let level: Option<String> = get_setting(driver, key).await?;
    VideoConstraint::from_resolution_level(level.as_deref())
        .ok_or_else(|| eyre!("no video constraint for resolution level {level:?}"))
}

pub(super) async fn get_noise_suppression(driver: &dyn BrowserDriver) -> Result<NoiseSuppression> {
    get_setting(driver, NOISE_SUPPRESSION).await
}

pub(super) async fn set_noise_suppression(driver: &dyn BrowserDriver, value: NoiseSuppression) -> Result<()> {
    set_setting(driver, NOISE_SUPPRESSION, value).await
}

/// Whether volume normalization is on, which the web app's "Automatically adjust volume" switch turns on and off.
pub(super) async fn get_auto_gain_control(driver: &dyn BrowserDriver) -> Result<bool> {
    let levels: Option<serde_json::Value> = get_setting(driver, VOLUME_NORMALIZATION).await?;
    Ok(levels.is_some())
}

/// Turns volume normalization on or off, as [`get_auto_gain_control`] reads it, or unsets it for
/// [`OwnOrDefault::Default`].
pub(super) async fn set_auto_gain_control(driver: &dyn BrowserDriver, value: OwnOrDefault<bool>) -> Result<()> {
    match value {
        OwnOrDefault::Own(true) => {
            driver.eval(VOLUME_NORMALIZATION_ON, None).await?;
            Ok(())
        }
        OwnOrDefault::Own(false) => set_setting(driver, VOLUME_NORMALIZATION, serde_json::Value::Null).await,
        OwnOrDefault::Default => unset_setting(driver, VOLUME_NORMALIZATION).await,
    }
}

pub(super) async fn get_background_blur(driver: &dyn BrowserDriver) -> Result<bool> {
    get_setting(driver, BACKGROUND_BLUR).await
}

pub(super) async fn set_background_blur(driver: &dyn BrowserDriver, value: OwnOrDefault<bool>) -> Result<()> {
    edit_setting(driver, BACKGROUND_BLUR, value).await
}

pub(super) async fn get_video_constraint_publish_webcam(driver: &dyn BrowserDriver) -> Result<VideoConstraint> {
    get_video_constraint(driver, WEBCAM_MAX_RESOLUTION_LEVEL).await
}

pub(super) async fn set_video_constraint_publish_webcam(
    driver: &dyn BrowserDriver,
    value: OwnOrDefault<VideoConstraint>,
) -> Result<()> {
    edit_setting(
        driver,
        WEBCAM_MAX_RESOLUTION_LEVEL,
        value.map(VideoConstraint::resolution_level),
    )
    .await
}

pub(super) async fn get_video_constraint_subscribe(driver: &dyn BrowserDriver) -> Result<VideoConstraint> {
    get_video_constraint(driver, RECEIVE_MAX_RESOLUTION_LEVEL).await
}

pub(super) async fn set_video_constraint_subscribe(
    driver: &dyn BrowserDriver,
    value: OwnOrDefault<VideoConstraint>,
) -> Result<()> {
    edit_setting(
        driver,
        RECEIVE_MAX_RESOLUTION_LEVEL,
        value.map(VideoConstraint::resolution_level),
    )
    .await
}

pub(super) async fn get_video_max_concurrent_tracks(driver: &dyn BrowserDriver) -> Result<VideoMaxConcurrentTracks> {
    // Read as a nullable float to tolerate the page returning e.g. `2.0`, then cast.
    let as_f64: Option<f64> = get_setting(driver, MAX_CONCURRENT_TRACKS).await?;
    Ok(as_f64.map_or(VideoMaxConcurrentTracks::Unlimited, |value| {
        VideoMaxConcurrentTracks::Limit(value as usize)
    }))
}

pub(super) async fn set_video_max_concurrent_tracks(
    driver: &dyn BrowserDriver,
    value: OwnOrDefault<VideoMaxConcurrentTracks>,
) -> Result<()> {
    edit_setting(
        driver,
        MAX_CONCURRENT_TRACKS,
        value.map(VideoMaxConcurrentTracks::limit),
    )
    .await
}

/// The transport that applies, reading [`TransportPreference::Automatic`] as WebTransport, which it picks where the
/// browser has it.
pub(super) async fn get_transport(driver: &dyn BrowserDriver) -> Result<TransportMode> {
    let preference: TransportPreference = get_setting(driver, TRANSPORT).await?;
    Ok(match preference {
        TransportPreference::Automatic | TransportPreference::WebTransport => TransportMode::WebTransport,
        TransportPreference::WebRtc => TransportMode::WebRTC,
    })
}

/// Sets the participant's transport, or unsets it for [`OwnOrDefault::Default`], so that an admin's value for the
/// session applies, else [`TransportPreference::Automatic`].
pub(super) async fn set_transport(driver: &dyn BrowserDriver, value: OwnOrDefault<TransportMode>) -> Result<()> {
    edit_setting(driver, TRANSPORT, value.map(TransportPreference::from)).await
}

#[cfg(test)]
pub(in crate::participant::frontend) mod tests {
    use super::*;
    use client_simulator_config::VideoConstraint;
    use eyre::Result;
    use futures::{
        future::BoxFuture,
        FutureExt as _,
    };
    use std::{
        sync::Mutex,
        time::Duration,
    };

    #[derive(Default)]
    pub(in crate::participant) struct RecordingDriver {
        calls: Mutex<Vec<(String, Option<serde_json::Value>)>>,
        next_result: Mutex<serde_json::Value>,
    }

    impl RecordingDriver {
        pub(in crate::participant) fn with_result(value: serde_json::Value) -> Self {
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

        fn inject_first_party_credentials(&self, _realm: &str, _credentials: &str) -> BoxFuture<'_, Result<()>> {
            async { Ok(()) }.boxed()
        }
    }

    fn participant_edit(set: serde_json::Value) -> (String, Option<serde_json::Value>) {
        (PARTICIPANT_EDIT.to_string(), Some(json!({ "set": set })))
    }

    fn participant_unset(key: &str) -> (String, Option<serde_json::Value>) {
        (PARTICIPANT_EDIT.to_string(), Some(json!({ "unset": [key] })))
    }

    #[tokio::test]
    async fn sets_video_settings_in_participant_layer() {
        let driver = RecordingDriver::default();

        set_video_constraint_publish_webcam(&driver, OwnOrDefault::Own(VideoConstraint::P480))
            .await
            .unwrap();
        set_video_constraint_subscribe(&driver, OwnOrDefault::Own(VideoConstraint::None))
            .await
            .unwrap();
        set_video_constraint_subscribe(&driver, OwnOrDefault::Default)
            .await
            .unwrap();
        set_video_max_concurrent_tracks(&driver, OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(2)))
            .await
            .unwrap();
        set_video_max_concurrent_tracks(&driver, OwnOrDefault::Own(VideoMaxConcurrentTracks::Unlimited))
            .await
            .unwrap();
        set_video_max_concurrent_tracks(&driver, OwnOrDefault::Default)
            .await
            .unwrap();
        set_background_blur(&driver, OwnOrDefault::Own(true)).await.unwrap();
        set_background_blur(&driver, OwnOrDefault::Default).await.unwrap();

        assert_eq!(
            driver.calls(),
            vec![
                participant_edit(json!({ "videoSendWebcamMaxResolutionLevel": "P480" })),
                participant_edit(json!({ "videoReceiveMaxResolutionLevel": null })),
                participant_unset("videoReceiveMaxResolutionLevel"),
                participant_edit(json!({ "videoSendMaxConcurrentTracks": 2 })),
                participant_edit(json!({ "videoSendMaxConcurrentTracks": null })),
                participant_unset("videoSendMaxConcurrentTracks"),
                participant_edit(json!({ "videoSendWebcamBackgroundBlur": true })),
                participant_unset("videoSendWebcamBackgroundBlur"),
            ],
        );
    }

    #[tokio::test]
    async fn reads_video_constraints_from_settings_report() {
        let driver = RecordingDriver::with_result(json!("P360"));

        let value = get_video_constraint_publish_webcam(&driver).await.unwrap();

        assert_eq!(value, VideoConstraint::P360);
        assert_eq!(
            driver.calls(),
            vec![(
                SETTING_GET.to_string(),
                Some(json!("videoSendWebcamMaxResolutionLevel"))
            )],
        );

        let driver = RecordingDriver::with_result(serde_json::Value::Null);

        let value = get_video_constraint_subscribe(&driver).await.unwrap();

        assert_eq!(value, VideoConstraint::None);
        assert_eq!(
            driver.calls(),
            vec![(SETTING_GET.to_string(), Some(json!("videoReceiveMaxResolutionLevel")))],
        );
    }

    #[tokio::test]
    async fn auto_gain_control_switches_volume_normalization() {
        let driver = RecordingDriver::default();

        set_auto_gain_control(&driver, OwnOrDefault::Own(false)).await.unwrap();
        set_auto_gain_control(&driver, OwnOrDefault::Default).await.unwrap();

        assert_eq!(
            driver.calls(),
            vec![
                participant_edit(json!({ "audioSendVolumeNormalization": null })),
                participant_unset("audioSendVolumeNormalization"),
            ],
        );

        let levels = json!({ "targetDbfs": 0.0, "thresholdDbfs": -38.0 });
        assert!(get_auto_gain_control(&RecordingDriver::with_result(levels))
            .await
            .unwrap());
        assert!(
            !get_auto_gain_control(&RecordingDriver::with_result(serde_json::Value::Null))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn gets_nullable_video_max_concurrent_tracks() {
        let driver = RecordingDriver::with_result(serde_json::Value::Null);

        let value = get_video_max_concurrent_tracks(&driver).await.unwrap();

        assert_eq!(value, VideoMaxConcurrentTracks::Unlimited);
        assert_eq!(
            driver.calls(),
            vec![(SETTING_GET.to_string(), Some(json!("videoSendMaxConcurrentTracks")))],
        );
    }

    #[tokio::test]
    async fn reads_integer_valued_max_concurrent_tracks_even_as_float() {
        // The page may hand back a JSON number that serde sees as a float (e.g. 2.0).
        let driver = RecordingDriver::with_result(json!(2.0));

        let value = get_video_max_concurrent_tracks(&driver).await.unwrap();

        assert_eq!(value, VideoMaxConcurrentTracks::Limit(2));
    }

    #[tokio::test]
    async fn transport_maps_to_and_from_hyper_video_preferences() {
        let driver = RecordingDriver::default();

        set_transport(&driver, OwnOrDefault::Own(TransportMode::WebRTC))
            .await
            .unwrap();
        set_transport(&driver, OwnOrDefault::Own(TransportMode::WebTransport))
            .await
            .unwrap();
        set_transport(&driver, OwnOrDefault::Default).await.unwrap();

        assert_eq!(
            driver.calls(),
            vec![
                participant_edit(json!({ "transport": "webRtc" })),
                participant_edit(json!({ "transport": "webTransport" })),
                participant_unset("transport"),
            ],
        );

        let transports = [
            ("webRtc", TransportMode::WebRTC),
            ("webTransport", TransportMode::WebTransport),
            ("automatic", TransportMode::WebTransport),
        ];
        for (setting, expected) in transports {
            let driver = RecordingDriver::with_result(json!(setting));
            assert_eq!(get_transport(&driver).await.unwrap(), expected);
        }
        assert!(get_transport(&RecordingDriver::with_result(json!("webSocket")))
            .await
            .is_err());
    }
}
