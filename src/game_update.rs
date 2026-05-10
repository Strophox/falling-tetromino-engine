/*!
Handles what happens when [`Game::update`] is called.
*/

use either::Either;

use crate::{
    core::{Configuration, DelayCurveExt, Game, Phase, State},
    game_modding::Hook,
};

use super::*;

impl<TetGen: TetrominoGenerator, PceRot: PieceRotator> Game<TetGen, PceRot> {
    /// Retrieve the when the next *autonomous* in-game update is scheduled.
    /// I.e., compute the next time the game would change state assuming no button updates
    ///
    /// Returns `None` when game ended.
    ///
    /// # Modifiers
    /// Note that this only predicts what an unmodded game would do;
    /// [`GameModifier`]s may arbitrarily change game state and change or prevent precise update predictions.
    pub fn peek_next_update_time(&self) -> Option<InGameTime> {
        // Find the next autonomous game update.
        let mut update_time = match self.phase {
            Phase::GameEnd { .. } => return None,
            Phase::ClearingLines {
                clear_finish_time, ..
            } => clear_finish_time,
            Phase::Spawning { spawn_time } => spawn_time,
            Phase::PieceInPlay {
                autoshift_scheduled,
                fall_or_lock_time,
                ..
            } => 'exp: {
                if let Some(autoshift_time) = autoshift_scheduled
                    && autoshift_time < fall_or_lock_time
                {
                    break 'exp autoshift_time;
                }
                fall_or_lock_time
            }
        };

        // Check against time-related end conditions.
        if let Some((time_limit, _)) = self.config.game_limits.time_elapsed
            && time_limit < update_time
        {
            update_time = time_limit;
        }

        Some(update_time)
    }

    /// Immediately end a game by forfeiting the current round.
    ///
    /// This can be used so `game.has_ended()` returns true and prevents future
    /// calls to `update` from continuing to advance the game.
    pub fn forfeit(&mut self) -> Result<NotificationFeed, UpdateGameError> {
        let piece_in_play = match self.phase {
            Phase::GameEnd { .. } => {
                // Do not allow updating a game that has already ended.
                return Err(UpdateGameError::AlreadyEnded);
            }

            Phase::Spawning { .. } | Phase::ClearingLines { .. } => None,
            Phase::PieceInPlay { piece, .. } => Some(piece),
        };

        self.phase = Phase::GameEnd {
            cause: GameEndCause::Forfeit { piece_in_play },
            is_win: false,
        };

        let mut feed = vec![(
            Notification::GameEnded {
                cause: GameEndCause::Forfeit { piece_in_play },
                is_win: false,
            },
            self.state.time,
        )];

        self.run_mods(Hook::GameEnded, &mut feed);

        Ok(feed)
    }

    /// The main function used to advance the game state.
    ///
    /// This will cause an internal update of the game's state up to and including the given
    /// `update_target_time` requested.
    /// If `input` is nonempty, then the same thing happens except that the `input`
    /// will be used at `update_target_time` after all autonomous updates are processed;
    /// The `input` may then cause additional updates / interactions (which are also
    /// handled exhaustively) before finally returning with in-game time at `target_time`.
    ///
    /// Unless an error occurs, this function will return all [`NotificationFeed`]s caused between the
    /// previous and the current `update` call, in chronological order.
    ///
    /// # Errors
    ///
    /// This function may error with:
    /// - [`UpdateGameError::AlreadyEnded`] if `game.ended()` is `true`, indicating that no more updates
    ///   can change the game state, or
    /// - [`UpdateGameError::TargetTimeInPast`] if `target_time < game.state().time`, indicating that
    ///   the requested update lies in the past.
    pub fn update(
        &mut self,
        mut target_time: InGameTime,
        mut player_input: Option<Input>,
    ) -> Result<NotificationFeed, UpdateGameError> {
        if target_time < self.state.time {
            // Do not allow updating if target time lies in the past.
            return Err(UpdateGameError::TargetTimeInPast);
        } else if self.has_ended() {
            // Do not allow updating a game that has already ended.
            return Err(UpdateGameError::AlreadyEnded);
        }

        let mut feed = Vec::new();

        if player_input.is_some() {
            self.run_mods(
                Hook::ReceivePlayerInput(&mut target_time, &mut player_input),
                &mut feed,
            );
        }

        // We linearly process all events until we reach the targeted update time.
        loop {
            // Check if game should end.
            if !self.has_ended() {
                self.run_mods(Hook::CheckGameLimitsPre, &mut feed);
                'check_game_limits: {
                    let (stat, is_win) = if let Some((line_limit, is_win)) =
                        self.config.game_limits.lines_cleared
                        && line_limit <= self.state.lineclears
                    {
                        (Stat::LinesCleared(line_limit), is_win)
                    } else if let Some((points_limit, is_win)) =
                        self.config.game_limits.points_scored
                        && points_limit <= self.state.points
                    {
                        (Stat::PointsScored(points_limit), is_win)
                    } else if let Some((pieces_limit, is_win)) =
                        self.config.game_limits.pieces_locked
                        && pieces_limit <= self.state.pieces_locked.iter().sum()
                    {
                        (Stat::PiecesLocked(pieces_limit), is_win)
                    } else if let Some((time_limit, is_win)) = self.config.game_limits.time_elapsed
                        && time_limit <= self.state.time
                    {
                        // FIXME: We actually end the game only *after* the first event which updates the game beyond the time limit.
                        // A different way would be to end the game *exactly* at the time limit and before processing such an event,
                        // but that would seem to require more complicated logic.
                        (Stat::TimeElapsed(time_limit), is_win)
                    } else {
                        break 'check_game_limits;
                    };

                    self.phase = Phase::GameEnd {
                        cause: GameEndCause::Limit(stat),
                        is_win,
                    };
                }
                self.run_mods(Hook::CheckGameLimitsPost, &mut feed);
            }

            // Except for the case where game phase has updated to have Ended, the upcoming match branches will all progress state and time.
            if !self.has_ended() {
                self.run_mods(Hook::ProgressTimeStatePre(&mut target_time), &mut feed);
            }

            match self.phase {
                // Game ended by now.
                // Return immediately and with accumulated messages.
                Phase::GameEnd { ref cause, is_win } => {
                    // Add message that game ended.
                    feed.push((
                        Notification::GameEnded {
                            cause: cause.clone(),
                            is_win,
                        },
                        self.state.time,
                    ));
                    self.run_mods(Hook::GameEnded, &mut feed);
                    return Ok(feed);
                }

                // Lines clearing.
                // Move on to spawning.
                Phase::ClearingLines {
                    clear_finish_time,
                    point_bonus,
                } if clear_finish_time <= target_time => {
                    self.run_mods(Hook::LinesClearPre(&mut target_time), &mut feed);
                    self.phase =
                        do_lines_clearing(&self.config, &mut self.state, clear_finish_time);
                    self.state.points += point_bonus;
                    self.state.time = clear_finish_time;
                    self.run_mods(Hook::LinesClearPost, &mut feed);
                }

                // Piece spawning.
                // - May move on to game over (BlockOut).
                // - Normally: Move on to piece-in-play.
                Phase::Spawning { spawn_time } if spawn_time <= target_time => {
                    self.run_mods(Hook::SpawnPre(&mut target_time), &mut feed);
                    self.phase = do_spawn(&self.config, &mut self.state, spawn_time);
                    self.state.time = spawn_time;
                    self.run_mods(Hook::SpawnPost, &mut feed);
                }

                // Piece being manipulated by player.
                Phase::PieceInPlay {
                    piece,
                    autoshift_scheduled,
                    fall_or_lock_time,
                    lock_cap_time: lock_time_cap,
                    lowest_y,
                } if let Some(input) = player_input
                    && target_time <= fall_or_lock_time
                    && autoshift_scheduled
                        .is_none_or(|autoshift_time| target_time <= autoshift_time) =>
                {
                    self.run_mods(Hook::PlayerActionPre(input, &mut target_time), &mut feed);
                    // Make sure the input cannot be processed again in this same update call.
                    player_input.take();
                    let updated_active_buttons =
                        calc_updated_active_buttons(self.state.active_buttons, input, target_time);

                    self.phase = do_player_input(
                        &self.config,
                        &mut self.state,
                        piece,
                        autoshift_scheduled,
                        fall_or_lock_time,
                        lock_time_cap,
                        lowest_y,
                        input,
                        target_time,
                        &mut feed,
                        &updated_active_buttons,
                    );
                    self.state.time = target_time;
                    self.state.active_buttons = updated_active_buttons;
                    self.run_mods(Hook::PlayerActionPost(input), &mut feed);
                }

                // Piece shifting autonomously.
                Phase::PieceInPlay {
                    piece,
                    autoshift_scheduled: Some(autoshift_time),
                    fall_or_lock_time,
                    lock_cap_time,
                    lowest_y,
                } if autoshift_time <= target_time && autoshift_time <= fall_or_lock_time => {
                    self.run_mods(Hook::AutoShiftPre(&mut target_time), &mut feed);
                    self.phase = do_autonomous_shift(
                        &self.config,
                        &mut self.state,
                        piece,
                        autoshift_time,
                        fall_or_lock_time,
                        lock_cap_time,
                        lowest_y,
                    );
                    self.state.time = autoshift_time;
                    self.run_mods(Hook::AutoShiftPost, &mut feed);
                }

                // Piece falling.
                Phase::PieceInPlay {
                    piece,
                    autoshift_scheduled,
                    fall_or_lock_time: fall_time,
                    lock_cap_time,
                    lowest_y,
                } if fall_time <= target_time && piece.is_airborne(&self.state.board) => {
                    self.run_mods(Hook::FallPre(&mut target_time), &mut feed);
                    self.phase = do_fall(
                        &self.config,
                        &mut self.state,
                        piece,
                        autoshift_scheduled,
                        fall_time,
                        lock_cap_time,
                        lowest_y,
                    );
                    self.state.time = fall_time;
                    self.run_mods(Hook::FallPost, &mut feed);
                }

                // Piece locking.
                Phase::PieceInPlay {
                    piece,
                    autoshift_scheduled: _,
                    fall_or_lock_time: lock_time,
                    lock_cap_time: _,
                    lowest_y: _,
                } if lock_time <= target_time => {
                    self.run_mods(Hook::LockPre(&mut target_time), &mut feed);
                    self.phase =
                        do_lock(&self.config, &mut self.state, piece, lock_time, &mut feed);
                    self.state.time = lock_time;
                    self.run_mods(Hook::LockPost, &mut feed);
                }

                // No actions within update target horizon, stop updating.
                // Return from update due to target time reached.
                _ => {
                    // Ensure states are updated.
                    // NOTE: Ensure buttons are still updated by inputs as requested,
                    // even when `PieceInPlay` case was not triggered (e.g. during `LinesClearing`).
                    if let Some(input) = player_input {
                        self.state.active_buttons = calc_updated_active_buttons(
                            self.state.active_buttons,
                            input,
                            target_time,
                        );
                    }
                    // Ensure time is updated as requested, even when none of above cases triggered.
                    // NOTE: This *might* be redundant in some cases.
                    self.state.time = target_time;
                    self.run_mods(Hook::ProgressTimeStatePost, &mut feed);
                    return Ok(feed);
                }
            }

            self.run_mods(Hook::ProgressTimeStatePost, &mut feed);
        }
    }
}

fn do_spawn<TetGen: TetrominoGenerator, PceRot: PieceRotator>(
    config: &Configuration<PceRot>,
    state: &mut State<TetGen>,
    spawn_time: InGameTime,
) -> Phase {
    // Take a tetromino.
    let next_tetromino = state.tetromino_preview.pop_front().unwrap_or_else(|| {
        state
            .tetromino_generator
            .using_rng(&mut state.rng)
            .next()
            .expect("piece generator empty before game end")
    });

    // Only put back in if necessary (e.g. if piece_preview_count < next_pieces.len()).
    state.tetromino_preview.extend(
        state.tetromino_generator.using_rng(&mut state.rng).take(
            config
                .generate_piece_preview
                .saturating_sub(state.tetromino_preview.len()),
        ),
    );

    // 'Raw' spawn piece, before remaining prespawn_actions are applied.
    let mut initial_piece = next_tetromino.spawn_piece();

    /* We do not currently allow 'arbitrary' initial actions, because
    this forces us to impose an equally arbitrary ordering on this set of actions which should happen 'simultaneously'
    in an single instant. What we have currently works like this:
    1. Raw initial spawn: Position piece.
    2. Initial 'Hold': Short-circuit rest of spawn phase since (no further sequencing, move on to next phase).
    3. Initial 'Rotate': Use rotation system to rotate piece as if in free space.
        * Note: We could also rotate on the actual board so more complex kicks get triggered.
          But this interacts increasingly weirdly in some situations as well as the rest of the initial actions routine.
    4. Initial 'Teleport': We try to reposition piece in leftmost/rightmost spot it can fit on the board, and otherwise leave it.
    5. If piece ended up in a fitting position -> Spawn. Otherwise: -> Blockout!

    FIXME: Other Initial systems to consider:
    - Initial 'Move': Happens before or after Rotate? What if it fails?
    - Initial 'Drop' (soft/hard/sonic(=teleport down)): ?...
    */

    // Optionally apply initial actions to spawn piece.
    if config.allow_spawn_manipulation {
        // "Initial Hold System".
        if state.active_buttons[Button::HoldPiece].is_some()
            && let Some(next_phase) = try_do_hold(state, next_tetromino, spawn_time)
        {
            return next_phase;
        }

        // "Initial Rotation System".
        let mut turns = 0;
        if state.active_buttons[Button::RotateLeft].is_some() {
            turns -= 1;
        }
        if state.active_buttons[Button::Rotate180].is_some() {
            turns += 2;
        }
        if state.active_buttons[Button::RotateRight].is_some() {
            turns += 1;
        }
        initial_piece = config.rotation_system.free_rotate(&initial_piece, turns);

        // "Initial Move System".
        let (move_l, move_r) = (
            state.active_buttons[Button::TeleLeft],
            state.active_buttons[Button::TeleRight],
        );
        if move_l != move_r {
            let dx = if move_l > move_r { -1 } else { 1 };
            initial_piece = initial_piece.offset((dx, 0));
        }

        // "Initial Teleport System".
        let (tele_l, tele_r) = (
            state.active_buttons[Button::TeleLeft],
            state.active_buttons[Button::TeleRight],
        );
        if tele_l != tele_r {
            // FIXME: Aw hell naw 💀 do we really need to pull in all of `itertools` just for conditionally `.rev()`ersing a range https://stackoverflow.com/questions/59467882/how-do-i-make-a-range-reverse-on-condition
            let xs: Vec<_> = if tele_l > tele_r {
                (0..WIDTH).collect()
            } else {
                (0..WIDTH).rev().collect()
            };

            // Search for different position piece might fit.
            for x in xs {
                let tele_piece = Piece {
                    position: (x as isize, initial_piece.position.1),
                    ..initial_piece
                };
                if tele_piece.fits_on(&state.board) {
                    initial_piece = tele_piece;
                    break;
                }
            }
        }
    }

    // Detect BlockOut.
    if !initial_piece.fits_on(&state.board) {
        return Phase::GameEnd {
            cause: GameEndCause::BlockOut {
                blocked_piece: initial_piece,
            },
            is_win: false,
        };
    }

    // We're falling if piece could move down.
    let is_airborne = initial_piece.is_airborne(&state.board);

    let initial_fall_or_lock_time = spawn_time.saturating_add(if is_airborne {
        // Fall immediately.
        InGameTime::ZERO
    } else {
        state.lock_delay.saturating_duration()
    });

    // Piece just spawned, lowest y = initial y.
    let initial_lowest_y = initial_piece.position.1;

    // Piece just spawned, standard full lock time max.
    let initial_lock_cap_time = spawn_time.saturating_add(
        state
            .lock_delay
            .mul_ennf64(config.lock_reset_cap_factor)
            .saturating_duration(),
    );

    // Properly schedule move after spawning, depending on how long move button has been active.
    let initial_autoshift_scheduled = if let Some((_, dir_active_since, is_teleport)) =
        calc_isleftshift_activesince_isteleport(&state.active_buttons)
    {
        Some(calc_next_autoshift_time(
            config,
            state,
            spawn_time,
            dir_active_since,
            is_teleport,
            is_airborne,
        ))
    } else {
        None
    };

    Phase::PieceInPlay {
        piece: initial_piece,
        autoshift_scheduled: initial_autoshift_scheduled,
        fall_or_lock_time: initial_fall_or_lock_time,
        lock_cap_time: initial_lock_cap_time,
        lowest_y: initial_lowest_y,
    }
}

#[allow(clippy::too_many_arguments)]
fn do_player_input<TetGen, PceRot: PieceRotator>(
    config: &Configuration<PceRot>,
    state: &mut State<TetGen>,
    previous_piece: Piece,
    previous_autoshift_scheduled: Option<InGameTime>,
    previous_fall_or_lock_time: InGameTime,
    previous_lock_cap_time: InGameTime,
    previous_lowest_y: isize,
    input: Input,
    input_time: InGameTime,
    feed: &mut NotificationFeed,
    updated_active_buttons: &ButtonsState,
) -> Phase {
    /*
    # Overview

    The complexity of various subparts in this function are ranked roughly:
       1. Figuring out movement and future movement (scheduling / preparing autonomous piece updates).
       2. Figuring out falling and locking (scheduling / preparing autonomous piece updates).
       3. All other immediate button changes (easy).


    # Analysis of nontrivial autonomous-event updates (`PieceData.fall_or_lock_time`, `PieceData.move_scheduled`).

    ## [¹] Falling

    The fall timer is influenced as follows:
    - refreshed falltimer  if  (grounded ~> airborne)
    - refreshed falltimer  if  ( _______ ~> airborne) + soft drop just pressed
    - refreshed falltimer  if  ( _______ ~> airborne) + soft drop just released
    - [previous falltimer  if  (airborne ~> airborne) + not in above cases]

    ## [²] Locking

    The lock timer is influenced as follows:
    -      zero locktimer  if  (grounded ~> grounded) + soft drop just pressed
    -      zero locktimer  if  ( _______ ~> grounded) + hard drop just pressed
    - refreshed locktimer  if  ( _______ ~> grounded) + (position|orientation) just changed
    - [previous locktimer  if  (grounded ~> grounded) + not in above cases]

    ## [³] Moving

    We do a complete case analysis. Relevant information to note is:
    * Our *previous state* contains variables `l`, `r`, which store whether the
      left or right movement button (respectively) is active and if so, the time
      it was activated.
      - Concerning our state, we also care about the different cases of
        how `l` and `r` compare, i.e. which activation came first.
    * Our *input* consists of a left/right release/press button change at a given
      in-game time. The in-game time is only relevant to update the button state.
    * Our *next state* consists of the respective `l`/`r` updated.
      Interesting cases here consist of activations when button was already activated
      previously without a deactivation input.
    * Our *other effects* consist of the immediate movement we might want to accomplish.

    Goals:
    * We want to understand when an immediate move is actually performed.
      It turns out this is if and only if a move activation input is received.
    * We want to understand when auto-move time needs to be set anew.
      This can be seen in Table (⁵) in entries marked (ˢ).
    * We want to understand when auto-move time needs to be canceled.
      This can be seen in Table (⁵) in entries marked (ᶜ) and coincides with
      on last move button deactivated.
    * We want to understand when auto-move time needs to be kept as-is.
      This can be seen in Table (⁵) in entries not marked with (ˢ) or (ᶜ).
      Of note are entries where movement is happening still.

    Table (⁵) details all interactions we desire in every single case.
    Example readings:
    * `l  r` `Actv.L`: Left and right are inactive and we active left.
      We end up in `L  r` `←+ ·ˢ` where only left is active and we need to initiate
      movement to the left.
    * `L <R` `Deact.l`: Left and right are both active -- right activated after left,
      so we are moving right -- and we deactivate left.
      We end up in `l  R` `·  →` where only right is active and we just keep moving
      to the right.

    [⁵] Table: "Karnaugh map".
    +------------+----------------------------------------------+
    |            |     Deact_l   Deact_r   Actv_R    Actv_L     |
    + Prev.state +----------------------------------------------+
    |            |                                              |
    |    l  r    |      l  r      l  r      l  R      L  r      |
    |    ·  ·    |      ·  ·      ·  ·      · +→ˢ     ←+ ·ˢ     |
    |            |                                              |
    |    L  r    |      l  r      L  r      L  R      L  r      |
    |    ←  ·    |      ←- ·ᶜ     ←  ·      ←-+→ˢ     ←+ ·ˢ     |
    |            |                                              |
    |    L> R    |      l  R      L  r      L  R      L  R      |
    |    ←  ·    |      ←-+→ˢ     ←  ·      ←-+→ˢ     ←+ ·ˢ     |
    |            |                                              |
    |    L==R    |      l  R      L  r      L  R      L  R      |
    |    ·  ·    |      · +→ˢ     ←+ ·ˢ     · +→ˢ     ←+ ·ˢ     |
    |            |                                              |
    |    L <R    |      l  R      L  r      L  R      L  R      |
    |    ·  →    |      ·  →      ←+-→ˢ     · +→ˢ     ←+-→ˢ     |
    |            |                                              |
    |    l  R    |      l  R      l  r      l  R      L  R      |
    |    ·  →    |      ·  →      · -→ᶜ     · +→ˢ     ←+-→ˢ     |
    |            |                                              |
    +------------+----------------------------------------------+

    The table has nontrivial complexity but from it we can derive expressions to
    for the things we actually care about (convention: `lrLR` refer to old state):
    * (ˢ) *Setting auto-move time*:
          Actv_R  ||  Actv_L  ||  L==R  ||  L>R !Deact_r  ||  L<R !Deact_l
      Or:
          !(Deact_l !L>=R || Deact_r !L=<R)
    * (ᶜ) *Canceling auto-move time*:
          L r Deact_l  ||  r L Deact_l
    * *Performing immediate move*; Same as (ˢ)!

    ### Move Resumption [⁴]

    We *also* want to allow a player to hold 'move' while a piece is stuck, in a way where
    the piece should move immediately as soon as it is unstuck (e.g. once fallen below the obstruction).
    * This system takes effect in the non-(ˢ)-(ᶜ)-entries of Table (⁵).
    * However, it has to be computed after another event has been handled that may be cause of unobstruction.

    */

    // Prepare to maybe change the move_scheduled.
    let mut autoshift_sentinel: Option<bool> = None;

    let mut updated_piece = previous_piece;
    use {Button as B, Input as I};
    match input {
        // Hold.
        // - If succeeds, changes game action state to spawn different piece.
        // - Otherwise does nothing.
        I::Activate(B::HoldPiece) => {
            if let Some(next_phase) = try_do_hold(state, updated_piece.tetromino, input_time) {
                return next_phase;
            }
        }

        // Soft Drop.
        // Instantly try to move piece one tile down.
        // The locking is handled as part of a different check/system further.
        I::Activate(B::DropSoft) => {
            if let Ok(fallen_piece) = updated_piece.offset_on(&state.board, (0, -1)) {
                updated_piece = fallen_piece;
            }
        }

        // Hard Drop.
        // Instantly try to move piece all the way down.
        // The locking is handled as part of a different check/system further.
        I::Activate(B::DropHard) => {
            updated_piece = updated_piece.teleported(&state.board, (0, -1));

            if config.send_notifications {
                feed.push((
                    Notification::HardDrop {
                        height_dropped: previous_piece
                            .position
                            .1
                            .abs_diff(updated_piece.position.1),
                        dropped_piece: updated_piece,
                    },
                    input_time,
                ));
            }
        }

        // Teleport down / 'Sonic drop'.
        // This is treated as Soft drop but only consisting of 0s-fall-delay, gravity-driven falls.
        I::Activate(B::TeleDown) => {}

        // Sideways teleports.
        // Just instantly try to move piece all the way into applicable sideways direction.
        I::Activate(tele_sideways @ (B::TeleLeft | B::TeleRight)) => {
            let offset = match tele_sideways {
                B::TeleLeft => (-1, 0),
                B::TeleRight => (1, 0),
                _ => unreachable!(),
            };

            updated_piece = updated_piece.teleported(&state.board, offset);
        }

        // Rotates.
        // Just instantly try to rotate piece into applicable direction.
        I::Activate(rotate @ (B::RotateLeft | B::RotateRight | B::Rotate180)) => {
            let right_turns = match rotate {
                B::RotateLeft => -1,
                B::RotateRight => 1,
                B::Rotate180 => 2,
                _ => unreachable!(),
            };

            if let Some(rotated_piece) =
                config
                    .rotation_system
                    .rotate(&updated_piece, &state.board, right_turns)
            {
                updated_piece = rotated_piece;
            }
        }

        // Movement.
        // This is relatively complicated; The logic is based on the comment in (³).
        I::Activate(B::MoveLeft | B::MoveRight) | I::Deactivate(B::MoveLeft | B::MoveRight) => {
            // Actually move piece.
            if let Input::Activate(button) = input {
                let dx = if matches!(button, Button::MoveLeft) {
                    -1
                } else {
                    1
                };
                if let Ok(moved_piece) = updated_piece.offset_on(&state.board, (dx, 0)) {
                    updated_piece = moved_piece;
                }
            }

            let prev_l = state.active_buttons[B::MoveLeft];
            let prev_r = state.active_buttons[B::MoveRight];
            let (l, r) = (prev_l.is_some(), prev_r.is_some());

            // *Setting auto-move time* (alt): !(Deact_L !L>=R || Deact_R !L=<R)
            let reschedule_autoshift = {
                let a = matches!(input, I::Deactivate(B::MoveLeft)) && !(r && prev_l >= prev_r);
                let b = matches!(input, I::Deactivate(B::MoveRight)) && !(l && prev_l <= prev_r);
                !(a || b)
            };

            // NOTE: Abandoned code.
            // The following commented code originally served to explicitly get rid of scheduled auto-shifts
            // in case every mvmt button was released. However, in the new system we compute the
            // 'direction of current movement' dynamically from the button state. So if there are
            // No buttons pressed anymore we do not even proceed further, and delete any auto-mvmt.
            //
            // *Canceling auto-move time*: L r Deact_l  ||  r L Deact_L
            // let cancel_autoshift = {
            //     let a = l && !r && matches!(input, I::Deactivate(B::MoveLeft));
            //     let b = !l && r && matches!(input, I::Deactivate(B::MoveRight));
            //     a || b
            // };

            autoshift_sentinel = Some(reschedule_autoshift);
        }

        I::Deactivate(B::TeleLeft | B::TeleRight) => {
            // FIXME: This is necessary to handle niche cases. E.g., try pressing the following, in order (under 0msARR): ML, MR, TL, TR.
            let reschedule_autoshift = true;
            // FIXME: Abandoned code.
            // let cancel_autoshift = false;
            autoshift_sentinel = Some(reschedule_autoshift);
        }

        // Various button releases.
        // These don't have any direct effect (move, rotate) on the `piece` in themselves.
        I::Deactivate(
            B::RotateLeft
            | B::RotateRight
            | B::Rotate180
            | B::DropSoft
            | B::DropHard
            | B::TeleDown
            | B::HoldPiece,
        ) => {}
    }

    // Epilogue. Finalize state updates.

    // Immutable.
    let updated_piece = updated_piece;

    // Update `lowest_y`, re-set `lock_time_cap` if applicable.
    let (updated_lowest_y, updated_lock_cap_time) = if matches!(input, I::Activate(B::DropHard)) {
        (previous_lowest_y.min(updated_piece.position.1), input_time)
    } else if updated_piece.position.1 < previous_lowest_y {
        // Refresh position and lock_time_cap.
        (
            updated_piece.position.1,
            input_time.saturating_add(
                state
                    .lock_delay
                    .mul_ennf64(config.lock_reset_cap_factor)
                    .saturating_duration(),
            ),
        )
    } else {
        (previous_lowest_y, previous_lock_cap_time)
    };

    let previous_is_airborne = previous_piece.is_airborne(&state.board);
    let updated_is_airborne = updated_piece.is_airborne(&state.board);

    // Update falltimer and locktimer. See (¹) and (²).
    let updated_fall_or_lock_time = if updated_is_airborne {
        // Calculate scheduled fall time. See (¹).
        let fall_reset = !previous_is_airborne
            || matches!(
                input,
                I::Activate(B::TeleDown | B::DropSoft) | I::Deactivate(B::DropSoft)
            );
        if fall_reset {
            // Refresh fall timer if we *started* falling, or soft drop just pressed, or soft drop just released.}
            let use_delayed_soft_drop = matches!(input, I::Activate(B::DropSoft));
            calc_next_fall_time(
                state,
                config,
                input_time,
                updated_active_buttons,
                use_delayed_soft_drop,
            )
        } else {
            // Falling as before.
            previous_fall_or_lock_time
        }
    } else {
        // Calculate scheduled lock time. See (²).
        let lock_immediately = matches!(input, I::Activate(B::DropHard))
            || (!previous_is_airborne && matches!(input, I::Activate(B::DropSoft)));
        let lock_reset_piecechange = updated_piece != previous_piece;
        let lock_reset_lenience = config.allow_lenient_lock_reset
            && matches!(
                input,
                I::Activate(
                    B::MoveLeft
                        | B::MoveRight
                        | B::RotateLeft
                        | B::Rotate180
                        | B::RotateRight
                        | B::TeleLeft
                        | B::TeleDown
                        | B::TeleRight
                )
            );

        if lock_immediately {
            // We are on the ground - if hard drop pressed or soft drop when ground is touched, lock immediately.
            input_time
        } else if lock_reset_lenience || lock_reset_piecechange {
            // On the ground - Refresh lock time if piece moved.
            // NOTE: lock_time_cap may actually lie in the past, so we first need to cap *it* from below (current time)!
            input_time
                .max(updated_lock_cap_time)
                .min(input_time.saturating_add(state.lock_delay.saturating_duration()))
        } else {
            // Previous lock time.
            previous_fall_or_lock_time
        }
    };

    // Update movetimer and rest of movement stuff.
    // See also (³).
    let updated_autoshift_scheduled = 'exp: {
        // After a hard drop, we want to lock immediately and cancel any auto-shifts.
        if matches!(input, I::Activate(B::DropHard)) {
            break 'exp None;
        }

        let Some((is_shifting_left, dir_active_since, is_teleport)) =
            calc_isleftshift_activesince_isteleport(updated_active_buttons)
        else {
            // No sensible movement information received, cancel autoshift.
            break 'exp None;
        };

        let next_autoshift_time = calc_next_autoshift_time(
            config,
            state,
            input_time,
            dir_active_since,
            is_teleport,
            updated_is_airborne,
        );

        // Handle case where movement-related input was handled.
        if let Some(reschedule_autoshift) = autoshift_sentinel {
            if reschedule_autoshift {
                // Handle the case where we need to ensure the new auto-shift happens before lock.
                if config.ensure_shift_delay_lt_lock_delay
                    && !updated_is_airborne
                    && next_autoshift_time > updated_fall_or_lock_time
                {
                    break 'exp Some(updated_fall_or_lock_time);
                }
                // Reschedule autonomous movement (normally).
                break 'exp Some(next_autoshift_time);
            }

            // FIXME: Abandoned code.
            // if cancel_autoshift {
            //     // Buttons deactivated; Cancel autonomous movement.
            //     break 'exp None; // Buttons unpressed: Remove autonomous movement.
            // }

            // No relevant movement changes caused by mvmt-related button input: Don't do anything.
            break 'exp previous_autoshift_scheduled;
        }

        // Otherwise wasn't a move-related action.

        // Check if piece fell down and can immediately move
        let dx = if is_shifting_left { -1 } else { 1 };
        if check_piece_became_newly_movable(previous_piece, updated_piece, &state.board, dx) {
            // Due to the system mentioned in (⁴), we check
            // if the piece was stuck and became unstuck, and insert an autonomous move.
            if matches!(input, I::Activate(B::DropSoft)) {
                // If it was just a fall, make it immediate.
                break 'exp Some(input_time);
            } else {
                // Otherwise...
                break 'exp Some(next_autoshift_time);
            }
        }

        // If piece had an automatic move scheduled,
        // and now landed
        // and is trying to auto-shift
        // and is about to lock and it would lock *before* the autoshift triggers;
        // Then ensure the shift is truncated to be faster.
        if config.ensure_shift_delay_lt_lock_delay
            && previous_is_airborne
            && !updated_is_airborne
            && let Some(previous_autoshift_time) = previous_autoshift_scheduled
            && previous_autoshift_time > updated_fall_or_lock_time
        {
            break 'exp Some(updated_fall_or_lock_time);
        }

        // All checks passed, no changes need to be made.
        // This is the case where neither (³) or (⁴) apply.
        previous_autoshift_scheduled
    };

    // 'Update' ActionState;
    // Return it to the main state machine with the latest acquired piece data.
    Phase::PieceInPlay {
        piece: updated_piece,
        autoshift_scheduled: updated_autoshift_scheduled,
        fall_or_lock_time: updated_fall_or_lock_time,
        lock_cap_time: updated_lock_cap_time,
        lowest_y: updated_lowest_y,
    }
}

fn try_do_hold<TetGen>(
    state: &mut State<TetGen>,
    tetromino: Tetromino,
    next_spawn_time: InGameTime,
) -> Option<Phase> {
    match state.tetromino_held {
        // Nothing held yet, just hold spawned tetromino.
        None => {
            state.tetromino_held = Some((tetromino, false));
            // Issue a spawn.
            Some(Phase::Spawning {
                spawn_time: next_spawn_time,
            })
        }
        // Swap spawned tetromino, push held back into next pieces queue.
        Some((held_tet, true)) => {
            state.tetromino_held = Some((tetromino, false));
            // Cause the next spawn to specially be the piece we held.
            state.tetromino_preview.push_front(held_tet);
            // Issue a spawn.
            Some(Phase::Spawning {
                spawn_time: next_spawn_time,
            })
        }
        // Else can't hold, don't do anything.
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn do_autonomous_shift<TetGen, PceRot>(
    config: &Configuration<PceRot>,
    state: &mut State<TetGen>,
    previous_piece: Piece,
    autoshift_time: InGameTime,
    previous_fall_or_lock_time: InGameTime,
    previous_lock_cap_time: InGameTime,
    previous_lowest_y: isize,
) -> Phase {
    // Move piece and update all appropriate piece-related values.

    let (updated_piece, updated_is_airborne, updated_autoshift_scheduled) = 'exp: {
        let Some((is_left_shift, dir_active_since, is_teleport)) =
            calc_isleftshift_activesince_isteleport(&state.active_buttons)
        else {
            // No sensible movement information received.
            break 'exp (
                previous_piece,
                previous_piece.is_airborne(&state.board),
                None,
            );
        };

        let dx = if is_left_shift { -1 } else { 1 };
        if let Ok(moved_piece) = previous_piece.offset_on(&state.board, (dx, 0)) {
            let updated_piece = moved_piece;
            // Able to do relevant move; Insert autonomous movement.
            let is_airborne = updated_piece.is_airborne(&state.board);
            let autoshift_time = calc_next_autoshift_time(
                config,
                state,
                autoshift_time,
                dir_active_since,
                is_teleport,
                is_airborne,
            );

            break 'exp (updated_piece, is_airborne, Some(autoshift_time));
        }

        // Unable to move; Remove autonomous movement.
        (
            previous_piece,
            previous_piece.is_airborne(&state.board),
            None,
        )
    };

    // Horizontal move could not have affected height, so it stays the same!
    let updated_lowest_y = previous_lowest_y;
    let updated_lock_cap_time = previous_lock_cap_time;

    let updated_fall_or_lock_time = if updated_is_airborne {
        // Calculate scheduled fall time. See (¹).
        let was_grounded = !previous_piece.is_airborne(&state.board);

        if was_grounded {
            // Refresh fall timer if we *started* falling.
            calc_next_fall_time(state, config, autoshift_time, &state.active_buttons, false)
        } else {
            // Falling as before.
            previous_fall_or_lock_time
        }
    } else {
        // Calculate schedule lock time.
        // Only update lock time if piece changed.
        let lock_reset_piecechange = updated_piece != previous_piece;
        if lock_reset_piecechange {
            // NOTE: updated_lock_time_cap may actually lie in the past, so we first need to cap *it* from below (current time)!
            autoshift_time
                .max(updated_lock_cap_time)
                .min(autoshift_time.saturating_add(state.lock_delay.saturating_duration()))
        } else {
            previous_fall_or_lock_time
        }
    };

    // Update 'ActionState';
    // Return it to the main state machine with the latest acquired piece data.
    Phase::PieceInPlay {
        piece: updated_piece,
        autoshift_scheduled: updated_autoshift_scheduled,
        fall_or_lock_time: updated_fall_or_lock_time,
        lock_cap_time: updated_lock_cap_time,
        lowest_y: updated_lowest_y,
    }
}

fn do_fall<TetGen, PceRot>(
    config: &Configuration<PceRot>,
    state: &mut State<TetGen>,
    previous_piece: Piece,
    previous_autoshift_scheduled: Option<InGameTime>,
    fall_time: InGameTime,
    previous_lock_cap_time: InGameTime,
    previous_lowest_y: isize,
) -> Phase {
    /*
    # Overview

    The complexity of various subparts in this function are ranked roughly:
       1. Falling - due to how it is sometimes falling *and* moving *and then* updating falling/locking info.
       2. Moving - due to how it is mostly a single movement + updating falling/locking info.
       3. Locking - due to how simple it is if it happens.

    # Analysis of nontrivial autonomous-event updates (`PieceData.fall_or_lock_time`, `PieceData.move_scheduled`).

    ## Falling

    The fall timer is influenced as follows¹:
    - immediate fall + refreshed falltimer  if  fell
    - refreshed falltimer  if  (grounded ~> airborne)ᵃ
    - [old falltimer  if  not in above cases]

    ## Locking

    The lock timer is influenced as follows²:
    - immediate lock  if  locked
    - refreshed locktimer  if  (airborne ~> grounded)ᵇ
    - [old locktimer  if  not in above cases]

    ## Moving

    The move timer is influenced as follows³:
    - immediate move + some refreshed movetimer  if  moved
    - no movetimer  if  move not possible
    - [old movetimer  if  not in above cases]

    ### Move Resumption

    We *also* want to allow a player to hold 'move' while a piece is stuck, in a way where
    the piece should move immediately as soon as it is unstuck⁴ (e.g. once fallen below the obstruction).
    However, it has to be computed after another event has been handled that may be cause of unobstruction.
    */

    // Drop piece and update all appropriate piece-related values.
    let updated_piece = if let Ok(fallen_piece) = previous_piece.offset_on(&state.board, (0, -1)) {
        fallen_piece
    } else {
        // Piece could not fall. Return previous.
        previous_piece
    };

    let (updated_lowest_y, updated_lock_cap_time) = if updated_piece.position.1 < previous_lowest_y
    {
        // Refresh position and lock_time_cap.
        (
            updated_piece.position.1,
            fall_time.saturating_add(
                state
                    .lock_delay
                    .mul_ennf64(config.lock_reset_cap_factor)
                    .saturating_duration(),
            ),
        )
    } else {
        (previous_lowest_y, previous_lock_cap_time)
    };

    let previous_is_airborne = previous_piece.is_airborne(&state.board);
    let updated_is_airborne = updated_piece.is_airborne(&state.board);

    let updated_fall_or_lock_time = if updated_is_airborne {
        calc_next_fall_time(state, config, fall_time, &state.active_buttons, false)
    } else {
        // NOTE: lock_time_cap may actually lie in the past, so we first need to cap *it* from below (current time)!
        fall_time
            .max(updated_lock_cap_time)
            .min(fall_time.saturating_add(state.lock_delay.saturating_duration()))
    };

    let updated_autoshift_scheduled = 'exp: {
        let Some((is_left_shift, _dir_active_since, _is_teleport)) =
            calc_isleftshift_activesince_isteleport(&state.active_buttons)
        else {
            // No sensible movement information received, cancel autoshift.
            break 'exp None;
        };

        let dx = if is_left_shift { -1 } else { 1 };
        if check_piece_became_newly_movable(previous_piece, updated_piece, &state.board, dx) {
            // Due to the system mentioned in (⁴), we check
            // if the piece was stuck and became unstuck, and insert an immediate autonomous move.
            break 'exp Some(fall_time);
        }

        // If piece had an automatic move scheduled and now landed and is about to lock and would lock *before* the autshift triggers, ensure the shift is truncated to be faster.
        if config.ensure_shift_delay_lt_lock_delay
            && previous_is_airborne
            && !updated_is_airborne
            && let Some(previous_autoshift_time) = previous_autoshift_scheduled
            && previous_autoshift_time > updated_fall_or_lock_time
        {
            break 'exp Some(updated_fall_or_lock_time);
        }

        // No changes need to be made.
        previous_autoshift_scheduled
    };

    // 'Update' ActionState;
    // Return it to the main state machine with the latest acquired piece data.
    Phase::PieceInPlay {
        piece: updated_piece,
        autoshift_scheduled: updated_autoshift_scheduled,
        fall_or_lock_time: updated_fall_or_lock_time,
        lock_cap_time: updated_lock_cap_time,
        lowest_y: updated_lowest_y,
    }
}

fn do_lock<TetGen, PceRot>(
    config: &Configuration<PceRot>,
    state: &mut State<TetGen>,
    piece: Piece,
    lock_time: InGameTime,
    feed: &mut NotificationFeed,
) -> Phase {
    // Before board is changed, precompute whether a piece was 'spun' into position;
    // - 'Spun' pieces give higher points bonus.
    // - Only locked pieces can yield bonus (i.e. can't possibly move down).
    // - Only locked pieces clearing lines can yield bonus (i.e. can't possibly move left/right).
    // Thus, if a piece cannot move back up at lock time, it must have gotten there by rotation.
    // That's what a 'spin' is.
    let is_spin = piece.offset_on(&state.board, (0, 1)).is_err();

    let any_below_skyline = piece
        .coords()
        .iter()
        .any(|&(_, y)| (y as usize) < LOCK_OUT_HEIGHT);

    // If all minos of the tetromino were locked entirely outside the `SKYLINE` bounding height, it's game over.
    if !any_below_skyline {
        return Phase::GameEnd {
            cause: GameEndCause::LockOut {
                locking_piece: piece,
            },
            is_win: false,
        };
    }

    // Locking.
    for (x, y) in piece.coords() {
        if (0..WIDTH).contains(&(x as usize)) {
            // Ensure line exists.
            if y as usize >= state.board.len() {
                state.board.resize((y + 1) as usize, Default::default());
            }
            // Put tile onto board.
            state.board[y as usize].0[x as usize] = Some(piece.tetromino.into());
        }
    }

    if config.send_notifications {
        feed.push((Notification::PieceLocked { piece }, lock_time));
    }

    // Update tally of pieces_locked.
    state.pieces_locked[piece.tetromino as usize] += 1;

    // Update ability to hold piece.
    if let Some((_held_tet, swap_allowed)) = &mut state.tetromino_held {
        *swap_allowed = true;
    }

    // Points bonus calculation.

    // Find lines which might get cleared by piece locking. (actual clearing done later).
    let mut cleared_lines = Vec::new();
    for (y, (line, is_frozen)) in state.board.iter().enumerate() {
        // Line will be cleared if it isn't frozen and contains no empty tiles anymore.
        if !is_frozen && !line.contains(&None) {
            cleared_lines.push((y, line.map(Option::unwrap)));
        }
    }

    let lineclears = u32::try_from(cleared_lines.len()).unwrap();

    if lineclears == 0 {
        // If no lines cleared, no points bonus and combo is reset.
        state.consecutive_lineclears = 0;

        // 'Update' ActionState;
        // No lines cleared, directly proceed to spawn.
        return Phase::Spawning {
            spawn_time: lock_time.saturating_add(config.spawn_delay),
        };
    }

    // Further calculation.

    // Increase combo.
    state.consecutive_lineclears += 1;

    let combo = state.consecutive_lineclears;

    let is_perfect = state.board.iter().all(|(line, _is_frozen)| {
        line.iter().all(|tile| tile.is_none()) || line.iter().all(|tile| tile.is_some())
    });

    // Compute main Points Bonus.
    let point_bonus =
        lineclears * lineclears * if is_spin { 4 } else { 1 } * if is_perfect { 4 } else { 1 }
            + (combo - 1);
    // let point_bonus = lineclears * if is_spin { 2 } else { 1 } * if is_perfect { 4 } else { 1 } * 2
    //     - 1
    //     + (combo - 1);

    if config.send_notifications {
        feed.push((
            Notification::LinesClearing {
                lines: cleared_lines,
                line_clear_duration: config.line_clear_duration,
            },
            lock_time,
        ));

        feed.push((
            Notification::Accolade {
                point_bonus,
                tetromino: piece.tetromino,
                is_spin,
                lineclears,
                is_perfect,
                combo,
            },
            lock_time,
        ));
    }

    // 'Update' ActionState;
    // Lines must be cleared, enter line clearing state.
    Phase::ClearingLines {
        clear_finish_time: lock_time.saturating_add(config.line_clear_duration),
        point_bonus,
    }
}

fn do_lines_clearing<TetGen, PceRot>(
    config: &Configuration<PceRot>,
    state: &mut State<TetGen>,
    clear_finish_time: InGameTime,
) -> Phase {
    // To delete all lines in one pass, iterate through all height indices from top to bottom.
    for y in (0..state.board.len()).rev() {
        let (line, is_frozen) = &mut state.board[y];
        // Full line: move it to the cleared lines storage and push an empty line to the board.
        if !*is_frozen && line.iter().all(|tile| tile.is_some()) {
            // We remove the line.
            state.board.remove(y);

            state.lineclears += 1;

            // Increment level if update requested.
            if state
                .lineclears
                .is_multiple_of(config.update_delays_every_n_lineclears)
            {
                update_fall_and_lock_delays(config, state);
            }
        }
    }

    Phase::Spawning {
        spawn_time: clear_finish_time.saturating_add(config.spawn_delay),
    }
}

/// Update the fall and lock delay of a game [`State`] according to a given [`Configuration`] (containing delay curves for falling and locking).
pub fn update_fall_and_lock_delays<TetGen, PceRot>(
    config: &Configuration<PceRot>,
    state: &mut State<TetGen>,
) {
    if let Some(hit_at_n_lineclears) = state.fall_delay_lowerbound_hit_at_n_lineclears {
        // Fall delay zero was hit at some point, only possibly decrease lock delay now.

        if let Some(lock_delay_curve) = &config.lock_delay_curve {
            // Actually compute new delay from equation.
            let relevant_lineclears = state.lineclears - hit_at_n_lineclears;
            let (new_lock_delay, _lock_lowerbound_hit) = lock_delay_curve
                .retrieve_and_check(relevant_lineclears, config.update_delays_every_n_lineclears);

            state.lock_delay = new_lock_delay;
        }
    } else {
        // Calculate decreased fall delay and (semi)fixed lock delay as normal.

        // Actually compute new delay from equation.
        let (new_fall_delay, fall_lowerbound_hit) = config
            .fall_delay_curve
            .retrieve_and_check(state.lineclears, config.update_delays_every_n_lineclears);

        // Remember the first time fall delay hit zero.
        if fall_lowerbound_hit {
            state.fall_delay_lowerbound_hit_at_n_lineclears = Some(state.lineclears);
        }

        state.fall_delay = new_fall_delay;

        if let Some(lock_curve) = &config.lock_delay_curve {
            // If lock delay does have its own curve, update lock delay to fall delay if that is longer
            let (lock_delay, _) = lock_curve
                .retrieve_and_check(state.lineclears, config.update_delays_every_n_lineclears);
            state.lock_delay = lock_delay.max(state.fall_delay);
        } else {
            // If lock delay does not have its own curve, it is equal to the fall delay.
            state.lock_delay = state.fall_delay;
        }
    }
}

fn calc_updated_active_buttons(
    mut previous_active_buttons: ButtonsState,
    input: Input,
    input_time: InGameTime,
) -> ButtonsState {
    match input {
        Input::Activate(button) => {
            previous_active_buttons[button] = Some(input_time);
        }
        Input::Deactivate(button) => {
            if matches!(input, Input::Deactivate(Button::MoveLeft))
                && previous_active_buttons[Button::MoveRight].is_some()
            {
                previous_active_buttons[Button::MoveRight] = Some(input_time);
            } else if matches!(input, Input::Deactivate(Button::MoveRight))
                && previous_active_buttons[Button::MoveLeft].is_some()
            {
                previous_active_buttons[Button::MoveLeft] = Some(input_time);
            }
            previous_active_buttons[button] = None;
        }
    }
    previous_active_buttons
}

fn check_piece_became_newly_movable(
    previous_piece: Piece,
    updated_piece: Piece,
    board: &Board,
    dx: isize,
) -> bool {
    let moved_previous_piece = previous_piece.offset_on(board, (dx, 0));
    let moved_updated_piece = updated_piece.offset_on(board, (dx, 0));

    moved_previous_piece.is_err() && moved_updated_piece.is_ok()
}

/// This function may return
/// 1. An bool which is `true` when movement is to the left (and `false` when to the right),
/// 2. and an optional time at which the relevant 'move' direction button had been activated.
///    This option is `None` if the move is caused by an intent to teleport instantly instead.
///
/// It returns `None` when it cannot determine a direction to move to, which happens when:
/// * Both directions were pressed at the exact same in-game time, or
/// * No direction is pressed.
fn calc_isleftshift_activesince_isteleport(
    active_buttons: &ButtonsState,
) -> Option<(bool, InGameTime, bool)> {
    let teleporting_left_and_dir_active_since = match (
        active_buttons[Button::TeleLeft],
        active_buttons[Button::TeleRight],
    ) {
        (Some(time_actvd_left), Some(time_actvd_right)) => {
            match time_actvd_left.cmp(&time_actvd_right) {
                // 'Right' was pressed more recently, go right.
                std::cmp::Ordering::Less => Some((false, time_actvd_right, true)),
                // Both pressed at exact same time, don't move.
                std::cmp::Ordering::Equal => None,
                // 'Left' was pressed more recently, go left.
                std::cmp::Ordering::Greater => Some((true, time_actvd_left, true)),
            }
        }
        // Only 'left' pressed.
        (Some(time_actvd_left), None) => Some((true, time_actvd_left, true)),
        // Only 'right' pressed.
        (None, Some(time_actvd_right)) => Some((false, time_actvd_right, true)),
        // None pressed. No movement.
        (None, None) => None,
    };

    // FIXME: Currently, we only care about teleports if they are active, and so override ARR of auto-move.
    if teleporting_left_and_dir_active_since.is_some() {
        return teleporting_left_and_dir_active_since;
    }

    match (
        active_buttons[Button::MoveLeft],
        active_buttons[Button::MoveRight],
    ) {
        (Some(time_actvd_left), Some(time_actvd_right)) => {
            match time_actvd_left.cmp(&time_actvd_right) {
                // 'Right' was pressed more recently, go right.
                std::cmp::Ordering::Less => Some((false, time_actvd_right, false)),
                // Both pressed at exact same time, don't move.
                std::cmp::Ordering::Equal => None,
                // 'Left' was pressed more recently, go left.
                std::cmp::Ordering::Greater => Some((true, time_actvd_left, false)),
            }
        }
        // Only 'left' pressed.
        (Some(time_actvd_left), None) => Some((true, time_actvd_left, false)),
        // Only 'right' pressed.
        (None, Some(time_actvd_right)) => Some((false, time_actvd_right, false)),
        // None pressed. No movement.
        (None, None) => None,
    }
}

fn calc_next_autoshift_time<TetGen, PceRot>(
    config: &Configuration<PceRot>,
    state: &State<TetGen>,
    current_time: InGameTime,
    dir_active_since: InGameTime,
    shift_is_teleport: bool,
    piece_is_airborne: bool,
) -> InGameTime {
    let mut shift_delay =
        if current_time.saturating_sub(dir_active_since) >= config.delayed_auto_shift {
            if shift_is_teleport {
                InGameTime::ZERO
            } else {
                config.auto_repeat_rate
            }
        } else {
            config.delayed_auto_shift
        };

    if config.ensure_shift_delay_lt_lock_delay
        && !piece_is_airborne
        && let ExtDuration::Finite(lock_delay) = state.lock_delay
        && shift_delay > lock_delay
    {
        // Ensure moves occur faster than locks.
        shift_delay = lock_delay
    }

    current_time.saturating_add(shift_delay)
}

fn calc_next_fall_time<TetGen, PceRot>(
    state: &State<TetGen>,
    config: &Configuration<PceRot>,
    current_time: InGameTime,
    active_buttons: &ButtonsState,
    use_delayed_soft_drop: bool,
) -> InGameTime {
    let fall_delay = if active_buttons[Button::TeleDown].is_some() {
        ExtDuration::ZERO
    } else if active_buttons[Button::DropSoft].is_some() {
        if use_delayed_soft_drop && let Some(delayed_soft_drop) = config.delayed_soft_drop {
            ExtDuration::Finite(delayed_soft_drop).min(state.fall_delay)
        } else {
            match config.soft_drop_rate {
                Either::Left(factor) => state.fall_delay.div_ennf64(factor),
                Either::Right(upperbound) => state.fall_delay.min(upperbound),
            }
        }
    } else {
        state.fall_delay
    };

    current_time.saturating_add(fall_delay.saturating_duration())
}
