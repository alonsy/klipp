//! Updates install by themselves (FB-35 for test builds; IDEA-43 for normal
//! releases too): testers kept running an old one for hours after "update
//! available". Once the update check has found a newer version, no game has
//! been in front for a few minutes and no clip is being saved, the recorder
//! asks the window's updater to install it without showing the window
//! (`klipp.exe --install-update`); the updater checks the download's
//! signature as for any update, and the installer logs what it did
//! (installlog). Every start checks a minute and a half in, so an update
//! found earlier installs right after Klipp (or Windows) restarts. "Install
//! updates by themselves" off in Settings → Advanced leaves it to the update card.

use std::time::{Duration, Instant};

use crate::channel;
use crate::games::DESKTOP;

/// No game in front for this long: not in a match, and not between rounds.
pub const QUIET_FOR: Duration = Duration::from_secs(180);
/// Asked once per version this often at most, in case an install didn't go through.
const AGAIN_AFTER: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub struct AutoInstall {
    asked: Option<(String, Instant)>,
}

impl AutoInstall {
    /// The version to install now, if it's time. `game`: what was in front
    /// within `QUIET_FOR` ("Desktop" for none); `saving`: a clip is on its way.
    pub fn due(&mut self, available: Option<String>, game: &str, saving: bool) -> Option<String> {
        let version = available?;
        let updates = channel::read();
        if !updates.auto_install || !updates.checks {
            return None;
        }
        self.decide(version, game, saving, Instant::now())
    }

    fn decide(&mut self, version: String, game: &str, saving: bool, now: Instant) -> Option<String> {
        if game != DESKTOP || saving {
            return None;
        }
        if self.asked.as_ref().is_some_and(|(v, at)| *v == version && now.duration_since(*at) < AGAIN_AFTER) {
            return None;
        }
        self.asked = Some((version.clone(), now));
        Some(version)
    }
}
