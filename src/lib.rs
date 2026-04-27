/*!
# Falling Tetromino Engine

A tetromino stacker engine in Rust, with the goals of being featureful, efficient and elegant.

```rust
use falling_tetromino_engine::*;

// Initialize a game. In-game time starts at 0s.
let mut game = Game::builder()
    .seed(1234)
    /* Further customization possible here. */
    .build();

// Update the game with info that 'left' is activated at second 4.2 (i.e. piece starts moving left).
let input = Input::Activate(Button::MoveLeft);
game.update(InGameTime::from_secs_f64(4.2), Some(input));

// Update the game with info that no input changes up to second 6.79 (e.g. piece falls).
game.update(InGameTime::from_secs_f64(6.79), None);

// Read game state (for rendering etc.)
let State { board, .. } = game.state();
```


## Features Overview

Fundamental points to note:
- The engine implements the pure game logic/backend, i.e. it accurately and only simulates a virtual board with tetromino pieces spawning/moving/locking, lines clearing etc.
- The engine is frontend-agnostic and by itself does not prescribe how to interact with the real world player (it does not know about the keyboard, refresh-/framerate etc.)

Internally, the game processes a pure timeline like so:
```txt
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
- **Spawn actions** (IRS/IHS; by keeping rotate/hold pressed during spawn),
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

## FIXME
Current documentation is lacking and sometimes slightly outdated. *All* features should be commented in detail (including IRS, etc., cargo feature `serde` etc.)]]
*/

#![doc(
    html_logo_url = "https://github.com/Strophox/falling-tetromino-engine/blob/e707bda026a3ec24a250caed96cb907e6924d7f1/logo/tetromino_logo_glow2.png?raw=true"
)]
#![doc(
    html_favicon_url = "https://github.com/Strophox/falling-tetromino-engine/blob/e707bda026a3ec24a250caed96cb907e6924d7f1/logo/tetromino_logo.png?raw=true"
)]
#![warn(missing_docs)]

pub mod core;
pub mod game_building;
pub mod game_modding;
pub mod game_update;
pub mod helper_types;
pub mod piece_rotation;
pub mod tetromino_generation;

pub use core::{
    Board, Button, ButtonsState, CoordAdd, Coordinate, DelayCurve, DelayParameters, DelayTable,
    GameEndCause, GameLimits, GameRng, HEIGHT, InGameTime, Input, LOCK_OUT_HEIGHT, Line,
    Notification, NotificationFeed, Offset, Orientation, Piece, SoftDropRate, Stat, Tetromino,
    TileID, UpdateGameError, WIDTH,
};
pub use game_building::GameBuilder;
pub use game_modding::GameModifier;
pub use helper_types::{extduration::ExtDuration, extnonnegf64::ExtNonNegF64};
pub use piece_rotation::{PieceRotator, StdPceRot};
pub use tetromino_generation::{StdTetGen, TetrominoGenerator};

/// Standard export of the more generic [`core::Configuration`] type.
///
/// # Note on Type Inference
/// Importing this provides better type inference, as the generic type defaults do not always work as expected for `core::Configuration`.
pub type Configuration = core::Configuration;
/// Standard export of the more generic [`core::StateInitialization`] type.
///
/// # Note on Type Inference
/// Importing this provides better type inference, as the generic type defaults do not always work as expected for `core::StateInitialization`.
pub type StateInitialization = core::StateInitialization;
/// Standard export of the more generic [`core::State`] type.
///
/// # Note on Type Inference
/// Importing this provides better type inference, as the generic type defaults do not always work as expected for `core::State`.
pub type State = core::State;
/// Standard export of the more generic [`core::Phase`] type.
///
/// # Note on Type Inference
/// Importing this provides better type inference, as the generic type defaults do not always work as expected for `core::Phase`.
pub type Phase = core::Phase;
/// Standard export of the more generic [`core::Game`] type.
///
/// # Note on Type Inference
/// Importing this provides better type inference, as the generic type defaults do not always work as expected for `core::Game`.
pub type Game = core::Game;

/// Standard export of the more generic [`core::Game`] type.
///
/// # Note on Type Inference
/// Importing this provides better type inference, as the generic type defaults do not always work as expected for `game_modding::GameAccess`.
pub type GameAccess<'a> = game_modding::GameAccess<'a>;
