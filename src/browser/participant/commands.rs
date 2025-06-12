use crate::config::{
    NoiseSuppression,
    WebcamResolution,
};
use chromiumoxide::{
    cdp::js_protocol::runtime::{
        CallArgument,
        CallFunctionOnParams,
    },
    js::Evaluation,
    Page,
};
use eyre::{
    Context as _,
    Result,
};

macro_rules! create_eval_getter {
    ($fn_name:ident, $js_function:expr, $result_type:ty, $convert_into:ty) => {
        pub async fn $fn_name(page: &Page) -> Result<$convert_into> {
            create_eval_getter!($fn_name, $js_function, $result_type);
            let value = $fn_name(page).await?;
            value.parse::<$convert_into>().context(format!(
                "failed to parse {} from string",
                stringify!($result_type)
            ))
        }
    };

    ($fn_name:ident, $js_function:expr, $result_type:ty) => {
        pub async fn $fn_name(page: &Page) -> Result<$result_type> {
            let function = CallFunctionOnParams::builder()
                .function_declaration($js_function)
                .build()
                .map_err(|e| eyre::eyre!("failed to build {} command: {e}", stringify!($fn_name)))?;

            let evaluation = Evaluation::Function(function);

            page.evaluate(evaluation)
                .await
                .context(format!("failed to evaluate {}", stringify!($fn_name)))
                .and_then(|e| {
                    e.into_value::<$result_type>().context(format!(
                        "failed to convert evaluation result to {}",
                        stringify!($result_type)
                    ))
                })
        }
    };
}

macro_rules! create_eval_setter {
    ($fn_name:ident, $js_function:expr) => {
        pub async fn $fn_name<S>(page: &Page, value: S) -> Result<()>
        where
            S: serde::Serialize,
        {
            let value = serde_json::to_value(value)
                .context(format!("failed to serialize value for {}", stringify!($fn_name)))?;
            let argument = CallArgument::builder().value(value).build();
            let function = CallFunctionOnParams::builder()
                .function_declaration($js_function)
                .arguments(vec![argument])
                .build()
                .map_err(|e| eyre::eyre!("failed to build {} command: {e}", stringify!($fn_name)))?;

            debug!("evaluating {function:?}");

            let evaluation = Evaluation::Function(function);

            page.evaluate(evaluation)
                .await
                .context(format!("failed to evaluate {}", stringify!($fn_name)))?;

            Ok(())
        }
    };
}

create_eval_getter!(
    get_noise_suppression,
    "function f() { return hyper.settings.media.noiseSuppression; }",
    String,
    NoiseSuppression
);

create_eval_setter!(
    set_noise_suppression,
    "function f(noiseSuppression) { hyper.settings.media.actions.setNoiseSuppression(noiseSuppression); }"
);

create_eval_getter!(
    get_background_blur,
    "function f() { return hyper.settings.media.backgroundBlur; }",
    bool
);

create_eval_setter!(
    set_background_blur,
    "function f(value) { return hyper.settings.media.actions.setBackgroundBlur(value); }"
);

create_eval_getter!(
    get_outgoing_camera_resolution,
    "function f() { return hyper.settings.videoCodec.videoResolutionForWebcamEncoder.level; }",
    String,
    WebcamResolution
);

create_eval_setter!(
    set_outgoing_camera_resolution,
    "function f(value) { return hyper.settings.videoCodec.actions.setVideoResolutionForWebcamEncoder(value); }"
);

create_eval_getter!(
    get_force_webrtc,
    "function f() { return hyper.settings.sessionDebug.forceWebrtc; }",
    bool
);

create_eval_setter!(
    set_force_webrtc,
    "function f(value) { return hyper.settings.sessionDebug.actions.setForceWebrtc(value); }"
);
