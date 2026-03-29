/*!
This module handles what happens when [`Game::update`] is called.
*/

use crate::modding::Hook;

use super::*;

impl Game {
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

            Phase::Spawning { .. } | Phase::LinesClearing { .. } => None,
            Phase::PieceInPlay { piece, .. } => Some(piece),
        };

        self.phase = Phase::GameEnd {
            cause: GameEndCause::Forfeit { piece_in_play },
            is_win: false,
        };

        let mut feed = vec![(Notification::GameEnded { is_win: false }, self.state.time)];

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
                Hook::PlayerInputReceived(&mut target_time, &mut player_input),
                &mut feed,
            );
        }

        // We linearly process all events until we reach the targeted update time.
        loop {
            // Maybe move on to game over if time condition is met now.
            if let Some((time_limit, is_win)) = self.config.game_limits.time_elapsed {
                // FIXME: We actually end the game *after* an event was processed that moved us beyond the time limit.
                // A different way would be to end the game *exactly* at the time limit and before processing such an event,
                // but this requires more complicated logic.
                if time_limit <= self.state.time {
                    self.phase = Phase::GameEnd {
                        cause: GameEndCause::Limit(Stat::TimeElapsed(time_limit)),
                        is_win,
                    };
                }
                self.run_mods(Hook::CheckGameLimitsPost, &mut feed);
            }

            match self.phase {
                // Game ended by now.
                // Return immediately and with accumulated messages.
                Phase::GameEnd { cause: _, is_win } => {
                    // Add message that game ended.
                    feed.push((Notification::GameEnded { is_win }, self.state.time));
                    self.run_mods(Hook::GameEnded, &mut feed);
                    return Ok(feed);
                }

                // Lines clearing.
                // Move on to spawning.
                Phase::LinesClearing {
                    clear_finish_time,
                    score_bonus,
                } if clear_finish_time <= target_time => {
                    self.run_mods(Hook::TimeStateProgressionPre(&mut target_time), &mut feed);
                    self.run_mods(Hook::LinesClearPre(&mut target_time), &mut feed);
                    self.phase =
                        do_lines_clearing(&self.config, &mut self.state, clear_finish_time);
                    self.state.points += score_bonus;
                    self.state.time = clear_finish_time;
                    self.run_mods(Hook::LinesClearPost, &mut feed);
                    self.run_mods(Hook::TimeStateProgressionPost, &mut feed);

                    // Check if game should end.
                    if let Some((line_limit, is_win)) = self.config.game_limits.lines_cleared {
                        if line_limit <= self.state.lineclears {
                            // End game immediately.
                            self.phase = Phase::GameEnd {
                                cause: GameEndCause::Limit(Stat::LinesCleared(line_limit)),
                                is_win,
                            };
                        }
                    } else if let Some((points_limit, is_win)) =
                        self.config.game_limits.points_scored
                    {
                        if points_limit <= self.state.points {
                            // End game immediately.
                            self.phase = Phase::GameEnd {
                                cause: GameEndCause::Limit(Stat::PointsScored(points_limit)),
                                is_win,
                            };
                        }
                    }
                    self.run_mods(Hook::CheckGameLimitsPost, &mut feed);
                }

                // Piece spawning.
                // - May move on to game over (BlockOut).
                // - Normally: Move on to piece-in-play.
                Phase::Spawning { spawn_time } if spawn_time <= target_time => {
                    self.run_mods(Hook::TimeStateProgressionPre(&mut target_time), &mut feed);
                    self.run_mods(Hook::SpawnPre(&mut target_time), &mut feed);
                    self.phase = do_spawn(&self.config, &mut self.state, spawn_time);
                    self.state.time = spawn_time;
                    self.run_mods(Hook::SpawnPost, &mut feed);
                    self.run_mods(Hook::TimeStateProgressionPost, &mut feed);
                }

                // Piece being manipulated by player.
                Phase::PieceInPlay {
                    piece,
                    auto_move_scheduled,
                    fall_or_lock_time,
                    lock_time_cap,
                    lowest_y,
                } if player_input.is_some()
                    && target_time <= fall_or_lock_time
                    && auto_move_scheduled
                        .is_none_or(|auto_move_time| target_time <= auto_move_time) =>
                {
                    // SAFETY: `player_input.is_some()`.
                    let input = unsafe { player_input.take().unwrap_unchecked() };

                    self.run_mods(Hook::TimeStateProgressionPre(&mut target_time), &mut feed);
                    self.run_mods(Hook::PlayerActionPre(input, &mut target_time), &mut feed);
                    let updated_active_buttons =
                        calc_updated_active_buttons(self.state.active_buttons, input, target_time);
                    self.phase = do_player_input(
                        &self.config,
                        &mut self.state,
                        piece,
                        auto_move_scheduled,
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
                    self.run_mods(Hook::TimeStateProgressionPost, &mut feed);
                }

                // Piece moving autonomously.
                Phase::PieceInPlay {
                    piece,
                    auto_move_scheduled: Some(auto_move_time),
                    fall_or_lock_time,
                    lock_time_cap,
                    lowest_y,
                } if auto_move_time <= target_time && auto_move_time <= fall_or_lock_time => {
                    self.run_mods(Hook::TimeStateProgressionPre(&mut target_time), &mut feed);
                    self.run_mods(Hook::AutoMovePre(&mut target_time), &mut feed);
                    self.phase = do_autonomous_move(
                        &self.config,
                        &mut self.state,
                        piece,
                        auto_move_time,
                        fall_or_lock_time,
                        lock_time_cap,
                        lowest_y,
                    );
                    self.state.time = auto_move_time;
                    self.run_mods(Hook::AutoMovePost, &mut feed);
                    self.run_mods(Hook::TimeStateProgressionPost, &mut feed);
                }

                // Piece falling.
                Phase::PieceInPlay {
                    piece,
                    auto_move_scheduled,
                    fall_or_lock_time: fall_time,
                    lock_time_cap,
                    lowest_y,
                } if fall_time <= target_time && piece.is_airborne(&self.state.board) => {
                    self.run_mods(Hook::TimeStateProgressionPre(&mut target_time), &mut feed);
                    self.run_mods(Hook::FallPre(&mut target_time), &mut feed);
                    self.phase = do_fall(
                        &self.config,
                        &mut self.state,
                        piece,
                        auto_move_scheduled,
                        fall_time,
                        lock_time_cap,
                        lowest_y,
                    );
                    self.state.time = fall_time;
                    self.run_mods(Hook::FallPost, &mut feed);
                    self.run_mods(Hook::TimeStateProgressionPost, &mut feed);
                }

                // Piece locking.
                Phase::PieceInPlay {
                    piece,
                    auto_move_scheduled: _,
                    fall_or_lock_time: lock_time,
                    lock_time_cap: _,
                    lowest_y: _,
                } if lock_time <= target_time => {
                    self.run_mods(Hook::TimeStateProgressionPre(&mut target_time), &mut feed);
                    self.run_mods(Hook::LockPre(&mut target_time), &mut feed);
                    self.phase =
                        do_lock(&self.config, &mut self.state, piece, lock_time, &mut feed);
                    self.state.time = lock_time;
                    self.run_mods(Hook::LockPost, &mut feed);
                    self.run_mods(Hook::TimeStateProgressionPost, &mut feed);

                    if let Some((pieces_limit, is_win)) = self.config.game_limits.pieces_locked {
                        if pieces_limit <= self.state.pieces_locked.iter().sum() {
                            // End game immediately.
                            self.phase = Phase::GameEnd {
                                cause: GameEndCause::Limit(Stat::PiecesLocked(pieces_limit)),
                                is_win,
                            };
                        }
                    }
                    self.run_mods(Hook::CheckGameLimitsPost, &mut feed);
                }

                // No actions within update target horizon, stop updating.
                // Return from update due to target time reached.
                _ => {
                    // Ensure states are updated.
                    // Ensure time is updated as requested, even when none of above cases triggered.
                    self.run_mods(Hook::TimeStateProgressionPre(&mut target_time), &mut feed);
                    // NOTE: Ensure buttons are still updated by inputs as requested,
                    // even when `PieceInPlay` case was not triggered (e.g. during `LinesClearing`).
                    if let Some(input) = player_input {
                        self.state.active_buttons = calc_updated_active_buttons(
                            self.state.active_buttons,
                            input,
                            target_time,
                        );
                    }
                    // NOTE: This *might* be redundant in some cases.
                    self.state.time = target_time;
                    self.run_mods(Hook::TimeStateProgressionPost, &mut feed);
                    return Ok(feed);
                }
            }
        }
    }
}

fn do_spawn(config: &Configuration, state: &mut State, spawn_time: InGameTime) -> Phase {
    let [active_movlf, active_movrt, active_rotlf, active_rotrt, active_rot180, _ds, _dh, _td, _tl, _tr, active_hld] =
        state
            .active_buttons
            .map(|keydowntime| keydowntime.is_some());

    // Take a tetromino.
    let next_tetromino = state.piece_preview.pop_front().unwrap_or_else(|| {
        state
            .piece_generator
            .with_rng(&mut state.rng)
            .next()
            .expect("piece generator empty before game end")
    });

    // Only put back in if necessary (e.g. if piece_preview_count < next_pieces.len()).
    state.piece_preview.extend(
        state.piece_generator.with_rng(&mut state.rng).take(
            config
                .piece_preview_count
                .saturating_sub(state.piece_preview.len()),
        ),
    );

    // "Initial Hold" system.
    if active_hld && config.allow_initial_actions {
        if let Some(next_phase) = try_do_hold(state, next_tetromino, spawn_time) {
            return next_phase;
        }
    }

    // 'Raw' spawn piece, before remaining prespawn_actions are applied.
    let piece_v1_raw = next_tetromino.piece_spawn_state();

    // "Initial Rotation" system.

    let mut turns = 0;

    if active_rotrt {
        turns += 1;
    }
    if active_rot180 {
        turns += 2;
    }
    if active_rotlf {
        turns -= 1;
    }

    /* NOTE (FIXME?): We do not currently allow other initial actions, because
    This forces us to impose an ordering on a set of actions which happen 'simultaneously'
    at game instant but require sequencing nevertheless. Currently it works like this:
    1. Raw initial spawn: Position piece.
    2. Initial 'Hold': Short-circuit rest of spawn sequence (no further sequencing).
    3. Initial 'Rotate': Use rotation system to change piece if possible.
        * Note: We use proper rotation. We could also simply hardcode a unique Initial 'Orientation'.
          But this is more flexible.

    Initial systems considered:
    * Initial 'Move': Happens before or after Rotate? Maybe depending on whether only one sequencing fails (->complexity)?
    * Initial 'Teleport' (L/R/down): Same thing.
    * Initial 'Drop' (soft/hard): Same thing.
    */

    // Optionally apply rotation to spawn piece.
    let piece_v2_rot = if config.allow_initial_actions {
        config
            .rotation_system
            .rotate(&piece_v1_raw, &state.board, turns)
    } else {
        piece_v1_raw.fits_onto(&state.board).then_some(piece_v1_raw)
    };

    // Try finding `Some` valid spawn piece from the provided options in order.
    let Some(piece_v3_ready) = piece_v2_rot else {
        // Otherwise BlockOut
        let blocked_piece = if config.allow_initial_actions {
            match config
                .rotation_system
                .rotate(&piece_v1_raw, &Board::default(), turns)
            {
                Some(rotated_piece) => rotated_piece,
                // This odd case happens when the rotation system does not even do rotation on an empty board.
                None => piece_v1_raw,
            }
        } else {
            piece_v1_raw
        };

        return Phase::GameEnd {
            cause: GameEndCause::BlockOut { blocked_piece },
            is_win: false,
        };
    };

    // We're falling if piece could move down.
    let is_airborne = piece_v3_ready.is_airborne(&state.board);

    let fall_or_lock_time = spawn_time.saturating_add(if is_airborne {
        // Fall immediately.
        Duration::ZERO
    } else {
        state.lock_delay.saturating_duration()
    });

    // Piece just spawned, lowest y = initial y.
    let lowest_y = piece_v3_ready.position.1;

    // Piece just spawned, standard full lock time max.
    let lock_time_cap = spawn_time.saturating_add(
        state
            .lock_delay
            .mul_ennf64(config.lock_reset_cap_factor)
            .saturating_duration(),
    );

    // Schedule immediate move after spawning, if any move button held.
    // NOTE: We have no Initial Move System for (mechanics, code) simplicity reasons.
    let auto_move_scheduled = if active_movlf || active_movrt {
        Some(spawn_time)
    } else {
        None
    };

    Phase::PieceInPlay {
        piece: piece_v3_ready,
        auto_move_scheduled,
        fall_or_lock_time,
        lock_time_cap,
        lowest_y,
    }
}

#[allow(clippy::too_many_arguments)]
fn do_player_input(
    config: &Configuration,
    state: &mut State,
    previous_piece: Piece,
    previous_auto_move_scheduled: Option<InGameTime>,
    previous_fall_or_lock_time: InGameTime,
    previous_lock_time_cap: InGameTime,
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
          Actv_R  ||  Actv_L  ||  L==R  ||  L>R !Deact_R  ||  L<R !Deact_L
      Or:
          !(Deact_L !L>=R || Deact_R !L=<R)
    * (ᶜ) *Canceling auto-move time*:
          L r Deact_l  ||  r L Deact_L
    * *Performing immediate move*; Same as (ˢ)!

    ### Move Resumption [⁴]

    We *also* want to allow a player to hold 'move' while a piece is stuck, in a way where
    the piece should move immediately as soon as it is unstuck (e.g. once fallen below the obstruction).
    * This system takes effect in the non-(ˢ)-(ᶜ)-entries of Table (⁵).
    * However, it has to be computed after another event has been handled that may be cause of unobstruction.

    */

    // Prepare to maybe change the move_scheduled.
    let mut computed_move_input_data: Option<(bool, bool)> = None;

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

        // Teleports.
        // Just instantly try to move piece all the way into applicable direction.
        I::Activate(teleport @ (B::TeleDown | B::TeleLeft | B::TeleRight)) => {
            let offset = match teleport {
                B::TeleDown => (0, -1),
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

        // Hard Drop.
        // Instantly try to move piece all the way down.
        // The locking is handled as part of a different check/system further.
        I::Activate(B::DropHard) => {
            updated_piece = updated_piece.teleported(&state.board, (0, -1));

            if config.notification_level != NotificationLevel::Silent {
                feed.push((
                    Notification::HardDrop {
                        previous_piece,
                        updated_piece,
                    },
                    input_time,
                ));
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

        // Movement.
        // This is relatively complicated; The logic is based on the comment in (³).
        I::Activate(B::MoveLeft | B::MoveRight) | I::Deactivate(B::MoveLeft | B::MoveRight) => {
            let prev_l = state.active_buttons[B::MoveLeft];
            let prev_r = state.active_buttons[B::MoveRight];
            let (l, r) = (prev_l.is_some(), prev_r.is_some());

            // *Setting auto-move time* (alt): !(Deact_L !L>=R || Deact_R !L=<R)
            let rescheduleautomove = {
                let a = matches!(input, I::Deactivate(B::MoveLeft)) && !(r && prev_l >= prev_r);
                let b = matches!(input, I::Deactivate(B::MoveRight)) && !(l && prev_l <= prev_r);
                !(a || b)
            };

            // *Canceling auto-move time*: L r Deact_l  ||  r L Deact_L
            let cancelautomove = {
                let a = l && !r && matches!(input, I::Deactivate(B::MoveLeft));
                let b = !l && r && matches!(input, I::Deactivate(B::MoveRight));
                a || b
            };

            computed_move_input_data = Some((rescheduleautomove, cancelautomove));
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
            | B::TeleLeft
            | B::TeleRight
            | B::HoldPiece,
        ) => {}
    }

    // Epilogue. Finalize state updates.

    // Update movetimer and rest of movement stuff.
    // See also (³).

    let updated_auto_move_scheduled = 'exp: {
        let Some((is_moving_left, mut dir_active_since)) =
            calc_is_moving_left_and_dir_active_since(updated_active_buttons)
        else {
            // No sensible movement information received.
            break 'exp None;
        };

        let dx = if is_moving_left { -1 } else { 1 };

        // Handle case where movement input was activated.
        if let Some((rescheduleautomove, cancelautomove)) = computed_move_input_data {
            if rescheduleautomove {
                let Ok(moved_piece) = updated_piece.offset_on(&state.board, (dx, 0)) else {
                    // Unable to move; Unschedule autonomous movement.
                    break 'exp None;
                };

                // Actually do move.
                if matches!(input, Input::Activate(_)) {
                    updated_piece = moved_piece;
                } else {
                    dir_active_since = input_time;
                }

                // Reschedule autonomous movement.
                let is_airborne = updated_piece.is_airborne(&state.board);
                let auto_move_time = calc_next_auto_move_time(
                    config,
                    state,
                    input_time,
                    dir_active_since,
                    is_airborne,
                );
                break 'exp Some(auto_move_time);
            }

            if cancelautomove {
                // Buttons deactivated; Cancel autonomous movement.
                break 'exp None; // Buttons unpressed: Remove autonomous movement.
            }

            // No relevant movement changes caused by mvmt-related button input: Don't do anything.
            break 'exp previous_auto_move_scheduled;
        }

        if check_piece_became_movable(previous_piece, updated_piece, &state.board, dx) {
            // Due to the system mentioned in (⁴), we check
            // if the piece was stuck and became unstuck, and insert an immediate autonomous move.
            break 'exp Some(input_time);
        }

        // All checks passed, no changes need to be made.
        // This is the case where neither (³) or (⁴) apply.
        previous_auto_move_scheduled
    };

    // Update `lowest_y`, re-set `lock_time_cap` if applicable.
    let (updated_lowest_y, updated_lock_time_cap) = if updated_piece.position.1 < previous_lowest_y
    {
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
        (previous_lowest_y, previous_lock_time_cap)
    };

    let previous_is_airborne = previous_piece.is_airborne(&state.board);
    let updated_is_airborne = updated_piece.is_airborne(&state.board);

    // Update falltimer and locktimer. See (¹) and (²).
    let updated_fall_or_lock_time = if updated_is_airborne {
        // Calculate scheduled fall time. See (¹).
        let fall_reset = !previous_is_airborne
            || matches!(input, I::Activate(B::DropSoft) | I::Deactivate(B::DropSoft));
        if fall_reset {
            // Refresh fall timer if we *started* falling, or soft drop just pressed, or soft drop just released.}
            calc_next_fall_time(state, config, input_time, updated_active_buttons)
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
                .max(updated_lock_time_cap)
                .min(input_time.saturating_add(state.lock_delay.saturating_duration()))
        } else {
            // Previous lock time.
            previous_fall_or_lock_time
        }
    };

    // 'Update' ActionState;
    // Return it to the main state machine with the latest acquired piece data.
    Phase::PieceInPlay {
        piece: updated_piece,
        auto_move_scheduled: updated_auto_move_scheduled,
        fall_or_lock_time: updated_fall_or_lock_time,
        lock_time_cap: updated_lock_time_cap,
        lowest_y: updated_lowest_y,
    }
}

fn try_do_hold(
    state: &mut State,
    tetromino: Tetromino,
    next_spawn_time: InGameTime,
) -> Option<Phase> {
    match state.piece_held {
        // Nothing held yet, just hold spawned tetromino.
        None => {
            state.piece_held = Some((tetromino, false));
            // Issue a spawn.
            Some(Phase::Spawning {
                spawn_time: next_spawn_time,
            })
        }
        // Swap spawned tetromino, push held back into next pieces queue.
        Some((held_tet, true)) => {
            state.piece_held = Some((tetromino, false));
            // Cause the next spawn to specially be the piece we held.
            state.piece_preview.push_front(held_tet);
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
fn do_autonomous_move(
    config: &Configuration,
    state: &mut State,
    previous_piece: Piece,
    auto_move_time: InGameTime,
    previous_fall_or_lock_time: InGameTime,
    previous_lock_time_cap: InGameTime,
    previous_lowest_y: isize,
) -> Phase {
    // Move piece and update all appropriate piece-related values.

    let (updated_piece, updated_is_airborne, updated_auto_move_scheduled) = 'exp: {
        let Some((move_is_left, dir_active_since)) =
            calc_is_moving_left_and_dir_active_since(&state.active_buttons)
        else {
            // No sensible movement information received.
            break 'exp (
                previous_piece,
                previous_piece.is_airborne(&state.board),
                None,
            );
        };

        let dx = if move_is_left { -1 } else { 1 };
        if let Ok(moved_piece) = previous_piece.offset_on(&state.board, (dx, 0)) {
            let updated_piece = moved_piece;
            // Able to do relevant move; Insert autonomous movement.
            let is_airborne = updated_piece.is_airborne(&state.board);
            let auto_move_time = calc_next_auto_move_time(
                config,
                state,
                auto_move_time,
                dir_active_since,
                !is_airborne,
            );

            break 'exp (updated_piece, is_airborne, Some(auto_move_time));
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
    let updated_lock_time_cap = previous_lock_time_cap;

    let updated_fall_or_lock_time = if updated_is_airborne {
        // Calculate scheduled fall time. See (¹).
        let was_grounded = !previous_piece.is_airborne(&state.board);

        if was_grounded {
            // Refresh fall timer if we *started* falling.
            calc_next_fall_time(state, config, auto_move_time, &state.active_buttons)
        } else {
            // Falling as before.
            previous_fall_or_lock_time
        }
    } else {
        // NOTE: updated_lock_time_cap may actually lie in the past, so we first need to cap *it* from below (current time)!
        auto_move_time
            .max(updated_lock_time_cap)
            .min(auto_move_time.saturating_add(state.lock_delay.saturating_duration()))
    };

    // Update 'ActionState';
    // Return it to the main state machine with the latest acquired piece data.
    Phase::PieceInPlay {
        piece: updated_piece,
        auto_move_scheduled: updated_auto_move_scheduled,
        fall_or_lock_time: updated_fall_or_lock_time,
        lock_time_cap: updated_lock_time_cap,
        lowest_y: updated_lowest_y,
    }
}

fn do_fall(
    config: &Configuration,
    state: &mut State,
    previous_piece: Piece,
    previous_auto_move_scheduled: Option<InGameTime>,
    fall_time: InGameTime,
    previous_lock_time_cap: InGameTime,
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

    let (updated_lowest_y, updated_lock_time_cap) = if updated_piece.position.1 < previous_lowest_y
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
        (previous_lowest_y, previous_lock_time_cap)
    };

    let updated_is_airborne = updated_piece.is_airborne(&state.board);

    let updated_fall_or_lock_time = if updated_is_airborne {
        calc_next_fall_time(state, config, fall_time, &state.active_buttons)
    } else {
        // NOTE: lock_time_cap may actually lie in the past, so we first need to cap *it* from below (current time)!
        fall_time
            .max(updated_lock_time_cap)
            .min(fall_time.saturating_add(state.lock_delay.saturating_duration()))
    };

    let updated_auto_move_scheduled = 'exp: {
        let Some((is_moving_left, _dir_active_since)) =
            calc_is_moving_left_and_dir_active_since(&state.active_buttons)
        else {
            // No sensible movement information received.
            break 'exp None;
        };

        let dx = if is_moving_left { -1 } else { 1 };
        if check_piece_became_movable(previous_piece, updated_piece, &state.board, dx) {
            // Due to the system mentioned in (⁴), we check
            // if the piece was stuck and became unstuck, and insert an immediate autonomous move.
            break 'exp Some(fall_time);
        }

        // No changes need to be made.
        previous_auto_move_scheduled
    };

    // 'Update' ActionState;
    // Return it to the main state machine with the latest acquired piece data.
    Phase::PieceInPlay {
        piece: updated_piece,
        auto_move_scheduled: updated_auto_move_scheduled,
        fall_or_lock_time: updated_fall_or_lock_time,
        lock_time_cap: updated_lock_time_cap,
        lowest_y: updated_lowest_y,
    }
}

fn do_lock(
    config: &Configuration,
    state: &mut State,
    piece: Piece,
    lock_time: InGameTime,
    feed: &mut NotificationFeed,
) -> Phase {
    // Before board is changed, precompute whether a piece was 'spun' into position;
    // - 'Spun' pieces give higher score bonus.
    // - Only locked pieces can yield bonus (i.e. can't possibly move down).
    // - Only locked pieces clearing lines can yield bonus (i.e. can't possibly move left/right).
    // Thus, if a piece cannot move back up at lock time, it must have gotten there by rotation.
    // That's what a 'spin' is.
    let is_spin = piece.offset_on(&state.board, (0, 1)).is_err();

    let any_below_skyline = piece
        .tiles()
        .iter()
        .any(|&((_, y), _)| (y as usize) < Game::LOCK_OUT_HEIGHT);

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
    for ((x, y), tile_type_id) in piece.tiles() {
        // Put tile into board.
        state.board[y as usize][x as usize] = Some(tile_type_id);
    }

    if config.notification_level != NotificationLevel::Silent {
        feed.push((Notification::PieceLocked { piece }, lock_time));
    }

    // Update tally of pieces_locked.
    state.pieces_locked[piece.tetromino as usize] += 1;

    // Update ability to hold piece.
    if let Some((_held_tet, swap_allowed)) = &mut state.piece_held {
        *swap_allowed = true;
    }

    // Score bonus calculation.

    // Find lines which might get cleared by piece locking. (actual clearing done later).
    let mut cleared_y_coords = Vec::<usize>::with_capacity(4);
    for y in (0..Game::HEIGHT).rev() {
        if state.board[y].iter().all(|mino| mino.is_some()) {
            cleared_y_coords.push(y);
        }
    }

    let lineclears = u32::try_from(cleared_y_coords.len()).unwrap();

    if lineclears == 0 {
        // If no lines cleared, no score bonus and combo is reset.
        state.consecutive_line_clears = 0;

        // 'Update' ActionState;
        // No lines cleared, directly proceed to spawn.
        return Phase::Spawning {
            spawn_time: lock_time.saturating_add(config.spawn_delay),
        };
    }

    // Further calculation.

    // Increase combo.
    state.consecutive_line_clears += 1;

    let combo = state.consecutive_line_clears;

    let is_perfect_clear = state.board.iter().all(|line| {
        line.iter().all(|tile| tile.is_none()) || line.iter().all(|tile| tile.is_some())
    });

    // Compute main Score Bonus.
    let score_bonus =
        lineclears * if is_spin { 2 } else { 1 } * if is_perfect_clear { 4 } else { 1 } * 2 - 1
            + (combo - 1);

    if config.notification_level != NotificationLevel::Silent {
        feed.push((
            Notification::LinesClearing {
                y_coords: cleared_y_coords,
                line_clear_duration: config.line_clear_duration,
            },
            lock_time,
        ));

        feed.push((
            Notification::Accolade {
                score_bonus,
                tetromino: piece.tetromino,
                is_spin,
                lineclears,
                is_perfect_clear,
                combo,
            },
            lock_time,
        ));
    }

    // 'Update' ActionState;
    // Lines must be cleared, enter line clearing state.
    Phase::LinesClearing {
        clear_finish_time: lock_time.saturating_add(config.line_clear_duration),
        score_bonus,
    }
}

fn do_lines_clearing(
    config: &Configuration,
    state: &mut State,
    clear_finish_time: InGameTime,
) -> Phase {
    for y in (0..Game::HEIGHT).rev() {
        // Full line: move it to the cleared lines storage and push an empty line to the board.
        if state.board[y].iter().all(|tile| tile.is_some()) {
            // Starting from the offending line, we move down all others, then default the uppermost.
            state.board[y..].rotate_left(1);
            // FIXME: This could underflow.
            state.board[Game::HEIGHT - 1] = Line::default();
            state.lineclears += 1;

            // Increment level if update requested.
            if state.lineclears % config.update_delays_every_n_lineclears == 0 {
                // Calculate new fall- and lock delay for game state.
                (state.fall_delay, state.lock_delay) = calc_fall_and_lock_delay(
                    &config.fall_delay_params,
                    &config.lock_delay_params,
                    state.fall_delay_lowerbound_hit_at_n_lineclears,
                    state.lineclears,
                );

                // Remember the first time fall delay hit zero.
                if state.fall_delay == config.fall_delay_params.lowerbound
                    && state.fall_delay_lowerbound_hit_at_n_lineclears.is_none()
                {
                    state.fall_delay_lowerbound_hit_at_n_lineclears = Some(state.lineclears);
                }
            }
        }
    }

    Phase::Spawning {
        spawn_time: clear_finish_time.saturating_add(config.spawn_delay),
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
            previous_active_buttons[button] = None;
        }
    }
    previous_active_buttons
}

fn check_piece_became_movable(
    previous_piece: Piece,
    updated_piece: Piece,
    board: &Board,
    dx: isize,
) -> bool {
    let moved_previous_piece = previous_piece.offset_on(board, (dx, 0));
    let moved_updated_piece = updated_piece.offset_on(board, (dx, 0));

    moved_previous_piece.is_err() && moved_updated_piece.is_ok()
}

/// This function may return an integer = `-1` | `1` and a time at or after `move_time` for the next designated auto-move.
/// It returns `None` when it cannot determine a direction to move to, which happens when:
/// * Both directions were pressed at the exact same in-game time, or
/// * No direction is pressed.
fn calc_is_moving_left_and_dir_active_since(
    active_buttons: &ButtonsState,
) -> Option<(bool, InGameTime)> {
    Some(
        match (
            active_buttons[Button::MoveLeft],
            active_buttons[Button::MoveRight],
        ) {
            (Some(time_actvd_left), Some(time_actvd_right)) => {
                match time_actvd_left.cmp(&time_actvd_right) {
                    // 'Right' was pressed more recently, go right.
                    std::cmp::Ordering::Less => (false, time_actvd_right),
                    // Both pressed at exact same time, don't move.
                    std::cmp::Ordering::Equal => return None,
                    // 'Left' was pressed more recently, go left.
                    std::cmp::Ordering::Greater => (true, time_actvd_left),
                }
            }
            // Only 'left' pressed.
            (Some(time_prsd_left), None) => (true, time_prsd_left),
            // Only 'right' pressed.
            (None, Some(time_prsd_right)) => (false, time_prsd_right),
            // None pressed. No movement.
            (None, None) => return None,
        },
    )
}

fn calc_next_auto_move_time(
    config: &Configuration,
    state: &State,
    current_time: InGameTime,
    direction_active_since: InGameTime,
    is_grounded: bool,
) -> InGameTime {
    let mut move_delay =
        if current_time.saturating_sub(direction_active_since) >= config.delayed_auto_shift {
            config.auto_repeat_rate
        } else {
            config.delayed_auto_shift
        };

    let ensure_lt_lock_delay =
        (config.ensure_move_delay_lt_lock_delay && is_grounded).then_some(state.lock_delay);

    if let Some(ExtDuration::Finite(lock_delay)) = ensure_lt_lock_delay {
        // Ensure moves occur faster than locks.
        // FIXME: Is there a more elegant approach than trying to subtract the smallest possible nonzero `Duration`?
        move_delay = move_delay.min(lock_delay.saturating_sub(Duration::from_nanos(1)));
    }

    current_time.saturating_add(move_delay)
}

fn calc_next_fall_time(
    state: &State,
    config: &Configuration,
    current_time: InGameTime,
    active_buttons: &ButtonsState,
) -> InGameTime {
    let fall_delay = if active_buttons[Button::TeleDown].is_some() {
        ExtDuration::ZERO
    } else if active_buttons[Button::DropSoft].is_some() {
        state.fall_delay.div_ennf64(config.soft_drop_factor)
    } else {
        state.fall_delay
    };

    current_time.saturating_add(fall_delay.saturating_duration())
}

// Compute the fall and lock delay corresponding to the current lineclear progress.
fn calc_fall_and_lock_delay(
    fall_delay_params: &DelayParameters,
    lock_delay_params: &DelayParameters,
    fall_delay_lowerbound_hit_at_n_lineclears: Option<u32>,
    lineclears: u32,
) -> (ExtDuration, ExtDuration) {
    if let Some(hit_at_n_lineclears) = fall_delay_lowerbound_hit_at_n_lineclears {
        // Fall delay zero was hit at some point, only possibly decrease lock delay now.

        // Actually compute new delay from equation.
        let lock_delay = lock_delay_params.calculate(lineclears - hit_at_n_lineclears);

        (fall_delay_params.lowerbound, lock_delay)
    } else {
        // Normally decrease fall delay.

        // Actually compute new delay from equation.
        let fall_delay = fall_delay_params.calculate(lineclears);

        (fall_delay, lock_delay_params.base_delay)
    }
}
