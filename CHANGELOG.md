# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [Unreleased]

--


## [1.4.0] - 2026-03-23

### Added
- `GameModifier` trait with many, many proper hooks (methods).
    * `try_clone` for mods!

### Changed
- Major revamp of Modding facilities.
- Renames:
    * 'feedback' terminology becomes 'notification'.
    * `FeedbackVerbosity` -> `NotificationLevel`.
    * `Feedback` -> `Notification`.
    * no more `FeedbackMsg = (Feedback, Time)`, but `NotificationFeed = Vec<(Notification, Time)>`.
* `Game::clone_unmodded` now becomes the more proper `Game::try_clone` (if modifiers' `try_clone` succeed).


## [1.3.0] - 2026-03-22

### Added
- `Piece::is_airborne`.

### Changed
- Player input is now handled before falling/locking/auto-moving if they occur at the exact same in-game-time.
- Auto-movement is now handled before falling/locking if they occur at the exact same in-game-time.
- Auto-movement now consistently causes its own timeline event every time. (Relevant for mods.)
- Use `ChaCha8Rng` (previously `ChaCha12Rng`).
- Module names: `builder`, `randomization`, `rotation`

### Removed
- `struct PieceData { .. }` has been superseded by `Phase::PieceInPlay { .. }`.


## [1.2.0] 2026-03-20

### Added
- `ensure_move_delay_lt_lock_delay` toggle: Ensures that when lock delay is very low, DAS/ARR is dynamically adapted to be shorter.
- `ButtonsState` type alias.
- `GameLimits` struct and methods `::{new, single, iter}`.
- `GameEndCause::{Custom(String), TopOut { blocked_lines: Vec<Line> }}`.
- Left-/Right-handed versions of classic rotation: `RotationSystem::{ClassicL, ClassicR}`.
- `TetrominoGenerator::Recency` gained new field `is_base_not_exp`, further providing control over weights calculation.

### Changed
- Default `Configuration` adapted.
- `Coord`s are now `isize` instead of `usize` (for flexibility; we need to check bounds either way).
- Refactor entire player movement logic:
    * Improved handling of incoming button activations even while button is still active / has not been deactivated yet.
    * Handle edge cases where movement buttons are pressed at the exact same instant.
- Piece offset / rotation logic has been cleaned up and more explicitly implements desired behavior in edge cases.
    * See `Piece::{fits_onto, offset_on, reoriented_offset_on, find_reoriented_offset_on}`.
- Game more efficiently checks most limits only when necessary instead of every update loop iteration.
- `Game::forfeit` is now much more similar in functionality and behavior to `Game::update`.
- `Game::result` has been superseded by `Game::has_ended`.
- `GameResult` has been superseded by `GameEndCause`.
- `ButtonChange::{Press, Release}` has been superseded by `Input::{Activate, Deactivate}`.
- `GameEndCause::BlockOut` -> `GameEndCause::BlockOut { blocked_piece: Piece }`.
- `GameEndCause::LockOut` -> `GameEndCause::LockOut { locking_piece: Piece }` + do not lock this piece onto board.
- `GameEndCause::Forfeit` -> `GameEndCause::Forfeit { piece_in_play: Option<Piece> }`
- `Phase::GameEnd { result: GameResult }` -> `Phase::GameEnd { cause: GameEndCause, is_win: bool }`.
- `TetrominoGenerator`s tweaked and hardened against crashes.
- List of renames:
    * `Offset` -> `CoordOffset`
    * `allow_prespawn_actions` -> `allow_initial_actions`
    * `soft_drop_divisor` -> `soft_drop_factor`
    * `lenient_lock_delay_reset` -> `allow_lenient_lock_reset`
    * `end_conditions` -> `game_limits`
    * `Button::RotateAround` -> `Button::Rotate180`
    * `GameOver` -> `GameEndCause`
    * `capped_lock_time` -> `lock_time_cap`
    * `UpdateGameError::GameEnded` -> `UpdateGameError::AlreadyEnded`
    * `SKYLINE_HEIGHT` -> `LOCK_OUT_HEIGHT`
    * `buttons_pressed` -> `active_buttons`
    * `TetrominoGenerator::BalanceRelative -> TetrominoGenerator::BalanceOut`
    * `recency` -> `snappy_recency`


## [1.1.0] - 2026-03-08

### Fixed
- When left and right are pressed + left[/right] was pressed last,
  the mere act of releasing right[/left] by itself does not trigger a move to the left[/right] anymore.


## [1.0.0] - 2026-02-16

### Added
- Initial release
- Game implementation, notably with update method

### Changed
-

### Fixed
-

### Removed
-
