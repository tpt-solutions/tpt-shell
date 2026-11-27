// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! WebView2 release channels (spec §6.1): choosing the Stable, Beta, Dev or Canary runtime.
//!
//! The runtime search is controlled by the `WEBVIEW2_RELEASE_CHANNELS` environment variable, a
//! list of channel numbers: Stable 0, Beta 1, Dev 2, Canary 3. Microsoft's documentation gives
//! that variable and the `ReleaseChannels` policy as the mechanisms; it does not give the EdgeUpdate
//! client ids of the preview runtimes, so this module does not read the registry for them. It asks
//! the loader for the installed version instead, with the search restricted to one channel.
//!
//! The variable is read when the environment is created. [`restrict_to`] sets it for this process,
//! so an app that picks a channel must do that before its first window is made.
//!
//! Support: Partial. The selection is built and tested on Windows where the channel is installed.
//! Beta, Dev and Canary are not installed on the development machine, so their path is tested only
//! for the refusal.

use std::ffi::OsString;

use crate::loader::{LoadError, RuntimeLoader};

/// The environment variable the loader reads for the release channels it may use.
///
/// Support: Full.
pub const RELEASE_CHANNELS_VAR: &str = "WEBVIEW2_RELEASE_CHANNELS";

/// A WebView2 release channel.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
    Dev,
    Canary,
}

impl Channel {
    /// The channel's name, as the developer writes it.
    ///
    /// Support: Full.
    pub fn name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Dev => "dev",
            Channel::Canary => "canary",
        }
    }

    /// The channel with `name` (`stable`, `beta`, `dev` or `canary`, in any case).
    ///
    /// Support: Full.
    pub fn parse(name: &str) -> Option<Channel> {
        match name.to_ascii_lowercase().as_str() {
            "stable" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            "dev" => Some(Channel::Dev),
            "canary" => Some(Channel::Canary),
            _ => None,
        }
    }

    /// The value of [`RELEASE_CHANNELS_VAR`] that allows only this channel.
    fn release_channel_value(self) -> &'static str {
        match self {
            Channel::Stable => "0",
            Channel::Beta => "1",
            Channel::Dev => "2",
            Channel::Canary => "3",
        }
    }
}

/// Restricts this process's runtime search to `channel`. Returns the previous value of the
/// variable, so a caller that only checks can put it back.
///
/// Support: Full.
pub fn restrict_to(channel: Channel) -> Option<OsString> {
    let previous = std::env::var_os(RELEASE_CHANNELS_VAR);
    std::env::set_var(RELEASE_CHANNELS_VAR, channel.release_channel_value());
    previous
}

/// The installed version of `channel`'s runtime, or the loader's reason it is not there. The
/// environment is left as it was.
///
/// Support: Full on Windows.
pub fn installed_version<L: RuntimeLoader>(
    loader: &L,
    channel: Channel,
) -> Result<String, LoadError> {
    let previous = restrict_to(channel);
    let version = loader.runtime_version();
    match previous {
        Some(value) => std::env::set_var(RELEASE_CHANNELS_VAR, value),
        None => std::env::remove_var(RELEASE_CHANNELS_VAR),
    }
    version
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The tests change the process environment, so they run one at a time.
    static ENV: Mutex<()> = Mutex::new(());

    #[test]
    fn names_parse_in_any_case_and_unknown_names_are_refused() {
        assert_eq!(Channel::parse("Beta"), Some(Channel::Beta));
        assert_eq!(Channel::parse("CANARY"), Some(Channel::Canary));
        assert_eq!(Channel::parse("nightly"), None);
        for channel in [
            Channel::Stable,
            Channel::Beta,
            Channel::Dev,
            Channel::Canary,
        ] {
            assert_eq!(Channel::parse(channel.name()), Some(channel));
        }
    }

    #[test]
    fn restricting_sets_the_channel_number_and_returns_the_previous_value() {
        let _guard = ENV.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var(RELEASE_CHANNELS_VAR, "0,1");
        let previous = restrict_to(Channel::Canary);
        assert_eq!(previous, Some(OsString::from("0,1")));
        assert_eq!(std::env::var(RELEASE_CHANNELS_VAR).as_deref(), Ok("3"));
        std::env::remove_var(RELEASE_CHANNELS_VAR);
    }

    /// Stable is installed on the development machine, so its version is read. Other channels may
    /// be missing; the function then says why, and the environment is left as it was.
    #[test]
    fn the_installed_version_is_read_and_the_environment_is_restored() {
        let Some(path) = std::env::var_os("TPT_WEBVIEW2_LOADER") else {
            return;
        };
        let _guard = ENV.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var(RELEASE_CHANNELS_VAR, "0");
        let loader =
            crate::loader::OfficialLoader::load(std::path::Path::new(&path)).expect("the loader");
        let stable = installed_version(&loader, Channel::Stable);
        assert_eq!(std::env::var(RELEASE_CHANNELS_VAR).as_deref(), Ok("0"));
        for channel in [Channel::Beta, Channel::Dev, Channel::Canary] {
            let _ = installed_version(&loader, channel);
        }
        assert_eq!(std::env::var(RELEASE_CHANNELS_VAR).as_deref(), Ok("0"));
        std::env::remove_var(RELEASE_CHANNELS_VAR);
        assert!(
            stable.is_ok(),
            "the stable runtime is installed here: {stable:?}"
        );
    }
}
