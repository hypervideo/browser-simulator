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
        BorrowedCredentials,
        FirstPartyCredentialsManager,
    },
    participant::shared::ResolvedFrontendKind,
};
use eyre::Result;

/// How to authenticate the frontend. Hyper Lite does not need Hyper Core credentials.
pub(in crate::participant) enum FrontendAuth {
    HyperCore {
        credentials: Option<BorrowedCredentials>,
        credentials_manager: FirstPartyCredentialsManager,
    },
    HyperLite,
}

impl FrontendAuth {
    pub(in crate::participant) fn for_kind(
        kind: ResolvedFrontendKind,
        credentials: Option<BorrowedCredentials>,
        credentials_manager: FirstPartyCredentialsManager,
    ) -> Self {
        match kind {
            ResolvedFrontendKind::HyperCore => Self::HyperCore {
                credentials,
                credentials_manager,
            },
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
            FrontendAuth::HyperCore {
                credentials,
                credentials_manager,
            } => {
                let credentials = if let Some(credentials) = credentials {
                    credentials
                } else {
                    credentials_manager
                        .fetch_new_credentials(context.launch_spec.base_url(), context.participant_name())
                        .await?
                };
                Ok(Box::new(ParticipantInner::new(context, credentials)))
            }
            FrontendAuth::HyperLite => Ok(Box::new(ParticipantInnerLite::new(context))),
        }
    }
}
