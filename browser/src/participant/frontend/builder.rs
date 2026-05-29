use super::{
    core::ParticipantInner,
    driver::{
        FrontendAutomation,
        FrontendContext,
    },
    lite::ParticipantInnerLite,
};
use crate::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
    },
    participant::shared::ResolvedFrontendKind,
};
use eyre::Result;

/// How to authenticate the frontend. HyperLite needs no cookie.
pub(in crate::participant) enum FrontendAuth {
    HyperCore {
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    },
    HyperLite,
}

impl FrontendAuth {
    pub(in crate::participant) fn for_kind(
        kind: ResolvedFrontendKind,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Self {
        match kind {
            ResolvedFrontendKind::HyperCore => Self::HyperCore { cookie, cookie_manager },
            ResolvedFrontendKind::HyperLite => Self::HyperLite,
        }
    }
}

/// Builds the concrete `FrontendAutomation` for a context + auth, shared by all backends.
pub(in crate::participant) struct FrontendKindBuilder;

impl FrontendKindBuilder {
    pub(in crate::participant) async fn build(
        context: FrontendContext,
        auth: FrontendAuth,
    ) -> Result<Box<dyn FrontendAutomation>> {
        match auth {
            FrontendAuth::HyperCore { cookie, cookie_manager } => {
                let cookie = if let Some(cookie) = cookie {
                    cookie
                } else {
                    cookie_manager
                        .fetch_new_cookie(context.launch_spec.base_url(), context.participant_name())
                        .await?
                };
                Ok(Box::new(ParticipantInner::new(context, cookie)))
            }
            FrontendAuth::HyperLite => Ok(Box::new(ParticipantInnerLite::new(context))),
        }
    }
}
