// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The local endpoints of a managed kiosk (spec §10.1): the health check and the admin endpoint.
//! Each is compiled only with its cargo feature, and each listens on a loopback address only.

#[cfg(any(feature = "admin-endpoint", feature = "health-endpoint"))]
use std::net::SocketAddr;

#[cfg(any(feature = "admin-endpoint", feature = "health-endpoint"))]
use tpt_shell_core::Error;

#[cfg(any(feature = "admin-endpoint", feature = "health-endpoint"))]
use tpt_shell_core::Result;

/// Fails unless `addr` is a loopback address. Both endpoints refuse to listen anywhere else.
#[cfg(any(feature = "admin-endpoint", feature = "health-endpoint"))]
pub(crate) fn require_loopback(addr: SocketAddr, what: &str) -> Result<()> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(Error::Backend(format!(
            "the {what} must listen on a loopback address, not {addr}"
        )))
    }
}

/// Starts the health endpoint. It answers from the heartbeat file that the supervisor names in
/// `TPT_HEARTBEAT_FILE`, so that name must be set.
#[cfg(feature = "health-endpoint")]
pub(crate) fn health(
    addr: SocketAddr,
    max_age: std::time::Duration,
) -> Result<tpt_shell_kiosk::http::Server> {
    require_loopback(addr, "health endpoint")?;
    let heartbeat = tpt_shell_kiosk::heartbeat::from_env().ok_or_else(|| {
        Error::Backend(format!(
            "the health endpoint needs {} to name the heartbeat file",
            tpt_shell_kiosk::heartbeat::ENV
        ))
    })?;
    tpt_shell_kiosk::health::start(addr, heartbeat, max_age)
        .map_err(|e| Error::Backend(format!("cannot start the health endpoint: {e}")))
}

/// Starts the admin endpoint. `token` must be at least `MIN_TOKEN` characters.
#[cfg(feature = "admin-endpoint")]
pub(crate) fn admin(
    addr: SocketAddr,
    token: &str,
    status: std::sync::Arc<dyn Fn() -> String + Send + Sync>,
    reset: std::sync::Arc<dyn Fn() + Send + Sync>,
) -> Result<tpt_shell_kiosk::http::Server> {
    require_loopback(addr, "admin endpoint")?;
    let actions = tpt_shell_kiosk::admin::Actions { status, reset };
    tpt_shell_kiosk::admin::start(addr, token, actions)
        .map_err(|e| Error::Backend(format!("cannot start the admin endpoint: {e}")))
}
