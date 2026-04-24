/*! The core [`Game`] type and types for its fields. */

use super::*;

/// Configuration options of the game.
///
/// Note:
/// * [`Game::config`] **may** be mutated by user.
/// * It is not mutated by `Game` itself.
///
/// # Reproducibility
/// The game does not detect changes to its configuration.
/// It is therefore the user's responsibility to either not change configuration after the game has started,
/// or supply the information manually / externally.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Configuration<PceRot = StdPceRot> {
    /// How many pieces should be pre-generated and accessible/visible in the game state.
    #[cfg_attr(feature = "serde", serde(rename = "preview"))]
    pub generate_piece_preview: usize,

    /// Whether holding a 'rotate' button lets a piece be smoothly spawned in a rotated state,
    /// or holding the 'hold' button lets a piece be swapped immediately before it evens spawns.
    #[cfg_attr(feature = "serde", serde(rename = "initsys"))]
    pub allow_spawn_manipulation: bool,

    /// The method of tetromino rotation used.
    #[cfg_attr(feature = "serde", serde(rename = "rotsys"))]
    pub rotation_system: PceRot,

    /// How long the game should take to spawn a new piece.
    #[cfg_attr(feature = "serde", serde(rename = "are"))]
    pub spawn_delay: Duration,

    /// How long it takes for the active piece to start automatically shifting more to the side
    /// after the initial time a 'move' button has been pressed.
    #[cfg_attr(feature = "serde", serde(rename = "das"))]
    pub delayed_auto_shift: Duration,

    /// How long it takes for automatic side movement to repeat once it has started.
    #[cfg_attr(feature = "serde", serde(rename = "arr"))]
    pub auto_repeat_rate: Duration,

    /// Specification of how fall delay gets calculated from the rest of the state.
    #[cfg_attr(feature = "serde", serde(rename = "fallparams"))]
    pub fall_delay_params: DelayParameters,

    /// How many times faster than normal drop speed a piece should fall while 'soft drop' is being held.
    #[cfg_attr(feature = "serde", serde(rename = "sdf"))]
    pub soft_drop_factor: ExtNonNegF64,

    /// Specification of how fall delay gets calculated from the rest of the state.
    #[cfg_attr(feature = "serde", serde(rename = "lockparams"))]
    pub lock_delay_params: DelayParameters,

    /// Whether engine should try to ensure that delays for autonomous moves - which are determined by
    /// `delayed_auto_shift` and `auto_repeat_rate` - should be less than `lock_delay` runs out.
    /// This allows DAS and ARR to function at extreme game speeds.
    #[cfg_attr(feature = "serde", serde(rename = "sltl"))]
    pub ensure_shift_delay_lt_lock_delay: bool,

    /// Whether just pressing a rotation- or movement button is enough to refresh lock delay.
    /// Normally, lock delay only resets if rotation or movement actually succeeds.
    #[cfg_attr(feature = "serde", serde(rename = "llr"))]
    pub allow_lenient_lock_reset: bool,

    /// How long each spawned active piece may touch the ground in total until it should lock down
    /// immediately.
    #[cfg_attr(feature = "serde", serde(rename = "lcf"))]
    pub lock_reset_cap_factor: ExtNonNegF64,

    /// How long the game should take to clear a line.
    #[cfg_attr(feature = "serde", serde(rename = "lcd"))]
    pub line_clear_duration: Duration,

    /// When to update the fall and lock delays in [`State`].
    #[cfg_attr(feature = "serde", serde(rename = "update_every"))]
    pub update_delays_every_n_lineclears: u32,

    /// Stores the ways in which a round of the game should be limited.
    ///
    /// Each limitation may be either of positive ('game completed') or negative ('game over'), as
    /// designated by the `bool` stored with it.
    ///
    /// No limitations may allow for endless games.
    #[cfg_attr(feature = "serde", serde(rename = "limits"))]
    pub game_limits: GameLimits,

    /// The amount of feedback information that is to be generated.
    #[cfg_attr(feature = "serde", serde(rename = "notifs"))]
    pub send_notifications: bool,
}

/// Some values that were used to help initialize the game.
///
/// Note:
/// * [`Game::state_init`] cannot be mutated by user.
/// * It is not mutated by `Game` itself.
///
/// This struct is used for game reproducibility.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StateInitialization<TetGen = StdTetGen> {
    /// The value to seed the game's PRNG with.
    #[cfg_attr(feature = "serde", serde(rename = "seed"))]
    pub seed: u64,

    /// The method (and internal state) of tetromino generation used.
    #[cfg_attr(feature = "serde", serde(rename = "tetgen"))]
    pub tetromino_generator: TetGen,
}

/// Struct storing internal game state that changes over the course of play.
///
/// Note:
/// * [`Game::state`] cannot be mutated by the user.
/// * It is mutated by the `Game` itself.
#[derive(Eq, PartialEq, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State<TetGen = StdTetGen> {
    /// Current in-game time.
    pub time: InGameTime,
    /// The stores which buttons are considered active and since when.
    pub active_buttons: ButtonsState,
    /// The internal pseudo random number generator used.
    pub rng: GameRng,
    /// The method (and internal state) of tetromino generation used.
    pub piece_generator: TetGen,
    /// Upcoming pieces to be played.
    pub piece_preview: VecDeque<Tetromino>,
    /// Data about the piece being held. `true` denotes that the held piece can be swapped back in.
    pub piece_held: Option<(Tetromino, bool)>,
    /// The main playing grid storing empty (`None`) and filled, fixed tiles (`Some(nz_u32)`).
    pub board: Board,
    /// The current duration a piece takes to fall one unit.
    pub fall_delay: ExtDuration,
    /// The point (number of lines cleared) at which fall delay was updated to zero (possibly capped if formula yielded negative).
    pub fall_delay_lowerbound_hit_at_n_lineclears: Option<u32>,
    /// The current duration a piece takes to try and lock down.
    pub lock_delay: ExtDuration,
    /// Tallies of how many pieces of each type have been played so far.
    pub pieces_locked: [u32; Tetromino::VARIANTS.len()],
    /// The total number of lines that have been cleared.
    pub lineclears: u32,
    /// The number of consecutive pieces that have been locked and caused a line clear.
    pub consecutive_lineclears: u32,
    /// The current total points the player has scored in this game.
    pub points: u32,
}

/// An event that is scheduled by the game engine to execute some action.
///
/// Note:
/// * [`Game::phase`] cannot be mutated by user.
/// * It is mutated by the `Game` itself.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Phase {
    /// The state of the game "taking its time" to spawn a piece.
    /// This is the state the board will have right before attempting to spawn a new piece.
    Spawning {
        /// The in-game time at which the game moves on to the next `Phase.`
        spawn_time: InGameTime,
    },

    /// The state of the game having an active piece in-play, which can be controlled by a player.
    PieceInPlay {
        /// The tetromino game piece itself.
        piece: Piece,
        /// Optional time of the next move event.
        autoshift_scheduled: Option<InGameTime>,
        /// The time of the next fall or lock event.
        fall_or_lock_time: InGameTime,
        /// The time after which the active piece will immediately lock upon touching ground.
        lock_cap_time: InGameTime,
        /// The lowest recorded vertical position of the main piece.
        lowest_y: isize,
    },

    /// The state of the game "taking its time" to clear out lines.
    /// In this state the board is as it was at the time of the piece locking down,
    /// i.e. with some horizontally completed lines.
    /// After exiting this state, the
    ClearingLines {
        /// The in-game time at which the game moves on to the next `Phase.`
        clear_finish_time: InGameTime,
        /// The score bonus that will be earned once the lines are cleared out.
        point_bonus: u32,
    },

    /// The state of the game being irreversibly over, and not playable anymore.
    GameEnd {
        /// The cause of why the game ended.
        cause: GameEndCause,
        /// Whether the ending is considered a win.
        is_win: bool,
    },
}

/// Main game struct representing a round of play.
#[derive(Debug)]
pub struct Game<TetGen = StdTetGen, PceRot = StdPceRot> {
    /// Some internal configuration options of the `Game`.
    ///
    /// # Reproducibility
    /// The game does not detect changes to its configuration.
    /// It is therefore the user's responsibility to either not change configuration after the game has started,
    /// or supply the information manually / externally.
    pub config: Configuration<PceRot>,

    pub(crate) state_init: StateInitialization<TetGen>,

    pub(crate) state: State<TetGen>,

    pub(crate) phase: Phase,

    /// A list of special modifiers that apply to the `Game`.
    ///
    /// # Reproducibility
    /// The game does not detect changes to its modifiers.
    /// It is therefore the user's responsibility to either not change modifiers after the game has started,
    /// or supply the information manually / externally.
    pub modifiers: Vec<Box<dyn GameModifier<TetGen, PceRot>>>,
}

impl<PceRot: Default> Default for Configuration<PceRot> {
    fn default() -> Self {
        Self {
            generate_piece_preview: 4,
            allow_spawn_manipulation: true,
            rotation_system: PceRot::default(),
            spawn_delay: Duration::from_millis(50),
            delayed_auto_shift: Duration::from_millis(167),
            auto_repeat_rate: Duration::from_millis(33),
            fall_delay_params: DelayParameters::constant(Duration::from_millis(1000).into()),
            soft_drop_factor: ExtNonNegF64::new(15.0).unwrap(),
            lock_delay_params: DelayParameters::constant(Duration::from_millis(500).into()),
            allow_lenient_lock_reset: false,
            ensure_shift_delay_lt_lock_delay: false,
            lock_reset_cap_factor: ExtNonNegF64::new(8.0).unwrap(),
            line_clear_duration: Duration::from_millis(200),
            update_delays_every_n_lineclears: 10,
            game_limits: Default::default(),
            send_notifications: true,
        }
    }
}

impl Phase {
    /// Read accessor to a `Phase`'s possible [`Piece`].
    pub fn piece(&self) -> Option<&Piece> {
        if let Phase::PieceInPlay { piece, .. } = self {
            Some(piece)
        } else {
            None
        }
    }

    /// Mutable accessor to a `Phase`'s possible [`Piece`].
    pub fn piece_mut(&mut self) -> Option<&mut Piece> {
        if let Phase::PieceInPlay { piece, .. } = self {
            Some(piece)
        } else {
            None
        }
    }
}

impl<TetGen, PceRot> Game<TetGen, PceRot> {
    /// Creates a blank new template representing a yet-to-be-started [`Game`] ready for configuration.
    pub fn builder() -> GameBuilder<TetGen> {
        GameBuilder::default()
    }

    /// Read accessor for the game's initial values.
    pub const fn state_init(&self) -> &StateInitialization<TetGen> {
        &self.state_init
    }

    /// Read accessor for the current game state.
    pub const fn state(&self) -> &State<TetGen> {
        &self.state
    }

    /// Read accessor for the current game state.
    pub const fn phase(&self) -> &Phase {
        &self.phase
    }

    /// Whether the game has ended, and whether it can continue to update.
    pub const fn has_ended(&self) -> bool {
        matches!(self.phase, Phase::GameEnd { .. })
    }

    /// Check whether a certain stat value has been met or exceeded.
    pub fn check_stat_met(&self, stat: Stat) -> bool {
        match stat {
            Stat::TimeElapsed(t) => t <= self.state.time,
            Stat::PiecesLocked(p) => p <= self.state.pieces_locked.iter().sum(),
            Stat::LinesCleared(l) => l <= self.state.lineclears,
            Stat::PointsScored(s) => s <= self.state.points,
        }
    }
}

impl<TetGen: Clone, PceRot: Clone> Game<TetGen, PceRot> {
    /// Try to create a cloned instance of the game.
    pub fn try_clone(&self) -> Result<Self, String> {
        let mut modifiers = Vec::new();
        for modifier in self.modifiers.iter() {
            modifiers.push(modifier.try_clone()?);
        }

        Ok(Self {
            config: self.config.clone(),
            state_init: self.state_init.clone(),
            state: self.state.clone(),
            phase: self.phase.clone(),
            modifiers,
        })
    }
}
