//! Game events, independent of which game they came from.

/// What happened. Game integrations produce these; the auto-clip engine only
/// ever sees this type, so adding a game doesn't touch the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Kill,
    /// A kill that was a headshot (not also reported as `Kill`).
    Headshot,
    /// The player's n-th kill in the current round (n >= 2), as the scoreboard counts it.
    MultiKill(u8),
    /// Comes with the `Kill`/`Headshot` it describes.
    KnifeKill,
    /// Comes with the `Kill`/`Headshot` it describes.
    ZeusKill,
    Death,
    Mvp,
    /// Round results: timeline markers only, never a reason to clip.
    RoundWin,
    RoundLoss,
    /// Your team defused the bomb. A marker only.
    BombDefused,
    /// You pressed the save hotkey here (marks each press in a merged clip).
    Capture,
    /// Fortnite: you won the match.
    VictoryRoyale,
    /// Fortnite: you knocked someone down.
    Knock,
    /// League: the game's first kill was yours. Comes with the `Kill`.
    FirstBlood,
    /// League: you took a Baron, Dragon or Herald from the other team.
    Steal,
    /// League: an objective you took part in (`detail` says which: "baron",
    /// "dragon_elder", "turret"…). A marker unless a setting clips them.
    Objective,
    /// League's "Ace": your team killed the whole enemy team, and you were in it.
    TeamAce,
    /// You helped a kill (League) or a goal (Rocket League). A marker unless a setting clips them.
    Assist,
    /// The match's result: timeline markers only.
    MatchWin,
    MatchLoss,
    /// Rocket League: your goal (`detail` "overtime", "aerial"… when the game says so).
    Goal,
    Save,
    EpicSave,
    /// Rocket League: you demolished a car.
    Demolition,
}

impl EventKind {
    /// Short name used in settings, file names, clip info and the studio's icons:
    /// "headshot", "3k", "ace", "victory", …
    pub fn label(self) -> String {
        match self {
            EventKind::Kill => "kill".into(),
            EventKind::Headshot => "headshot".into(),
            EventKind::MultiKill(n) if n >= 5 => "ace".into(),
            EventKind::MultiKill(n) => format!("{n}k"),
            EventKind::KnifeKill => "knife".into(),
            EventKind::ZeusKill => "zeus".into(),
            EventKind::Death => "death".into(),
            EventKind::Mvp => "mvp".into(),
            EventKind::RoundWin => "victory".into(),
            EventKind::RoundLoss => "defeat".into(),
            EventKind::BombDefused => "defuse".into(),
            EventKind::Capture => "capture".into(),
            EventKind::VictoryRoyale => "victory".into(),
            EventKind::Knock => "knock".into(),
            EventKind::FirstBlood => "firstblood".into(),
            EventKind::Steal => "steal".into(),
            EventKind::Objective => "objective".into(),
            EventKind::TeamAce => "teamace".into(),
            EventKind::Assist => "assist".into(),
            EventKind::MatchWin => "win".into(),
            EventKind::MatchLoss => "loss".into(),
            EventKind::Goal => "goal".into(),
            EventKind::Save => "save".into(),
            EventKind::EpicSave => "epicsave".into(),
            EventKind::Demolition => "demolition".into(),
        }
    }

    /// Human-readable name, for MP4 chapters.
    pub fn title(self) -> String {
        match self {
            EventKind::Kill => "Kill".into(),
            EventKind::Headshot => "Headshot".into(),
            EventKind::MultiKill(n) if n >= 5 => "Ace".into(),
            EventKind::MultiKill(n) => format!("{n}K"),
            EventKind::KnifeKill => "Knife kill".into(),
            EventKind::ZeusKill => "Zeus kill".into(),
            EventKind::Death => "Death".into(),
            EventKind::Mvp => "MVP".into(),
            EventKind::RoundWin => "Round won".into(),
            EventKind::RoundLoss => "Round lost".into(),
            EventKind::BombDefused => "Bomb defused".into(),
            EventKind::Capture => "Saved here".into(),
            EventKind::VictoryRoyale => "Victory Royale".into(),
            EventKind::Knock => "Knock".into(),
            EventKind::FirstBlood => "First blood".into(),
            EventKind::Steal => "Steal".into(),
            EventKind::Objective => "Objective".into(),
            EventKind::TeamAce => "Team ace".into(),
            EventKind::Assist => "Assist".into(),
            EventKind::MatchWin => "Victory".into(),
            EventKind::MatchLoss => "Defeat".into(),
            EventKind::Goal => "Goal".into(),
            EventKind::Save => "Save".into(),
            EventKind::EpicSave => "Epic save".into(),
            EventKind::Demolition => "Demolition".into(),
        }
    }

    /// `label()` as players of `game` say it: League calls five kills a
    /// pentakill (its "Ace" is a team wipe, `TeamAce`). For file names, clip
    /// info and timing overrides.
    pub fn label_in(self, game: &str) -> String {
        match self {
            EventKind::MultiKill(n) if n >= 5 && game == crate::league::GAME_NAME => "penta".into(),
            _ => self.label(),
        }
    }

    /// `title()` as players of `game` say it, for chapters.
    pub fn title_in(self, game: &str) -> String {
        match self {
            EventKind::MultiKill(n) if n >= 5 && game == crate::league::GAME_NAME => "Pentakill".into(),
            _ => self.title(),
        }
    }

    /// Which event names a clip after itself when several are in it.
    pub fn rank(self) -> u8 {
        match self {
            EventKind::VictoryRoyale => 200,
            EventKind::MultiKill(n) => 100 + n,
            EventKind::Goal => 90,
            EventKind::EpicSave => 80,
            EventKind::TeamAce => 70,
            EventKind::Steal => 60,
            EventKind::Demolition => 55,
            EventKind::KnifeKill => 50,
            EventKind::ZeusKill => 49,
            EventKind::FirstBlood => 45,
            EventKind::Headshot => 40,
            EventKind::Save => 35,
            EventKind::Mvp => 30,
            EventKind::Kill => 20,
            EventKind::Knock => 15,
            EventKind::Assist => 15,
            EventKind::Death => 10,
            EventKind::Objective => 8,
            EventKind::BombDefused => 5,
            EventKind::RoundWin | EventKind::MatchWin => 4,
            EventKind::RoundLoss | EventKind::MatchLoss => 3,
            EventKind::Capture => 1,
        }
    }

    pub fn is_kill(self) -> bool {
        matches!(self, EventKind::Kill | EventKind::Headshot)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameEvent {
    pub kind: EventKind,
    /// When it happened, on the shared clock.
    pub time: i64,
    /// Extra detail, e.g. the weapon ("ak47").
    pub detail: String,
    /// The event's chain stays open until at least this time (shared clock;
    /// 0 = only the merge gap), for games that count streaks over longer
    /// windows: League still counts a penta 30 s after the quadra.
    pub hold: i64,
}

impl GameEvent {
    pub fn new(kind: EventKind, time: i64, detail: impl Into<String>) -> Self {
        Self { kind, time, detail: detail.into(), hold: 0 }
    }

    /// Keeps this event's chain open until `until`.
    pub fn holding(mut self, until: i64) -> Self {
        self.hold = until;
        self
    }
}

/// What a game is currently playing, for naming and grouping clips.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatchContext {
    pub map: String,
    pub mode: String,
    /// When this was last seen (shared clock); 0 = never.
    pub seen: i64,
}
