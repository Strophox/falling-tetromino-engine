# Falling Tetromino Engine

[![Crates.io](https://img.shields.io/crates/v/falling-tetromino-engine.svg)](https://crates.io/crates/falling-tetromino-engine)
[![Documentation](https://docs.rs/falling-tetromino-engine/badge.svg)](https://docs.rs/falling-tetromino-engine)
[![License](https://img.shields.io/crates/l/falling-tetromino-engine)](https://github.com/Strophox/falling-tetromino-engine#license)

A tetromino stacker engine in Rust, with the goals of being featureful, efficient and elegant.


## Installation

Run `cargo add falling-tetromino-engine`.

Most engine types support Serialization through [`serde`](https://crates.io/crates/serde), available with the corresponding feature flag:
```toml
[dependencies]
falling-tetromino-engine = { version = "X.Y.Z", features = ["serde"] }
```


## Simple Example

```rust
use falling_tetromino_engine::*;

// Initialize a game. In-game time starts at 0s.
let mut game = Game::builder()
    .seed(1234)
    /* Further customization possible here. */
    .build();

// Update the game with info that 'left' is activated at second 4.2 (i.e. piece starts moving left).
let input = Input::Activate(Button::MoveLeft);
game.update(InGameTime::from_secs(4.2), Some(input));

// Update the game with info that no input changes up to second 6.79 (e.g. piece falls).
game.update(InGameTime::from_secs(6.79), None);

// Read game state (for rendering etc.)
let State { board, .. } = game.state();
```


## Features Overview

Fundamental points to note:
- The engine implements the pure game logic/backend, i.e. it accurately and only simulates a virtual board with tetromino pieces spawning/moving/locking, lines clearing etc. 
- The engine is frontend-agnostic and by itself does not prescribe how to interact with the real world player (it does not know about the keyboard, refresh-/framerate etc.)

Internally, the game processes a pure timeline like so:
```
Piece spawns              e.g. Game state viewed here
|        Piece falls                  |
|        |       Piece falls          |
v        v       v                    v
|--------¦--|----¦-------¦-------¦----+--¦------->
            ^
            |
            "RotateLeft" player input:
             Piece rotates
```
I.e. running a game at 60 Hz just means that `Game::update` is called 60 times in one second to determine the state in the timeline and show it.
(The precision used internally is currently based on [`std::time::Duration`](<https://doc.rust-lang.org/std/time/struct.Duration.html>) which goes down to nanoseconds.)

Depending on configuration, calls to `Game::update` and `Game::forfeit` can return additional information (`Notification`) which can facilitate frontend implementation (e.g. hard drop, piece lock, line clears and other visual feedback).

The engine provides possibilities for compile-time modding.
Mods may arbitrarily access and modify game state when called on given engine hooks.

In terms of advanced game mechanics the engine aims to compare with other modern tetromino stackers.
It should already incorporate many features desired by familiar/experienced players, such as:
- Available player actions:
    - **Move** left/right,
    - **Rotate** left/right/180°
    - **Drop** soft/hard
    - **Teleport** down(='Sonic drop') and left/right
    - **Hold** piece,
- **Tetromino randomizers**: 'Uniform', 'Stock' (generalized Bag), 'Recency' (history), 'Balance-out',
- **Piece preview** (arbitrary size),
- **Spawn delay** (ARE),
- **Initial actions** on-piece-spawn toggle ('Initial Hold/Rotation System'),
- **Rotation systems**: 'Ocular' (engine-specific, playtested), 'Classic', 'Super',
- **Delayed auto-move** (DAS),
- **Auto-move rate** (ARR),
- **Soft drop factor** (SDF),
- **Customizable gravity/fall and lock delay curves** (exponential and/or linear; also, '20G' (fall rate of ≥1200 Hz) just becomes ≤00083s fall delay),
- **Ensure move delay less than lock delay** toggle (i.e. DAS/ARR are automatically shortened when lock delay is very low),
- **Allow lenient lock-reset** toggle (i.e. reset lock delay even if rotate/move fails),
- **Lock-reset cap factor** (i.e. maximum time before lock delay cannot be reset),
- **Line clear duration** (LCD),
- **Customizable win/loss conditions** based on the time, pieces, lines, points,
- Score more **points** for larger lineclears, spins ('allspin'), perfect clear, combo,
- Game **reproducibility** (PRNG/determinism).

The basics seem to have been figured out through many iterative improvements.
Ongoing areas of investigation (to improve generalization) are:
- Choice of `Notification`s provided to frontend clients;
- Choice of update `Hook`s for modding clients;
- Choice of `Stat`s to query game with or make game automatically halt;
- Various engine generalizations for engine clients that want to plug custom behavior for currently-hardcoded structures.


## Implementation Idea

The game keeps:
- **Configuration** to read from, which determines game behavior.
- **State values** which persist throughout the game.
- A dedicated **'phase'**-state field:
    - This represents the macro-scale state machine and can store values specific to separate stages during the game.
    - E.g., 'Spawning' (no piece) vs. 'Piece-is-in-play' (with piece data to keep track of).

During each update, the game looks at the (very limited) number of upcoming in-game 'events' and processes them.
The only complicated phase is `Phase::PieceInPlay { .. }`, which encapsulates several types of upcoming events (priority in the given order):
- **Action by player**: Player input which causes e.g. the piece to move, makes it lock immediately or cancels auto-movement.
- **Autonomous movement**: While move buttons are active ('held down'), the piece may move autonomously.
- **Falling *or* locking**: Whenever the piece is airborne *or* grounded, there is an upcoming fall *or* lock scheduled.


## Engine Insights using just Type definitions and API

Much of the implementation is tightly encoded into types.
It may be instructive to simply consider the main type definitions for good insight into the deeper engine mechanics.
See also [full documentation](https://docs.rs/falling-tetromino-engine).

### Main `Game` type
```rust
// The main engine type.
struct Game {
    // May be modified by user, will not be modified by Game.
    config: Configuration,

    // Cannot be modified, basic data used for reproducibility.
    state_init: StateInitialization,

    // Cannot be modified by user, used by Game.
    state: State,
    // Cannot be modified by user, used by Game.
    phase: Phase,

    // Modding.
    modifiers: Vec<Box<dyn GameModifier>>,
}

impl Game {
  fn update(
    &mut self,
    mut target_time: InGameTime,
    mut player_input: Option<Input>, // "With or without button changes."
  ) -> Result<NotificationFeed, UpdateGameError>;

  fn forfeit(&mut self) -> Result<NotificationFeed, UpdateGameError>;
}
```


### `Game` fields types

```rust
struct Configuration {
    piece_preview_count: usize,
    allow_initial_actions: bool,
    rotation_system: RotationSystem,
    spawn_delay: Duration,
    delayed_auto_shift: Duration,
    auto_repeat_rate: Duration,
    fall_delay_params: DelayParameters,
    soft_drop_factor: ExtNonNegF64,
    lock_delay_params: DelayParameters,
    ensure_move_delay_lt_lock_delay: bool,
    allow_lenient_lock_reset: bool,
    lock_reset_cap_factor: ExtNonNegF64,
    line_clear_duration: Duration,
    update_delays_every_n_lineclears: u32,
    game_limits: Vec<(Stat, bool)>,
    notification_level: NotificationLevel,
}

struct StateInitialization {
    seed: u64,
    tetromino_generator: TetrominoGenerator,
}

struct State {
    time: InGameTime,
    active_buttons: [Option<InGameTime>; Button::VARIANTS.len()],
    rng: GameRng,
    piece_generator: TetrominoGenerator,
    piece_preview: VecDeque<Tetromino>,
    piece_held: Option<(Tetromino, bool)>,
    board: [[Option<TileID>; Game::WIDTH]; Game::HEIGHT],
    fall_delay: ExtDuration,
    fall_delay_lowerbound_hit_at_n_lineclears: Option<u32>,
    lock_delay: ExtDuration,
    pieces_locked: [u32; Tetromino::VARIANTS.len()],
    lineclears: u32,
    consecutive_line_clears: u32,
    points: u32,
}

enum Phase {
    Spawning { spawn_time: InGameTime },
    PieceInPlay {
        piece: Piece,
        auto_move_scheduled: Option<InGameTime>,
        fall_or_lock_time: InGameTime,
        lock_time_cap: InGameTime,
        lowest_y: isize,
    },
    LinesClearing { clear_finish_time: InGameTime, points_bonus: u32 },
    GameEnd { cause: GameEndCause, is_win: bool },
}
```

### Some Other Types

```rust
enum Tetromino { O, I, S, Z, T, L, J, }

enum Orientation { N, E, S, W, }

struct Piece {
    tetromino: Tetromino,
    orientation: Orientation,
    position: Coord,
}

enum Button {
    MoveLeft, MoveRight,
    RotateLeft, RotateRight, Rotate180,
    DropSoft, DropHard,
    TeleLeft, TeleRight, TeleDown, 
    HoldPiece,
}

enum Input { Activate(Button), Deactivate(Button), }

type InGameTime = Duration;
type GameRng = ChaCha8Rng;

struct DelayParameters {
    base_delay: ExtDuration,
    factor: ExtNonNegF64,
    subtrahend: ExtDuration,
    lowerbound: ExtDuration,
}

enum Notification {
    PieceLocked { piece: Piece },
    LinesClearing {
        y_coords: Vec<usize>,
        line_clear_duration: InGameTime,
    },
    HardDrop {
        height_dropped: usize,
        dropped_piece: Piece,
    },
    Accolade {
        points_bonus: u32,
        lineclears: u32,
        combo: u32,
        is_spin: bool,
        is_perfect_clear: bool,
        tetromino: Tetromino,
    },
    GameEnded {
        is_win: bool,
    },
    Debug(String),
    Custom(String),
}

enum NotificationLevel { Silent, Standard, Debug, }

enum UpdateGameError { TargetTimeInPast, AlreadyEnded, }

enum GameEndCause {
    LockOut { locking_piece: Piece },
    BlockOut { blocked_piece: Piece },
    TopOut { blocked_lines: Vec<Line> },
    Limit(Stat),
    Forfeit { piece_in_play: Option<Piece> },
    Custom(String),
}
```

### Modding

```rust
trait GameModifier: std::fmt::Debug {
    fn id(&self) -> String;
    fn args(&self) -> String;
    fn try_clone(&self) -> Result<Box<dyn GameModifier>, String>;

    fn on_player_input_received(&mut self, game: GameAccess, feed: &mut NotificationFeed, time: &mut InGameTime, player_input: &mut Option<Input>) {}
    fn on_game_built(&mut self, game: GameAccess) {}
    fn on_game_ended(&mut self, game: GameAccess, feed: &mut NotificationFeed) {}
    fn on_time_state_progression_pre(&mut self, game: GameAccess, feed: &mut NotificationFeed, time: &mut InGameTime) {}
    fn on_time_state_progression_post(&mut self, game: GameAccess, feed: &mut NotificationFeed) {}
    fn on_check_game_limits_post(&mut self, game: GameAccess, feed: &mut NotificationFeed) {}
    fn on_spawn_pre(&mut self, game: GameAccess, feed: &mut NotificationFeed, time: &mut InGameTime) {}
    fn on_spawn_post(&mut self, game: GameAccess, feed: &mut NotificationFeed) {}
    fn on_player_action_pre(&mut self, game: GameAccess, feed: &mut NotificationFeed, input: Input, time: &mut InGameTime) {}
    fn on_player_action_post(&mut self, game: GameAccess, feed: &mut NotificationFeed, input: Input) {}
    fn on_auto_move_pre(&mut self, game: GameAccess, feed: &mut NotificationFeed, time: &mut InGameTime) {}
    fn on_auto_move_post(&mut self, game: GameAccess, feed: &mut NotificationFeed) {}
    fn on_fall_pre(&mut self, game: GameAccess, feed: &mut NotificationFeed, time: &mut InGameTime) {}
    fn on_fall_post(&mut self, game: GameAccess, feed: &mut NotificationFeed) {}
    fn on_lock_pre(&mut self, game: GameAccess, feed: &mut NotificationFeed, time: &mut InGameTime) {}
    fn on_lock_post(&mut self, game: GameAccess, feed: &mut NotificationFeed) {}
    fn on_lines_clear_pre(&mut self, game: GameAccess, feed: &mut NotificationFeed, time: &mut InGameTime) {}
    fn on_lines_clear_post(&mut self, game: GameAccess, feed: &mut NotificationFeed) {}
}
```
