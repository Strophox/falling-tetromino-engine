/*! The core [`Game`] type and types for its fields. */

use super::*;

use either::Either;
use rand_chacha::ChaCha8Rng;

use std::{collections::VecDeque, fmt, num::NonZeroU8, ops, time::Duration};

/// The maximum height *any* piece tile could reach *before* `GameOver::LockOut` occurs.
pub const HEIGHT: usize = 32;
/// The game field width.
pub const WIDTH: usize = 10;
/// The height of the (conventionally) visible playing grid that can be played in.
/// No tile piece may have all its tiles locked entirely at or above this index height (see [`GameEndCause::LockOut`]), although it may do so partially.
pub const LOCK_OUT_HEIGHT: usize = 20;

/// Abstract identifier for which type of tile occupies a cell in the grid.
pub type TileID = NonZeroU8;
/// The type of horizontal lines of the playing grid.
pub type Line = [Option<TileID>; WIDTH];
// NOTE: Would've liked to use `impl Game { type Board = ...` (https://github.com/rust-lang/rust/issues/8995)
/// The type of the entire two-dimensional playing grid.
pub type Board = [Line; HEIGHT];
/// Coordinates conventionally used to index into the [`Board`], starting in the bottom left.
pub type Coordinate = (isize, isize);
/// Coordinate offsets that can be [`CoordAdd::add`]ed to [`Coordinate`]s.
pub type Offset = (isize, isize);
/// Type describing the state that is stored about buttons.
///
/// Specifically, it stores which buttons are considered active, and if yes, since when.
pub type ButtonsState = [Option<InGameTime>; Button::VARIANTS.len()];
/// The type used to identify points in time in a game's internal timeline.
pub type InGameTime = Duration;
/// The internal RNG used by a game.
pub type GameRng = ChaCha8Rng;
/// The type used to store fall or luck curves.
pub type SoftDropRate = Either<ExtNonNegF64, ExtDuration>;
/// The type used to store fall or luck curves.
pub type DelayCurve = Either<DelayParameters, DelayTable>;
/// Type alias for a stream of notifications with timestamps.
pub type NotificationFeed = Vec<(Notification, InGameTime)>;

/// Represents one of the seven "Tetrominos";
///
/// A *tetromino* is a two-dimensional, geometric shape made by
/// connecting four squares (orthogonally / at along the edges).
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Tetromino {
    /// 'O'-Tetromino.
    /// Four squares connected as one big square; `⠶`, `██`.
    ///
    /// 'O' has 90° rotational symmetry + 2 axes of mirror symmetry.
    O = 0,
    /// 'I'-Tetromino.
    /// Four squares connected as one straight line; `⡇`, `▄▄▄▄`.
    ///
    /// 'I' has 180° rotational symmetry + 2 axes of mirror symmetry.
    I = 1,
    /// 'S'-Tetromino.
    /// Four squares connected in an 'S'-snaking manner; `⠳`, `▄█▀`.
    ///
    /// 'S' has 180° rotational symmetry + 0 axes of mirror symmetry.
    S = 2,
    /// 'Z'-Tetromino:
    /// Four squares connected in a 'Z'-snaking manner; `⠞`, `▀█▄`.
    ///
    /// 'Z' has 180° rotational symmetry + 0 axes of mirror symmetry.
    Z = 3,
    /// 'T'-Tetromino:
    /// Four squares connected in a 'T'-junction shape; `⠗`, `▄█▄`.
    ///
    /// 'T' has 360° rotational symmetry + 1 axis of mirror symmetry.
    T = 4,
    /// 'L'-Tetromino:
    /// Four squares connected in an 'L'-shape; `⠧`, `▄▄█`.
    ///
    /// 'L' has 360° rotational symmetry + 0 axes of mirror symmetry.
    L = 5,
    /// 'J'-Tetromino:
    /// Four squares connected in a 'J'-shape; `⠼`, `█▄▄`.
    ///
    /// 'J' has 360° rotational symmetry + 0 axes of mirror symmetry.
    J = 6,
}

/// Represents the orientation an active piece can be in.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Orientation {
    /// North.
    N = 0,
    /// East.
    E,
    /// South.
    S,
    /// West.
    W,
}

/// An active tetromino in play.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Piece {
    /// Type of tetromino the active piece is.
    #[cfg_attr(feature = "serde", serde(rename = "tet"))]
    pub tetromino: Tetromino,

    /// In which way the tetromino is re-oriented.
    #[cfg_attr(feature = "serde", serde(rename = "orn"))]
    pub orientation: Orientation,

    /// The position of the active piece on a playing grid.
    #[cfg_attr(feature = "serde", serde(rename = "pos"))]
    pub position: Coordinate,
}

/// A struct describing how certain time 'delay' values procedurally progress during a game's lifetime.
///
/// # Example
/// The formulation used for calculation of fall delay is conceptually:
/// ```ignore
/// let fall_delay = |lineclears| {
///     initial_fall_delay.mul_ennf64(
///         multiplier.get().powf(lineclears) - subtrahend.get() * lineclears
///     )
/// }
/// ```
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DelayParameters {
    /// The duration at which the delay starts.
    #[cfg_attr(feature = "serde", serde(rename = "base"))]
    base_delay: ExtDuration,

    /// The base factor that gets exponentiated by number of line clears;
    /// `factor ^ lineclears ...`.
    ///
    /// Should be in the range `0.0 ≤ .. ≤ 1.0`, where
    /// - `0.0` means 'zero-out initial delay at every line clear',
    /// - `0.5` means 'halve initial delay for every line clear',
    /// - `1.0` means 'keep initial delay at 100%'.
    #[cfg_attr(feature = "serde", serde(rename = "mul"))]
    factor: ExtNonNegF64,

    /// The base subtrahend that gets multiplied by number of line clears;
    /// `... - subtrahend * lineclears`.
    ///
    /// Should be in the range `0.0 ≤ .. ≤ 1.0`, where
    /// - `0.0` means 'subtract 0% of initial delay for every line clear',
    /// - `0.5` means 'subtract 50% of initial delay for every line clear',
    /// - `1.0` means 'subtract 100% of initial delay for every line clear'.
    #[cfg_attr(feature = "serde", serde(rename = "sub"))]
    subtrahend: ExtDuration,

    /// The duration below which delay cannot decrease.
    #[cfg_attr(feature = "serde", serde(rename = "lower"))]
    lowerbound: ExtDuration,
}

/// A struct describing how certain time 'delay' values progress based on a hardcoded table.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DelayTable {
    /// The entries of the table.
    #[cfg_attr(feature = "serde", serde(rename = "entries"))]
    entries: Vec<ExtDuration>,
}

/// Certain statistics for which an instance of [`Game`] can be checked against.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GameLimits {
    /// A given amount of total time that can elapse in-game.
    #[cfg_attr(feature = "serde", serde(rename = "time"))]
    pub time_elapsed: Option<(InGameTime, bool)>,

    /// A given number of [`Tetromino`]s that can be locked/placed on the game's [`Board`].
    #[cfg_attr(feature = "serde", serde(rename = "pieces"))]
    pub pieces_locked: Option<(u32, bool)>,

    /// A given number of lines that can be cleared from the [`Board`].
    #[cfg_attr(feature = "serde", serde(rename = "lines"))]
    pub lines_cleared: Option<(u32, bool)>,

    /// A given number of points that can be scored.
    #[cfg_attr(feature = "serde", serde(rename = "points"))]
    pub points_scored: Option<(u32, bool)>,
}

/// Certain statistics for which an instance of [`Game`] can be checked against.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Stat {
    /// A given amount of total time that elapsed in-game.
    TimeElapsed(InGameTime),
    /// A given number of [`Tetromino`]s that have been locked/placed on the game's [`Board`].
    PiecesLocked(u32),
    /// A given number of lines that have been cleared from the [`Board`].
    LinesCleared(u32),
    /// A given number of points that have been scored.
    PointsScored(u32),
}

/// Represents an abstract game input.
// NOTE: We could consider calling this `Action` judging from its variants, however the Game stores a mapping of whether a given `Button` is active over a period of time. `Intents` could work but `Button` is less abstract and often corresponds directly to IRL player inputs.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Button {
    /// Moves the piece once to the left.
    MoveLeft = 0,
    /// Moves the piece once to the right.
    MoveRight,
    /// Rotate the piece by +90° (clockwise).
    RotateLeft,
    /// Rotate the piece by -90° (counter-clockwise).
    RotateRight,
    /// Rotate the piece by 180° (flip around).
    Rotate180,
    /// "Soft" dropping.
    /// This drops a piece down by one, locking it immediately if it hit a surface,
    /// Otherwise holding this button decreases fall speed by the game [`Configuration`]'s `soft_drop_factor`.
    DropSoft,
    /// "Hard" dropping.
    /// This immediately drops a piece all the way down until it hits a surface,
    /// locking it there (almost) instantly, too.
    DropHard,
    /// Teleport the piece down, also known as "Sonic" dropping.
    /// This immediately drops a piece all the way down until it hits a surface,
    /// but without locking it (unlike [`Button::DropHard`]).
    TeleDown,
    /// Instantly 'teleports' (moves) a piece left until it hits a surface.
    TeleLeft,
    /// Instantly 'teleports' (moves) a piece right until it hits a surface.
    TeleRight,
    /// Holding the current piece; and swapping in a new piece if one was held previously.
    HoldPiece,
}

/// A signal about button activation or deactivation.
///
/// `Activate` generally corresponds to a 'press down' input, whereas `Deactivate` is 'release'.
///
/// Note however that a button is allowed to be activated several times in sequence with only a single deactivation.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Input {
    /// The signal of a button now being activated.
    Activate(Button),
    /// The signal of a button now being deactivated.
    Deactivate(Button),
}

/// Represents how a game can end.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GameEndCause {
    /// 'Lock out' denotes the most recent piece would be completely locked down at
    /// or above [`LOCK_OUT_HEIGHT`].
    LockOut {
        /// The offending piece that does not fit below [`LOCK_OUT_HEIGHT`].
        locking_piece: Piece,
    },

    /// 'Block out' denotes a new piece being unable to spawn due to existing board tile(s)
    /// blocking one or several of the cells of a piece to be spawned.
    BlockOut {
        /// The offending piece that does not fit onto board.
        blocked_piece: Piece,
    },

    // 'Buffer out' denotes a number of new lines being unable to enter the existing board.
    /// This is currently unused in the base engine.
    BufferOut {
        /// The lines that got pushed out and did not fit on the board anymore.
        overflowing_lines: Vec<Line>,
    },

    /// Game over by having reached a [`Stat`] limit.
    Limit(Stat),

    /// Game ended by player forfeit.
    Forfeit {
        /// Piece that was in play at time of forfeit.
        piece_in_play: Option<Piece>,
    },

    /// Custom game over.
    /// This is unused in the base engine and intended for modding.
    Custom(String),
}

/// A number of feedback notifications that can be returned by the game.
///
/// These can be used to more easily render visual feedback to the player.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Notification {
    /// A piece was quickly dropped from its original position to a new one.
    HardDrop {
        /// Information about the old state of the hard-dropped piece.
        height_dropped: usize,
        /// Information about the new state of the hard-dropped piece.
        dropped_piece: Piece,
    },

    /// A piece was locked down in a certain configuration.
    PieceLocked {
        /// Information about the [`Piece`] that was locked.
        piece: Piece,
    },

    /// A number of lines were cleared.
    ///
    /// The duration indicates the line clear delay the game was configured with at the time.
    LinesClearing {
        /// A list of height coordinates/indices and the lines themselves that were cleared.
        lines: Vec<(usize, [TileID; WIDTH])>,
        /// Game time where lines started clearing.
        /// Starts simultaneously to when a piece was locked and successfully completed some horizontal [`Line`]s,
        /// therefore this will coincide with the time same value in a nearby [`Notification::PieceLocked`].
        line_clear_duration: InGameTime,
    },

    /// The player cleared some lines with a number of other stats that might have increased their
    /// points bonus.
    Accolade {
        /// The final computed score bonus caused by the action.
        point_bonus: u32,
        /// How many lines were cleared by the piece simultaneously
        lineclears: u32,
        /// The number of consecutive pieces played that caused a lineclear.
        combo: u32,
        /// Whether the piece was spun into place.
        is_spin: bool,
        /// Whether the entire board was cleared empty by this action.
        is_perfect: bool,
        /// The tetromino type that was locked.
        tetromino: Tetromino,
    },

    /// Message that the game has ended.
    GameEnded {
        /// Why the game ended.
        cause: GameEndCause,
        /// Whether it was a win or a loss.
        is_win: bool,
    },

    /// Generic text feedback message.
    ///
    /// This is currently unused in the base engine.
    Custom(String),
}

/// An error that can be thrown by [`Game::update`].
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
pub enum UpdateGameError {
    /// Error variant caused by an attempt to update the game with a requested `update_time` that lies in
    /// the game's past (` < game.state().time`).
    TargetTimeInPast,

    /// Error variant caused by an attempt to update a game that has ended (`game.ended() == true`).
    AlreadyEnded,
}

/// Trait to enable adding 2D coordinates together.
pub trait CoordAdd {
    /// Adds an offset to a coordinate, wrapping on overflow.
    fn add(self, offset: Offset) -> Coordinate;
}

impl CoordAdd for Coordinate {
    fn add(self, (dx, dy): Offset) -> Coordinate {
        let (x, y) = self;
        (x.wrapping_add(dx), y.wrapping_add(dy))
    }
}

/*#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let res = add((1,2),(3,4));
        assert_eq!(res, (4,6));
    }
}*/

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

    /// Optionally add a delay (lowerbound) to the first soft drop before continuing speed up fallng.
    /// This is analogous to DAS (delayed auto shift) seen in sideways movement.
    #[cfg_attr(feature = "serde", serde(rename = "dsd"))]
    pub delayed_soft_drop: Option<Duration>,

    /// How soft drop should speed up the falling of a piece should speed up while [`Button::DropSoft`] is held, either:
    /// - A factor by which to speed up current gravity ('Soft Drop Factor').
    /// - An upper bound to how short the current fall delay should be ('Auto Drop Rate').
    #[cfg_attr(feature = "serde", serde(rename = "sdr"))]
    pub soft_drop_rate: SoftDropRate,

    /// Specification of how fall delay gets calculated from the rest of the state.
    /// - One variant describes *parameters* from which to calculate the fall delay;
    /// - The other variant describes a *table* with hardcoded step intervals.
    #[cfg_attr(feature = "serde", serde(rename = "fall_curve"))]
    pub fall_delay_curve: DelayCurve,

    /// Specification of how fall delay gets calculated from the rest of the state.
    /// If `None`, lock delay equals fall delay.
    /// Otherwise there are two variants, analogous to [`Configuration::fall_delay_curve`].
    #[cfg_attr(feature = "serde", serde(rename = "lock_curve"))]
    pub lock_delay_curve: Option<DelayCurve>,

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

impl<PceRot: Default> Default for Configuration<PceRot> {
    fn default() -> Self {
        Self {
            generate_piece_preview: 4,
            allow_spawn_manipulation: true,
            rotation_system: PceRot::default(),
            spawn_delay: Duration::from_millis(50),
            delayed_auto_shift: Duration::from_millis(167),
            auto_repeat_rate: Duration::from_millis(33),
            delayed_soft_drop: Some(Duration::from_millis(100)),
            soft_drop_rate: Either::Right(Duration::from_millis(33).into()),
            fall_delay_curve: Either::Left(DelayParameters::constant(
                Duration::from_millis(1000).into(),
            )),
            lock_delay_curve: Some(Either::Left(DelayParameters::constant(
                Duration::from_millis(500).into(),
            ))),
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

impl Tetromino {
    /// All `Tetromino` enum variants in order.
    ///
    /// Note that `Tetromino::VARIANTS[t as usize] == t` always holds.
    pub const VARIANTS: [Self; 7] = {
        use Tetromino::*;
        [O, I, S, Z, T, L, J]
    };

    /// Returns the mino offsets of a tetromino shape, given an orientation.
    ///
    /// The order of the minos is guaranteed to be in a particular order:
    /// - Ordered by x coordinate, ascending.
    /// - Then ordered by y coordinate, ascending.
    pub const fn minos(self, oriented: Orientation) -> [Coordinate; 4] {
        use Orientation::*;
        match self {
            Tetromino::O => [(0, 0), (0, 1), (1, 0), (1, 1)], // ⠶
            Tetromino::I => match oriented {
                N | S => [(0, 0), (1, 0), (2, 0), (3, 0)], // ⠤⠤
                E | W => [(0, 0), (0, 1), (0, 2), (0, 3)], // ⡇
            },
            Tetromino::S => match oriented {
                N | S => [(0, 0), (1, 0), (1, 1), (2, 1)], // ⠴⠂
                E | W => [(0, 1), (0, 2), (1, 0), (1, 1)], // ⠳
            },
            Tetromino::Z => match oriented {
                N | S => [(0, 1), (1, 0), (1, 1), (2, 0)], // ⠲⠄
                E | W => [(0, 0), (0, 1), (1, 1), (1, 2)], // ⠞
            },
            Tetromino::T => match oriented {
                N => [(0, 0), (1, 0), (1, 1), (2, 0)], // ⠴⠄
                E => [(0, 0), (0, 1), (0, 2), (1, 1)], // ⠗
                S => [(0, 1), (1, 0), (1, 1), (2, 1)], // ⠲⠂
                W => [(0, 1), (1, 0), (1, 1), (1, 2)], // ⠺
            },
            Tetromino::L => match oriented {
                N => [(0, 0), (1, 0), (2, 0), (2, 1)], // ⠤⠆
                E => [(0, 0), (0, 1), (0, 2), (1, 0)], // ⠧
                S => [(0, 0), (0, 1), (1, 1), (2, 1)], // ⠖⠂
                W => [(0, 2), (1, 0), (1, 1), (1, 2)], // ⠹
            },
            Tetromino::J => match oriented {
                N => [(0, 0), (0, 1), (1, 0), (2, 0)], // ⠦⠄
                E => [(0, 0), (0, 1), (0, 2), (1, 2)], // ⠏
                S => [(0, 1), (1, 1), (2, 0), (2, 1)], // ⠒⠆
                W => [(0, 0), (1, 0), (1, 1), (1, 2)], // ⠼
            },
        }
    }

    /// Calculate the piece data that would result from spawning this tetromino as a piece in-play.
    pub const fn spawn_piece(self) -> Piece {
        let tet_width = match self {
            Tetromino::O => 2,
            Tetromino::I => 4,
            _ => 3,
        };

        Piece {
            tetromino: self,
            orientation: Orientation::N,
            position: (((WIDTH - tet_width) / 2) as isize, LOCK_OUT_HEIGHT as isize),
        }
    }

    /// Returns the convened-on standard tile id corresponding to the given tetromino.
    pub const fn tile_id(self) -> TileID {
        use Tetromino::*;
        let u8 = match self {
            O => 1,
            I => 2,
            S => 3,
            Z => 4,
            T => 5,
            L => 6,
            J => 7,
        };
        // SAFETY: Ye, `u8 > 0`;
        unsafe { NonZeroU8::new_unchecked(u8) }
    }
}

impl Orientation {
    /// All `Orientation` enum variants in order.
    ///
    /// Note that `Orientation::VARIANTS[o as usize] == o` always holds.
    pub const VARIANTS: [Self; 4] = {
        use Orientation::*;
        [N, E, S, W]
    };

    /// Find a new direction by turning right some number of times.
    ///
    /// This accepts `i32` to allow for left rotation.
    pub const fn turn_right(self, turns: i8) -> Self {
        Orientation::VARIANTS[((self as i8 + turns) as usize).rem_euclid(4)]
    }
}

impl Piece {
    /// Returns the coordinates and tile types for he piece on the board.
    pub fn tiles(&self) -> [(Coordinate, TileID); 4] {
        let Self {
            tetromino,
            orientation,
            position: (x, y),
        } = self;
        let tile_id = tetromino.tile_id();
        tetromino
            .minos(*orientation)
            .map(|(dx, dy)| ((x + dx, y + dy), tile_id))
    }

    /// Checks whether the piece fits at its current location onto the board.
    pub fn fits_on(&self, board: &Board) -> bool {
        self.tiles().iter().all(|&((x, y), _)| {
            0 <= x
                && (x as usize) < WIDTH
                && 0 <= y
                && (y as usize) < HEIGHT
                && board[y as usize][x as usize].is_none()
        })
    }

    /// Produce the piece with its position offset some.
    pub fn offset(&self, offset: Offset) -> Piece {
        Piece {
            position: self.position.add(offset),
            ..*self
        }
    }

    /// Check whether the piece fits a given offset from its current location onto the board.
    pub fn offset_on(&self, board: &Board, offset: Offset) -> Result<Piece, Piece> {
        let offset_piece = self.offset(offset);

        if offset_piece.fits_on(board) {
            Ok(offset_piece)
        } else {
            Err(offset_piece)
        }
    }

    /// Check whether piece could fall one unit down or not.
    pub fn is_airborne(&self, board: &Board) -> bool {
        self.offset_on(board, (0, -1)).is_ok()
    }

    /// Check whether the piece fits a given offset from its current location onto the board, with
    /// its rotation changed by some number of right turns.
    pub fn reoriented_offset_on(
        &self,
        board: &Board,
        right_turns: i8,
        offset: Offset,
    ) -> Result<Piece, Piece> {
        let reoriented_offset_piece = Piece {
            tetromino: self.tetromino,
            orientation: self.orientation.turn_right(right_turns),
            position: self.position.add(offset),
        };

        (if reoriented_offset_piece.fits_on(board) {
            Ok
        } else {
            Err
        })(reoriented_offset_piece)
    }

    /// Given an iterator over some offsets, check whether the rotated piece fits at any offset
    /// location onto the board.
    pub fn find_reoriented_offset_on(
        &self,
        board: &Board,
        right_turns: i8,
        offsets: impl IntoIterator<Item = Offset>,
    ) -> Option<Piece> {
        let original_pos = self.position;

        let mut updated_piece = *self;
        updated_piece.orientation = updated_piece.orientation.turn_right(right_turns);
        for offset in offsets {
            updated_piece.position = original_pos.add(offset);
            if updated_piece.fits_on(board) {
                return Some(updated_piece);
            }
        }

        None
    }

    /// Return the position the piece would hit if it kept moving at `offset` steps.
    /// For offset `(0,0)` this function return immediately.
    pub fn teleported(&self, board: &Board, offset: Offset) -> Piece {
        let mut updated_piece = *self;

        if offset != (0, 0) {
            // Move piece as far as possible.
            while let Ok(offset_updated_piece) = updated_piece.offset_on(board, offset) {
                if offset_updated_piece == updated_piece {
                    break;
                }
                updated_piece = offset_updated_piece;
            }
        }

        updated_piece
    }
}

impl std::fmt::Display for GameEndCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            GameEndCause::LockOut { .. } => "Lock out",
            GameEndCause::BlockOut { .. } => "Block out",
            GameEndCause::BufferOut { .. } => "Buffer out",
            GameEndCause::Limit(stat) => match stat {
                Stat::TimeElapsed(_) => "Time limit reached",
                Stat::PiecesLocked(_) => "Piece limit reached",
                Stat::LinesCleared(_) => "Line limit reached",
                Stat::PointsScored(_) => "Score limit reached",
            },
            GameEndCause::Forfeit { .. } => "Forfeited",
            GameEndCause::Custom(string) => string,
        };
        write!(f, "{s}")
    }
}

impl DelayParameters {
    /// The duration at which the delay starts.
    pub fn base_delay(&self) -> ExtDuration {
        self.base_delay
    }

    /// The base factor that gets exponentiated by number of line clears;
    /// `factor ^ lineclears ...`.
    ///
    /// Should be in the range `0.0 ≤ .. ≤ 1.0`, where
    /// - `0.0` means 'zero-out initial delay at every line clear',
    /// - `0.5` means 'halve initial delay for every line clear',
    /// - `1.0` means 'keep initial delay at 100%'.
    pub fn factor(&self) -> ExtNonNegF64 {
        self.factor
    }

    /// The base subtrahend that gets multiplied by number of line clears;
    /// `... - subtrahend * lineclears`.
    ///
    /// Should be in the range `0.0 ≤ .. ≤ 1.0`, where
    /// - `0.0` means 'subtract 0% of initial delay for every line clear',
    /// - `0.5` means 'subtract 50% of initial delay for every line clear',
    /// - `1.0` means 'subtract 100% of initial delay for every line clear'.
    pub fn subtrahend(&self) -> ExtDuration {
        self.subtrahend
    }

    /// The duration below which delay cannot decrease.
    pub fn lowerbound(&self) -> ExtDuration {
        self.lowerbound
    }

    /// Delay equation which decreases/decays exponentially in number of linescleared.
    pub fn new(
        base_delay: ExtDuration,
        lowerbound: ExtDuration,
        factor: ExtNonNegF64,
        subtrahend: ExtDuration,
    ) -> Option<Self> {
        Self::constant(Default::default())
            .with_bounds(base_delay, lowerbound)?
            .with_coefficients(factor, subtrahend)
    }

    /// Create a modified delay parameters where only the bounds are changed.
    pub fn with_bounds(&self, base_delay: ExtDuration, lowerbound: ExtDuration) -> Option<Self> {
        let correct_bounds = lowerbound <= base_delay;
        correct_bounds.then_some(Self {
            base_delay,
            lowerbound,
            ..*self
        })
    }

    /// Create a modified delay parameters where only the coefficients are changed.
    pub fn with_coefficients(&self, factor: ExtNonNegF64, subtrahend: ExtDuration) -> Option<Self> {
        let correct_coefficients = factor <= 1.into();
        correct_coefficients.then_some(Self {
            factor,
            subtrahend,
            ..*self
        })
    }

    /// Delay equation which does not change at all with number of linescleared.
    pub fn constant(delay: ExtDuration) -> Self {
        Self {
            base_delay: delay,
            factor: 1.into(),
            subtrahend: ExtDuration::ZERO,
            lowerbound: delay,
        }
    }

    /// Whether the delay curve is invariant to number of lineclears.
    pub fn is_constant(&self) -> bool {
        self.factor == 1.into() && self.subtrahend.is_zero()
    }

    /// Delay equation which implements guideline-like fall delays:
    /// *   0.0  lineclears ~> 20s to fall 20 units (1s/unit).
    /// *  28.8_ lineclears ~> 10s to fall 20 units.
    /// *  94.4_ lineclears ~>  2s to fall 20 units.
    /// * 120.9_ lineclears ~>  1s to fall 20 units.
    /// * 156.8_ lineclears ~> 1/3s to fall 20 units (NES max; 1 unit/frame).
    /// * 196.1_ lineclears ~> 1/60s to fall 20 units (1frame/20units).
    /// * 199.4_ lineclears ~>  0s to fall (instant gravity).
    pub fn standard_fall() -> Self {
        Self {
            base_delay: Duration::from_millis(1000).into(),
            factor: ExtNonNegF64::new(0.9763).unwrap(),
            subtrahend: Duration::from_secs_f64(0.000042).into(),
            lowerbound: Duration::ZERO.into(),
        }
    }

    /// Delay equation which implements guideline-like lock delays:
    /// * 0 lineclears ~> 500ms lock delay.
    /// * Decrease lock_delay by 10 ms every 10 lineclears (= 1 ms every lineclear).
    /// * End at 100ms lock delay.
    pub fn standard_lock() -> Self {
        Self {
            base_delay: Duration::from_millis(500).into(),
            factor: 1.into(),
            subtrahend: Duration::from_millis(1).into(),
            lowerbound: Duration::from_millis(100).into(),
        }
    }

    /// Calculates an actual delay value given a number of lineclears to determine progression.
    pub fn calculate_and_check(&self, lineclears: u32) -> (ExtDuration, bool) {
        // Multiplicative factor computed from lineclears;
        let raw_mul = self.factor.get().powf(f64::from(lineclears));
        // Wrap it back in ExtNonNegF64.
        // SAFETY: ∀e:int, ∀b:f64 ≤ 1, (b^e ≤ 1).
        let mul = ExtNonNegF64::new(raw_mul).unwrap();

        // Subtractive offset computed from lineclears.
        let sub = self.subtrahend.mul_ennf64(lineclears.into());

        // Calculate intended delay;
        let raw_delay = self.base_delay.mul_ennf64(mul).saturating_sub(sub);

        // Return delay capped by lower bound.
        (self.lowerbound.max(raw_delay), self.lowerbound >= raw_delay)
    }
}

impl DelayTable {
    /// Constructs a [`DelayTable`] using a table of entries.
    pub fn new(entries: Vec<ExtDuration>) -> Option<Self> {
        (!entries.is_empty()).then_some(DelayTable { entries })
    }

    /// The entries of the table.
    pub fn entries(&self) -> &Vec<ExtDuration> {
        &self.entries
    }

    /// Delay table which implements NES-like fall delays.
    pub fn classic_fall() -> Self {
        DelayTable {
            entries: [
                48, 43, 38, 33, 28, 23, 18, 13, 8, 6, 5, 5, 5, 4, 4, 4, 3, 3, 3, 2, 2, 2, 2, 2, 2,
                2, 2, 2, 2, 1,
            ]
            .map(|x| Duration::from_secs_f64(f64::from(x) / 60.0).into())
            .into(),
        }
    }

    /// Looks up a delay entry given a number of lineclears to determine progress.
    pub fn lookup_and_check(
        &self,
        lineclears: u32,
        update_delays_every_n_lineclears: u32,
    ) -> (ExtDuration, bool) {
        // Calculate how many times we should've progressed.
        let raw_idx = lineclears / 1.max(update_delays_every_n_lineclears);
        // Saturate to last entry.
        let idx = (raw_idx as usize).min(self.entries.len());

        (self.entries[idx], idx == self.entries.len() - 1)
    }
}

/// Extension trait to DelayDurve.
pub trait DelayCurveExt {
    /// Retrieve a delay value and check if it has hit its limit (usually lower bound).
    fn retrieve_and_check(
        &self,
        lineclears: u32,
        update_delays_every_n_lineclears: u32,
    ) -> (ExtDuration, bool);
}

impl DelayCurveExt for DelayCurve {
    fn retrieve_and_check(
        &self,
        lineclears: u32,
        update_delays_every_n_lineclears: u32,
    ) -> (ExtDuration, bool) {
        match self {
            Either::Left(params) => params.calculate_and_check(lineclears),
            Either::Right(table) => {
                table.lookup_and_check(lineclears, update_delays_every_n_lineclears)
            }
        }
    }
}

impl GameLimits {
    /// Create a fresh [`GameLimits`] without any limits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new [`GameLimits`] with a single [`Stat`] as the limit.
    pub fn single(stat: Stat, is_win: bool) -> Self {
        let mut new = Self::new();

        match stat {
            Stat::TimeElapsed(t) => new.time_elapsed = Some((t, is_win)),
            Stat::PiecesLocked(p) => new.pieces_locked = Some((p, is_win)),
            Stat::LinesCleared(l) => new.lines_cleared = Some((l, is_win)),
            Stat::PointsScored(s) => new.points_scored = Some((s, is_win)),
        };

        new
    }

    /// Iterate over all limiting [`Stat`] contained in a [`GameLimits`] struct.
    pub fn iter(&self) -> impl Iterator<Item = (Stat, bool)> {
        [
            self.time_elapsed
                .map(|(t, is_win)| (Stat::TimeElapsed(t), is_win)),
            self.pieces_locked
                .map(|(p, is_win)| (Stat::PiecesLocked(p), is_win)),
            self.lines_cleared
                .map(|(l, is_win)| (Stat::LinesCleared(l), is_win)),
            self.points_scored
                .map(|(s, is_win)| (Stat::PointsScored(s), is_win)),
        ]
        .into_iter()
        .flatten()
    }
}

impl Button {
    /// All `Button` enum variants.
    ///
    /// Note that `Button::VARIANTS[b as usize] == b` always holds.
    pub const VARIANTS: [Self; 11] = {
        use Button as B;
        [
            B::MoveLeft,
            B::MoveRight,
            B::RotateLeft,
            B::RotateRight,
            B::Rotate180,
            B::DropSoft,
            B::DropHard,
            B::TeleDown,
            B::TeleLeft,
            B::TeleRight,
            B::HoldPiece,
        ]
    };
}

impl<T> ops::Index<Button> for [T; Button::VARIANTS.len()] {
    type Output = T;

    fn index(&self, idx: Button) -> &Self::Output {
        &self[idx as usize]
    }
}

impl<T> ops::IndexMut<Button> for [T; Button::VARIANTS.len()] {
    fn index_mut(&mut self, idx: Button) -> &mut Self::Output {
        &mut self[idx as usize]
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

impl std::fmt::Display for UpdateGameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            UpdateGameError::TargetTimeInPast => {
                "cannot update game to timestamp it already passed"
            }
            UpdateGameError::AlreadyEnded => "cannot update game after it already ended",
        };
        write!(f, "{s}")
    }
}

impl std::error::Error for UpdateGameError {}
