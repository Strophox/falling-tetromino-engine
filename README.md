# Falling Tetromino Engine

[![Crates.io](https://img.shields.io/crates/v/falling-tetromino-engine.svg)](https://crates.io/crates/falling-tetromino-engine)
[![Documentation](https://docs.rs/falling-tetromino-engine/badge.svg)](https://docs.rs/falling-tetromino-engine)
[![License](https://img.shields.io/crates/l/falling-tetromino-engine)](https://github.com/Strophox/falling-tetromino-engine#license)

A backend with ergonomic API for games where tetrominos fall and stack.

## Installation

Add this to your Cargo.toml:

```toml
[dependencies]
falling-tetromino-engine = "1.0.0"
```


# About

The engine is completely frontend-agnostic, although calls to `Game::update` may optionally return additional info to facilitate implementation of visual effects.

The engine allows for compile-time mods which may arbitrarily access and modify game state during gameplay.

The engine aims to compete on the order of modern tetromino stackers;
It incorporates many features found in such games.
Experienced players may be familiar with most of the following mechanics:
- Variable gravity/fall delay, possibly in-between 'frames', '20G' (fall delay = 0s),
- Simple but flexible programming of custom fall and lock delay progressions (`DelayParameters`),
- (Arbitrary) piece preview,
- Pre-spawn action toggle ('Initial Hold/Rotation System'),
- Rotation systems: 'Ocular' (engine-specific, playtested), 'Classic', 'Super',
- Tetromino generators: 'Uniform', 'Stock' (generalized Bag), 'Recency' (history), 'Balancerelative',
- Spawn delay (ARE),
- Delayed auto-shift (DAS),
- Auto-repeat rate (ARR),
- Soft drop factor (SDF),
- Lenient-lock-delay-reset toggle (reset lock delay even if rotation fails),
- Lock-reset-cap factor (~maximum time before lock delay cannot be reset),
- Line clear delay (LCD),
- Custom win/loss conditions based on stats: time, pieces, lines, score,
- Hold piece,
- Higher score for higher lineclears and spins ('allspin')
- Game reproducibility (PRNG),
- Available player actions: MoveLeft, MoveRight; RotateLeft, RotateRight, RotateAround (180°); DropSoft, DropHard, TeleDown ('Sonic drop'), TeleLeft, TeleRight, HoldPiece.


# Example Usage

```rust
use falling_tetromino_engine::*;

// Starting up a game - note that in-game time starts at 0.0s.
let mut game = Game::builder()
    .seed(42)
    /* ...Further optional configuration possible... */
    .build();

// Updating the game with the info that 'left' should be pressed at second 5.0;
// If a piece is in the game, it will try to move left.
let button_change = ButtonChange::Press(Button::MoveLeft);
game.update(GameTime::from_secs(5.0), Some(button_change));

// ...

// Updating the game with the info that no input change has occurred up to second 7.0;
// This updates the game, e.g., pieces fall.
game.update(GameTime::from_secs(7.0), None);

// Read most recent game state;
// This is how a UI can know how to render the board, etc.
let State { board, .. } = game.state();
```


# Overview by Types

Much of the implementation is tightly encoded into types.

It may be instructive just to consider the main type definitions as an overview:

```rust
// The central engine type.
struct Game {
    config: Configuration, // Can be safely modified during play.
    state_init: StateInitialization,
    state: State,
    phase: Phase,
    modifiers: Vec<Modifier>, // Arbitrary compile-time mods by user.
}

// Fields:

struct Configuration {
    piece_preview_count: usize,
    allow_prespawn_actions: bool,
    rotation_system: RotationSystem,
    spawn_delay: Duration,
    delayed_auto_shift: Duration,
    auto_repeat_rate: Duration,
    fall_delay_params: DelayParameters,
    soft_drop_divisor: ExtNonNegF64,
    lock_delay_params: DelayParameters,
    lenient_lock_delay_reset: bool,
    lock_reset_cap_factor: ExtNonNegF64,
    line_clear_duration: Duration,
    update_delays_every_n_lineclears: u32,
    end_conditions: Vec<(Stat, bool)>,
    feedback_verbosity: FeedbackVerbosity,
}

struct StateInitialization {
    seed: u64,
    tetromino_generator: TetrominoGenerator,
}

struct State {
    time: InGameTime,
    buttons_pressed: [Option<InGameTime>; Button::VARIANTS.len()],
    rng: GameRng,
    piece_generator: TetrominoGenerator,
    piece_preview: VecDeque<Tetromino>,
    piece_held: Option<(Tetromino, bool)>,
    board: [[Option<TileTypeID>; Game::WIDTH]; Game::HEIGHT],
    fall_delay: ExtDuration,
    fall_delay_lowerbound_hit_at_n_lineclears: Option<u32>,
    lock_delay: ExtDuration,
    pieces_locked: [u32; Tetromino::VARIANTS.len()],
    lineclears: u32,
    consecutive_line_clears: u32,
    score: u32,
}

enum Phase {
    Spawning { spawn_time: InGameTime },
    PieceInPlay { piece_data: PieceData },
    LinesClearing { line_clears_finish_time: InGameTime },
    GameEnd { result: GameResult },
}

struct Modifier {
    descriptor: String,
    mod_function: Box<GameModFn>,
}
```

```rust
// Small SELECTION of smaller types:

enum Button {
    MoveLeft,   MoveRight,
    RotateLeft, RotateRight, RotateAround,
    DropSoft, DropHard,
    TeleLeft,   TeleRight, TeleDown, 
    HoldPiece,
}

enum Tetromino {
    O, I, S, Z, T, L, J
}

struct Piece {
    tetromino: Tetromino,
    orientation: Orientation,
    position: Coord,
}


struct DelayParameters {
    base_delay: ExtDuration,
    factor: ExtNonNegF64,
    subtrahend: ExtDuration,
    lowerbound: ExtDuration,
}

type GameModFn = dyn FnMut(
    &mut UpdatePoint<&mut Option<ButtonChange>>,
    &mut Configuration,
    &StateInitialization,
    &mut State,
    &mut Phase,
    &mut Vec<FeedbackMsg>,
);
```

These are almost all types, although for further information see documentation.
