//! Native browser identity bootstrap, independent of process or target authority.

use super::{Connection, Error, Lane};
use crate::browser::preparation::{Channel, version_major};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::time::Instant;

/// Observed browser identity; the schema revision is a separate binding identity.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub protocol_version: String,
    pub product: String,
    pub revision: String,
    pub user_agent: String,
    pub js_version: String,
}

#[derive(Clone, Debug)]
pub struct Bootstrap {
    pub identity: Identity,
    pub schema_commit: &'static str,
}

impl Bootstrap {
    /// Verify an exact build selected by the launch owner, then observe target
    /// metadata. This neither adopts foreign targets nor certifies host support.
    pub async fn initialize(
        connection: &Connection,
        expected_version: &str,
        deadline: Instant,
    ) -> Result<Self, Error> {
        Self::initialize_channel(connection, expected_version, None, deadline).await
    }

    pub(in crate::browser) async fn initialize_channel(
        connection: &Connection,
        expected_version: &str,
        channel: Option<Channel>,
        deadline: Instant,
    ) -> Result<Self, Error> {
        if version_major(expected_version).is_none() {
            return Err(Error::Identity);
        }
        Self::verify(
            connection,
            |identity| identity.matches_channel(expected_version, channel),
            channel == Some(Channel::Msedge),
            channel.is_some(),
            deadline,
        )
        .await
    }

    pub(super) async fn initialize_attached(
        connection: &Connection,
        deadline: Instant,
    ) -> Result<Self, Error> {
        Self::verify(
            connection,
            |identity| {
                identity
                    .product
                    .split_once('/')
                    .is_some_and(|(product, version)| {
                        matches!(
                            product,
                            "Chrome" | "HeadlessChrome" | "Edg" | "Microsoft Edge"
                        ) && version_major(version)
                            .zip(version_major(crate::browser::preparation::CHROME_VERSION))
                            .is_some_and(|(observed, pinned)| observed >= pinned)
                    })
            },
            false,
            true,
            deadline,
        )
        .await
    }

    async fn verify(
        connection: &Connection,
        accepts: impl FnOnce(&Identity) -> bool,
        selected_edge: bool,
        external: bool,
        deadline: Instant,
    ) -> Result<Self, Error> {
        let result = connection
            .call(
                None,
                "Browser.getVersion",
                json!({}),
                Lane::Ordinary,
                deadline,
            )
            .await?;
        let identity: Identity = serde_json::from_value(result).map_err(|_| Error::Identity)?;
        if !accepts(&identity)
            || identity.protocol_version != "1.3"
            || identity.revision.is_empty()
            || identity.user_agent.is_empty()
            || identity.js_version.is_empty()
        {
            return Err(Error::Identity);
        }
        if selected_edge || identity.edge_version().is_some() {
            connection.accept_edge_target_metadata();
        }
        connection.set_keyboard_platform(&identity.user_agent);
        if external {
            connection.accept_additive_browser_fields();
        }
        connection
            .call(
                None,
                "Target.setDiscoverTargets",
                json!({"discover":true}),
                Lane::Ordinary,
                deadline,
            )
            .await?;
        Ok(Self {
            identity,
            schema_commit: super::schema::commit(),
        })
    }
}

impl Identity {
    fn edge_version(&self) -> Option<&str> {
        self.product
            .strip_prefix("Edg/")
            .or_else(|| self.product.strip_prefix("Microsoft Edge/"))
    }

    fn matches_channel(&self, expected: &str, channel: Option<Channel>) -> bool {
        let chromium = self
            .product
            .strip_prefix("Chrome/")
            .or_else(|| self.product.strip_prefix("HeadlessChrome/"));
        match channel {
            Some(Channel::Msedge) => {
                if let Some(branded) = self.edge_version() {
                    return branded == expected;
                }
                chromium.is_some_and(|version| version_major(version) == version_major(expected))
                    && self
                        .user_agent
                        .split_ascii_whitespace()
                        .any(|token| token.strip_prefix("Edg/") == Some(expected))
            }
            Some(Channel::Chrome) | None => chromium == Some(expected),
        }
    }
}
