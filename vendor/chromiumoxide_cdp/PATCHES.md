# Local patches

This is `chromiumoxide_cdp` 0.9.1 with one compatibility patch for Chrome 146.

- `src/cdp.rs`: `Network.ClientSecurityState.privateNetworkRequestPolicy` accepts `localNetworkAccessRequestPolicy` as a serde alias, matching the key emitted by current Chrome for `Network.requestWillBeSentExtraInfo`.
