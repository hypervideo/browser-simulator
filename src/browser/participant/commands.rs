use super::state::NoiseSuppression;
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

pub async fn set_noise_suppression_eval(page: &Page, noise_suppression: NoiseSuppression) -> Result<()> {
    let argument = CallArgument::builder().value(noise_suppression.to_string()).build();
    let function = CallFunctionOnParams::builder()
        .function_declaration("function f(noiseSuppression) { window.setNoiseSuppression(noiseSuppression); }")
        .arguments(vec![argument])
        .build()
        .map_err(|e| eyre::eyre!("failed to build setNoiseSuppression command: {e}"))?;

    let evaluation = Evaluation::Function(function);

    page.evaluate(evaluation)
        .await
        .context("failed to evaluate setNoiseSuppression")?;

    Ok(())
}

pub async fn get_noise_suppression_eval(page: &Page) -> Result<NoiseSuppression> {
    let function = CallFunctionOnParams::builder()
        .function_declaration("function f() { return window.getNoiseSuppression(); }")
        .build()
        .map_err(|e| eyre::eyre!("failed to build getNoiseSuppression command: {e}"))?;

    let evaluation = Evaluation::Function(function);

    page.evaluate(evaluation)
        .await
        .context("failed to evaluate getNoiseSuppression")
        .map(|e| {
            if let Ok(value) = e.into_value::<String>() {
                NoiseSuppression::from(value)
            } else {
                NoiseSuppression::default()
            }
        })
}

pub async fn get_background_blur_eval(page: &Page) -> Result<bool> {
    let function = CallFunctionOnParams::builder()
        .function_declaration("function f() { return window.getBackgroundBlur(); }")
        .build()
        .map_err(|e| eyre::eyre!("failed to build getBackgroundBlur command: {e}"))?;

    let evaluation = Evaluation::Function(function);

    page.evaluate(evaluation)
        .await
        .context("failed to evaluate getBackgroundBlur")
        .map(|e| e.into_value::<bool>().unwrap_or_default())
}
