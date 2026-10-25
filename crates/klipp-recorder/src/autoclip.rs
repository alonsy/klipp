//! Turns game events into clips.
//!
//! Events close together (within the merge gap) form a *chain*. Once a chain
//! has been quiet for the merge gap, it becomes one clip if anything in it
//! qualifies: a headshot, enough kills for a multi-kill, and so on. Waiting for
//! the chain to finish is what keeps a 2K and the 3K that follows it from
//! becoming two overlapping clips. An event can hold its chain open longer
//! (`GameEvent::hold`), and a game whose events arrive late gets that much
//! longer before its chains close (`Rules::lateness`).
//!
//! Works only with `GameEvent`s, so every game integration shares it.

use std::collections::{BTreeMap, VecDeque};

use crate::clock::SECOND;
use crate::events::{EventKind, GameEvent};
use crate::settings::MultiKillMode;

/// Events are remembered this long so manual clips can show them as markers too.
const HISTORY: i64 = 10 * 60 * SECOND;
const MAX_HISTORY: usize = 1000;

/// Pre-roll and post-roll overrides.
pub type Rolls = (Option<i64>, Option<i64>);

#[derive(Clone, Debug, PartialEq)]
pub struct Rules {
    pub headshot: bool,
    /// Clip multi-kills of at least this many kills; 0 turns them off.
    pub multi_kill_min: u8,
    pub multi_kill_mode: MultiKillMode,
    pub any_kill: bool,
    pub knife_or_zeus_kill: bool,
    pub death: bool,
    pub mvp: bool,
    /// Fortnite's Victory Royale (FORTNITE TEST BUILD); false elsewhere.
    pub victory: bool,
    /// League's (false elsewhere): your first blood, a stolen Baron, Dragon
    /// or Herald, every objective you took part in, and team aces you were in.
    pub first_blood: bool,
    pub steal: bool,
    pub objective: bool,
    pub team_ace: bool,
    /// Rocket League's (false elsewhere); `assist` is League's too.
    pub goal: bool,
    pub assist: bool,
    pub save: bool,
    pub epic_save: bool,
    pub demolition: bool,
    pub pre_roll: i64,
    pub post_roll: i64,
    /// Events closer together than this join the same chain (and clip).
    pub merge_gap: i64,
    /// How late the game's events can reach us after they happened (League
    /// is polled): chains stay open this much longer, so a late event still
    /// joins the chain it belongs to. 0 for games that push events at once.
    pub lateness: i64,
    /// Per-event (pre, post) overrides, keyed by `EventKind::label_in()`; "multikill" covers every 2k+.
    pub timing: BTreeMap<String, Rolls>,
}

impl Rules {
    /// Every kill joins a chain (it may become part of a multi-kill); other
    /// events only when they're enabled. Round and match results are markers only.
    fn joins_chain(&self, kind: EventKind) -> bool {
        match kind {
            EventKind::Kill | EventKind::Headshot | EventKind::KnifeKill | EventKind::ZeusKill | EventKind::MultiKill(_) => true,
            EventKind::Death => self.death,
            EventKind::Mvp => self.mvp,
            EventKind::VictoryRoyale => self.victory,
            EventKind::FirstBlood => self.first_blood,
            EventKind::Steal => self.steal,
            EventKind::Objective => self.objective,
            EventKind::TeamAce => self.team_ace,
            EventKind::Goal => self.goal,
            EventKind::Assist => self.assist,
            EventKind::Save => self.save,
            EventKind::EpicSave => self.epic_save,
            EventKind::Demolition => self.demolition,
            EventKind::RoundWin
            | EventKind::RoundLoss
            | EventKind::MatchWin
            | EventKind::MatchLoss
            | EventKind::BombDefused
            | EventKind::Capture => false,
        }
    }

    fn timing(&self, game: &str, kind: EventKind) -> (i64, i64) {
        let specific = self.timing.get(&kind.label_in(game));
        let multi = matches!(kind, EventKind::MultiKill(_)).then(|| self.timing.get("multikill")).flatten();
        let pick = |f: fn(&Rolls) -> Option<i64>, default| {
            specific.and_then(f).or_else(|| multi.and_then(f)).unwrap_or(default)
        };
        (pick(|t| t.0, self.pre_roll), pick(|t| t.1, self.post_roll))
    }

    /// The best reason to clip a finished chain, if there is one.
    fn judge(&self, chain: &[GameEvent]) -> Option<EventKind> {
        let has = |k: EventKind| chain.iter().any(|e| e.kind == k);
        let kills = chain.iter().filter(|e| e.kind.is_kill()).count() as u8;
        let multi = match self.multi_kill_mode {
            MultiKillMode::Streak => kills,
            // The scoreboard count the game reported, even if earlier kills are outside this chain.
            MultiKillMode::Round => chain
                .iter()
                .filter_map(|e| match e.kind {
                    EventKind::MultiKill(n) => Some(n),
                    _ => None,
                })
                .max()
                .unwrap_or(kills.min(1)),
        };
        let mut reasons = Vec::new();
        if self.multi_kill_min >= 2 && multi >= self.multi_kill_min {
            reasons.push(EventKind::MultiKill(multi));
        }
        if self.headshot && has(EventKind::Headshot) {
            reasons.push(EventKind::Headshot);
        }
        if self.knife_or_zeus_kill && has(EventKind::KnifeKill) {
            reasons.push(EventKind::KnifeKill);
        }
        if self.knife_or_zeus_kill && has(EventKind::ZeusKill) {
            reasons.push(EventKind::ZeusKill);
        }
        if self.any_kill && kills > 0 {
            reasons.push(EventKind::Kill);
        }
        if self.death && has(EventKind::Death) {
            reasons.push(EventKind::Death);
        }
        if self.mvp && has(EventKind::Mvp) {
            reasons.push(EventKind::Mvp);
        }
        if self.victory && has(EventKind::VictoryRoyale) {
            reasons.push(EventKind::VictoryRoyale);
        }
        let switches = [
            (self.first_blood, EventKind::FirstBlood),
            (self.steal, EventKind::Steal),
            (self.objective, EventKind::Objective),
            (self.team_ace, EventKind::TeamAce),
            (self.goal, EventKind::Goal),
            (self.assist, EventKind::Assist),
            (self.save, EventKind::Save),
            (self.epic_save, EventKind::EpicSave),
            (self.demolition, EventKind::Demolition),
        ];
        reasons.extend(switches.into_iter().filter(|&(on, kind)| on && has(kind)).map(|(_, kind)| kind));
        reasons.into_iter().max_by_key(|k| k.rank())
    }
}

/// Reasons whose clip centres on the moment itself (its thumbnail) rather
/// than on the last kill: a Victory Royale's banner, a steal, a goal.
fn about_itself(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::VictoryRoyale
            | EventKind::FirstBlood
            | EventKind::Steal
            | EventKind::Objective
            | EventKind::TeamAce
            | EventKind::Goal
            | EventKind::Assist
            | EventKind::Save
            | EventKind::EpicSave
            | EventKind::Demolition
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipRequest {
    pub start: i64,
    pub end: i64,
    pub game: String,
    /// Why it was clipped; also names the file ("3k", "headshot").
    pub reason: EventKind,
    /// When the reason happened (the last kill of a multi-kill), for its marker.
    pub reason_time: i64,
    /// The chain's first event time, as in `pending`.
    pub key: i64,
}

struct Chain {
    game: String,
    events: Vec<GameEvent>,
}

impl Chain {
    fn first(&self) -> i64 {
        self.events.first().map_or(0, |e| e.time)
    }

    fn last(&self) -> i64 {
        self.events.last().map_or(0, |e| e.time)
    }

    /// Until when another event can still join: the merge gap after the last
    /// event, or later while an event holds the chain open.
    fn open_until(&self, merge_gap: i64) -> i64 {
        self.events.iter().map(|e| e.hold).fold(self.last() + merge_gap, i64::max)
    }
}

pub struct AutoClipper {
    rules: Rules,
    /// Games with their own rules (League of Legends, Rocket League); the rest use `rules`.
    game_rules: BTreeMap<String, Rules>,
    /// Chains still collecting events, oldest first.
    chains: Vec<Chain>,
    history: VecDeque<(String, GameEvent)>,
}

impl AutoClipper {
    pub fn new(rules: Rules) -> Self {
        Self { rules, game_rules: BTreeMap::new(), chains: Vec::new(), history: VecDeque::new() }
    }

    pub fn set_rules(&mut self, rules: Rules) {
        self.rules = rules;
    }

    pub fn set_game_rules(&mut self, game: &str, rules: Rules) {
        self.game_rules.insert(game.to_string(), rules);
    }

    fn rules_for(&self, game: &str) -> &Rules {
        self.game_rules.get(game).unwrap_or(&self.rules)
    }

    pub fn on_event(&mut self, game: &str, e: GameEvent) {
        while self.history.front().is_some_and(|(_, old)| e.time - old.time > HISTORY) || self.history.len() >= MAX_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back((game.to_string(), e.clone()));

        let rules = self.rules_for(game);
        if !rules.joins_chain(e.kind) {
            return;
        }
        let gap = rules.merge_gap;
        match self.chains.last_mut() {
            Some(chain) if chain.game == game && e.time <= chain.open_until(gap) => chain.events.push(e),
            _ => self.chains.push(Chain { game: game.to_string(), events: vec![e] }),
        }
    }

    /// Clips from chains that have been quiet for the merge gap (and their
    /// game's lateness). `now` should already allow for audio catching up.
    pub fn ready(&mut self, now: i64) -> Vec<ClipRequest> {
        let done = self
            .chains
            .iter()
            .take_while(|c| {
                let rules = self.rules_for(&c.game);
                now > c.open_until(rules.merge_gap) + rules.lateness
            })
            .count();
        let finished: Vec<Chain> = self.chains.drain(..done).collect();
        finished
            .into_iter()
            .filter_map(|chain| {
                let rules = self.rules_for(&chain.game);
                let reason = rules.judge(&chain.events)?;
                let (pre, post) = rules.timing(&chain.game, reason);
                let reason_time = chain
                    .events
                    .iter()
                    .rev()
                    .find(|e| if about_itself(reason) { e.kind == reason } else { e.kind.is_kill() })
                    .map_or(chain.last(), |e| e.time);
                let key = chain.first();
                Some(ClipRequest { start: key - pre, end: chain.last() + post, game: chain.game, reason, reason_time, key })
            })
            .collect()
    }

    /// Chains that will become a clip once they go quiet: (first event time, game, reason so far).
    pub fn pending(&self) -> Vec<(i64, String, EventKind)> {
        self.chains.iter().filter_map(|c| Some((c.first(), c.game.clone(), self.rules_for(&c.game).judge(&c.events)?))).collect()
    }

    /// Markers for a clip of `game` between `start` and `end`. In streak mode
    /// the game's per-round multi-kill counts are left out (they'd contradict
    /// the clip's own count); `reason` adds the clip's own multi-kill marker.
    pub fn markers(&self, game: &str, start: i64, end: i64, reason: Option<(EventKind, i64)>) -> Vec<GameEvent> {
        let streak = self.rules_for(game).multi_kill_mode == MultiKillMode::Streak;
        let mut out: Vec<GameEvent> = self
            .history
            .iter()
            .filter(|(g, e)| g == game && (start..=end).contains(&e.time))
            .filter(|(_, e)| !(streak && matches!(e.kind, EventKind::MultiKill(_))))
            .map(|(_, e)| e.clone())
            .collect();
        if let Some((kind @ EventKind::MultiKill(_), time)) = reason
            && streak
        {
            out.push(GameEvent::new(kind, time, ""));
            out.sort_by_key(|e| e.time);
        }
        out
    }
}
