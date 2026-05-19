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

mod core;
mod game_building;
mod game_modding;
mod game_update;
mod helper_types;
mod piece_rotation;
mod tetromino_generation;

pub use prelude_base::*;
pub use prelude_generic_stdized::*;

pub mod prelude_base {
    //! Non-generic base library constants, types and traits.
    pub use crate::core::{
        BOARD_WIDTH, Button, ButtonsState, CoordExt, Coordinate, DelayCurve, DelayCurveExt,
        DelayParameters, DelayTable, GameEndCause, GameLimits, GameRng, InGameTime, Input,
        MAX_BOARD_HEIGHT, Offset, Orientation, PLAYABLE_BOARD_HEIGHT, Phase, Piece, SoftDropRate,
        Stat, Tetromino, UpdateGameError,
    };
    pub use crate::helper_types::{extduration::ExtDuration, extnonnegf64::ExtNonNegF64};
    pub use crate::piece_rotation::{
        ClassicLRot, ClassicRRot, MiscPceRots, OcularRot, PieceRotator, SuperRot,
    };
    pub use crate::tetromino_generation::{
        BalanceOutGen, MiscTetGens, RecencyGen, RerollGen, StockGen, TetrominoGenerator,
    };
}

pub mod prelude_generic {
    //! Re-exports of generic library types and traits.
    pub use crate::core::{
        Board, Configuration, Game, Line, Notification, NotificationFeed, State,
        StateInitialization,
    };
    pub use crate::game_building::GameBuilder;
    pub use crate::game_modding::{GameAccess, GameModifier};
}

pub mod prelude_generic_stdized {
    //! Re-exports of generic library types and traits with provided defaults.
    //!
    //! It is encouraged to copy this sub-module to generate customized type re-exports for one's own project.
    //!
    //! # Note on Type Inference
    //! Importing this provides better type inference, as adding generic type defaults to our `core` did not always work as expected as of Rust 1.95.0.
    //
    // # Examples
    //
    // ```ignore
    // // Assume this imaginary `core` defines: `pub struct Game<TetGen = StdTetGen, PceRot = StdPceRot, TileData = Tetromino> { .. };`.
    // use falling_tetromino_engine::core;
    //
    // let ex1 = core::Game::new(); // ERROR - Type inference failed despite defaults for each generic parameter in original struct def.
    //
    // let ex2: core::Game = core::Game::new(); // Ok
    // let ex3 = <core::Game>::new(); // Ok
    //
    // type CoreGame = core::Game;
    // let ex4 = CoreGame::new(); // Ok
    // ```
    //
    // ```rs
    // use falling_tetromino_engine::std_generic_types_prelude as nice;
    //
    // let ex5 = nice::Game::new();
    // ```
    #![allow(missing_docs)]

    use crate::prelude_base::{MiscPceRots, MiscTetGens, Tetromino};
    use crate::prelude_generic;

    type StdTetGen = MiscTetGens;
    type StdPceRot = MiscPceRots;
    type StdTileData = Tetromino;

    pub type Board = prelude_generic::Board<StdTileData>;
    pub type Line = prelude_generic::Line<StdTileData>;
    pub type Notification = prelude_generic::Notification<StdTileData>;
    pub type NotificationFeed = prelude_generic::NotificationFeed<StdTileData>;
    pub type GameBuilder = prelude_generic::GameBuilder<StdTetGen, StdPceRot, StdTileData>;
    pub type Configuration = prelude_generic::Configuration<StdPceRot>;
    pub type StateInitialization = prelude_generic::StateInitialization<StdTetGen>;
    pub type State = prelude_generic::State<StdTetGen, StdTileData>;
    pub type Game = prelude_generic::Game<StdTetGen, StdPceRot, StdTileData>;
    pub type GameAccess<'a> = prelude_generic::GameAccess<'a, StdTetGen, StdPceRot, StdTileData>;

    // FIXME: This should be made possible using trait aliases or so.
    // pub trait GameModifier = prelude_generic::GameModifier<StdTetGen, StdPceRot, StdTileData>;
    pub use prelude_generic::GameModifier;
}
