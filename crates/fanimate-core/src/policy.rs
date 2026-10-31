//! What the people who run Fanimate allow it to do.
//!
//! Fanimate works fully offline and uses no network or AI unless asked. A
//! school, a studio or a person who wants to be sure can go further with a
//! policy file: `fanimate-policy.json` beside the program (set by an
//! administrator, for every user of the computer) or `policy.json` in the
//! user's Fanimate folder. This module holds the rules; finding, reading and
//! parsing the files is the app's job (`fanimate-app`'s `policy.rs`), so the
//! rules are tested without a window or a disk.
//!
//! ```json
//! { "offline": true, "ai": false, "cloud": false }
//! ```

use serde::Deserialize;

/// What may be used. Every field defaults to the permissive value, so an
/// empty file, or no file, changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Policy {
    /// Never use the network: no update checks, no cloud services.
    pub offline: bool,
    /// Whether the AI tools may be used at all, local ones included.
    pub ai: bool,
    /// Whether cloud services (render, AI credits, sync) may be used. They
    /// need the network too, so `offline` turns them off whatever this says.
    pub cloud: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            offline: false,
            ai: true,
            cloud: true,
        }
    }
}

impl Policy {
    /// Everything off: what a policy that cannot be read counts as. If an
    /// administrator's file is damaged, the safe reading of it is the
    /// strictest one, and the app says so.
    pub const LOCKED: Policy = Policy {
        offline: true,
        ai: false,
        cloud: false,
    };

    /// The two policies together: whatever either one forbids stays forbidden.
    pub fn stricter(self, other: Self) -> Self {
        Self {
            offline: self.offline || other.offline,
            ai: self.ai && other.ai,
            cloud: self.cloud && other.cloud,
        }
    }

    /// The same policy with the user's own choice to work offline added.
    pub fn with_user_offline(self, offline: bool) -> Self {
        Self {
            offline: self.offline || offline,
            ..self
        }
    }

    /// Whether anything may use the network.
    pub fn network_allowed(&self) -> bool {
        !self.offline
    }

    /// Whether any AI tool may run.
    pub fn ai_allowed(&self) -> bool {
        self.ai
    }

    /// Whether a cloud service may be used.
    pub fn cloud_allowed(&self) -> bool {
        self.cloud && !self.offline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_policy_allows_everything() {
        let p = Policy::default();
        assert!(p.network_allowed() && p.ai_allowed() && p.cloud_allowed());
    }

    #[test]
    fn offline_turns_the_cloud_off_whatever_the_cloud_says() {
        let p = Policy {
            offline: true,
            cloud: true,
            ..Policy::default()
        };
        assert!(!p.network_allowed() && !p.cloud_allowed());
        assert!(p.ai_allowed(), "local AI needs no network");
    }

    #[test]
    fn the_locked_policy_allows_nothing() {
        assert!(!Policy::LOCKED.network_allowed());
        assert!(!Policy::LOCKED.ai_allowed());
        assert!(!Policy::LOCKED.cloud_allowed());
    }

    #[test]
    fn two_policies_together_keep_every_restriction() {
        let admin = Policy {
            ai: false,
            ..Policy::default()
        };
        let user = Policy {
            offline: true,
            ..Policy::default()
        };
        let both = admin.stricter(user);
        assert!(both.offline && !both.ai && both.cloud);
        assert_eq!(both, user.stricter(admin), "the order does not matter");
        assert_eq!(Policy::default().stricter(Policy::default()), Policy::default());
        // a user choice can add a restriction but never remove one
        assert!(Policy::default().with_user_offline(true).offline);
        assert!(Policy::LOCKED.with_user_offline(false).offline);
    }
}
