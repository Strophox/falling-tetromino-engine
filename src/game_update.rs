/*!
This module handles what happens when [`Game::update`] is called.
*/

use super::*;

impl Game {
    /// Immediately end a game by forfeiting the current round.
    ///
    /// This can be used so `game.has_ended()` returns true and prevents future
    /// calls to `update` from continuing to advance the game.
    pub fn forfeit(&mut self) -> Result<Vec<FeedbackMsg>, UpdateGameError> {
        if self.has_ended() {
            // Do not allow updating a game that has already ended.
            return Err(UpdateGameError::AlreadyEnded);
        }

        let is_win = false;

        self.phase = Phase::GameEnd {
            cause: GameEndCause::Forfeit,
            is_win,
        };

        Ok(vec![(self.state.time, Feedback::GameEnded { is_win })])
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
    /// Unless an error occurs, this function will return all [`FeedbackMsg`]s caused between the
    /// previous and the current `update` call, in chronological order.
    ///
    /// # Errors
    ///
    /// This function may error with:
    /// - [`UpdateGameError::GameEnded`] if `game.ended()` is `true`, indicating that no more updates
    ///   can change the game state, or
    /// - [`UpdateGameError::TargetTimeInPast`] if `target_time < game.state().time`, indicating that
    ///   the requested update lies in the past.
    pub fn update(
        &mut self,
        target_time: InGameTime,
        mut input: Option<Input>,
    ) -> Result<Vec<FeedbackMsg>, UpdateGameError> {
        if target_time < self.state.time {
            // Do not allow updating if target time lies in the past.
            return Err(UpdateGameError::TargetTimeInPast);
        } else if self.has_ended() {
            // Do not allow updating a game that has already ended.
            return Err(UpdateGameError::AlreadyEnded);
        }

        // Prepare new button state.
        let mut new_state_buttons_pressed = self.state.buttons_pressed;
        match input {
            Some(Input::Activate(button)) => new_state_buttons_pressed[button] = Some(target_time),
            Some(Input::Deactivate(button)) => new_state_buttons_pressed[button] = None,
            None => {}
        }

        let mut feedback_msgs = Vec::new();

        // We linearly process all events until we reach the targeted update time.
        loop {
            // Maybe move on to game over if an end condition is met now.
            if let Some(new_phase) = self.try_end_game_if_end_condition_met() {
                self.phase = new_phase;
            }
            self.run_mods(UpdatePoint::MainLoopHead(&mut input), &mut feedback_msgs);

            match self.phase {
                // Game ended by now.
                // Return accumulated messages.
                Phase::GameEnd { cause: _, is_win } => {
                    // Add message that game ended.
                    feedback_msgs.push((self.state.time, Feedback::GameEnded { is_win }));

                    // Return early.
                    return Ok(feedback_msgs);
                }

                // Lines clearing.
                // Move on to spawning.
                Phase::LinesClearing {
                    line_clears_finish_time,
                } if line_clears_finish_time <= target_time => {
                    self.phase =
                        do_line_clearing(&self.config, &mut self.state, line_clears_finish_time);
                    self.state.time = line_clears_finish_time;

                    // Return from update due to game end.
                    self.run_mods(UpdatePoint::LinesCleared, &mut feedback_msgs);
                }

                // Piece spawning.
                // - May move on to game over (BlockOut).
                // - Normally: Move on to piece-in-play.
                Phase::Spawning { spawn_time } if spawn_time <= target_time => {
                    self.phase = do_spawn(&self.config, &mut self.state, spawn_time);
                    self.state.time = spawn_time;

                    self.run_mods(UpdatePoint::PieceSpawned, &mut feedback_msgs);
                }

                // Piece autonomously moving / falling / locking.
                // - Locking may move on to game over (LockOut).
                Phase::PieceInPlay { piece_data }
                    if (piece_data.fall_or_lock_time <= target_time
                        || piece_data
                            .auto_move_scheduled
                            .is_some_and(|move_time| move_time <= target_time)) =>
                {
                    let mut flag = false;
                    if let Some(move_time) = piece_data.auto_move_scheduled {
                        if move_time <= piece_data.fall_or_lock_time && move_time <= target_time {
                            // Piece is moving autonomously and before next fall/lock.
                            flag = true;

                            self.phase = do_autonomous_move(
                                &self.config,
                                &mut self.state,
                                piece_data,
                                move_time,
                            );
                            self.state.time = move_time;

                            self.run_mods(UpdatePoint::PieceAutoMoved, &mut feedback_msgs);
                        }
                    }
                    // else: Piece is not moving autonomously and instead falls or locks
                    if !flag {
                        if piece_data.is_fall_not_lock {
                            self.phase = do_fall(
                                &self.config,
                                &mut self.state,
                                piece_data,
                                piece_data.fall_or_lock_time,
                            );
                            self.state.time = piece_data.fall_or_lock_time;

                            self.run_mods(UpdatePoint::PieceFell, &mut feedback_msgs);
                        } else {
                            self.phase = do_lock(
                                &self.config,
                                &mut self.state,
                                piece_data.piece,
                                piece_data.fall_or_lock_time,
                                &mut feedback_msgs,
                            );
                            self.state.time = piece_data.fall_or_lock_time;

                            self.run_mods(UpdatePoint::PieceLocked, &mut feedback_msgs);
                        }
                    }
                }

                // Piece acted upon by player.
                Phase::PieceInPlay { piece_data } if input.is_some() => {
                    let Some(input) = input.take() else {
                        unreachable!()
                    };
                    self.phase = do_player_input(
                        &self.config,
                        &mut self.state,
                        piece_data,
                        input,
                        new_state_buttons_pressed,
                        target_time,
                        &mut feedback_msgs,
                    );
                    self.state.time = target_time;
                    self.state.buttons_pressed = new_state_buttons_pressed;

                    self.run_mods(UpdatePoint::PiecePlayed(input), &mut feedback_msgs);
                }

                // No actions within update target horizon, stop updating.
                _ => {
                    // Ensure states are updated.
                    // NOTE: This *might* be redundant in some cases.

                    // NOTE: Ensure time is updated as requested, even when none of above cases triggered.
                    self.state.time = target_time;

                    // NOTE: Ensure button state is updated as requested, even when `PieceInPlay` case is not triggered.
                    self.state.buttons_pressed = new_state_buttons_pressed;

                    // Return from update due to target time reached.
                    return Ok(feedback_msgs);
                }
            }
        }
    }

    /// Updates the internal `self.state.end` state, checking whether any [`Limits`] have been reached.
    #[allow(clippy::manual_map)]
    fn try_end_game_if_end_condition_met(&self) -> Option<Phase> {
        // Game already ended.
        if self.has_ended() {
            return None;

            // Not ended yet, so check whether any end conditions have been met now and return appropriate phase if yes.
        }

        self.config
            .end_conditions
            .iter()
            .find_map(|&(stat, is_win_condition)| {
                if self.check_stat_met(stat) {
                    Some(Phase::GameEnd {
                        cause: GameEndCause::Limit(stat),
                        is_win: is_win_condition,
                    })
                } else {
                    None
                }
            })
    }

    /// Goes through all internal 'game mods' and applies them sequentially at the given [`ModifierPoint`].
    fn run_mods(
        &mut self,
        mut update_point: UpdatePoint<&mut Option<Input>>,
        feedback_msgs: &mut Vec<FeedbackMsg>,
    ) {
        if self.config.feedback_verbosity == FeedbackVerbosity::Debug {
            use UpdatePoint as UP;
            let update_point = match &update_point {
                UP::MainLoopHead(x) => UP::MainLoopHead(format!("{x:?}")),
                UP::PiecePlayed(b) => UP::PiecePlayed(*b),
                UP::LinesCleared => UP::LinesCleared,
                UP::PieceSpawned => UP::PieceSpawned,
                UP::PieceAutoMoved => UP::PieceAutoMoved,
                UP::PieceFell => UP::PieceFell,
                UP::PieceLocked => UP::PieceLocked,
            };
            feedback_msgs.push((self.state.time, Feedback::Debug(update_point)));
        }
        for modifier in &mut self.modifiers {
            (modifier.mod_function)(
                &mut update_point,
                &mut self.config,
                &mut self.state_init,
                &mut self.state,
                &mut self.phase,
                feedback_msgs,
            );
        }
    }
}

fn do_spawn(config: &Configuration, state: &mut State, spawn_time: InGameTime) -> Phase {
    let [button_ml, button_mr, button_rl, button_rr, button_ra, _ds, _dh, _td, _tl, _tr, button_h] =
        state
            .buttons_pressed
            .map(|keydowntime| keydowntime.is_some());

    // Take a tetromino.
    let spawn_tet = state.piece_preview.pop_front().unwrap_or_else(|| {
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
    if button_h && config.allow_prespawn_actions {
        if let Some(new_phase) = try_hold(state, spawn_tet, spawn_time) {
            return new_phase;
        }
    }

    // Prepare data of spawned piece.
    let raw_pos = match spawn_tet {
        Tetromino::O => (4, Game::LOCK_OUT_HEIGHT as isize),
        _ => (3, Game::LOCK_OUT_HEIGHT as isize),
    };

    // 'Raw' spawn piece, before remaining prespawn_actions are applied.
    let raw_spawn_piece = Piece {
        tetromino: spawn_tet,
        orientation: Orientation::N,
        position: raw_pos,
    };

    // "Initial Rotation" system.

    let mut turns = 0;

    if button_rr {
        turns += 1;
    }
    if button_ra {
        turns += 2;
    }
    if button_rl {
        turns -= 1;
    }

    // Possibly; Rotation of 'raw' spawn piece.
    let spawn_piece = if config.allow_prespawn_actions {
        config
            .rotation_system
            .rotate(&raw_spawn_piece, &state.board, turns)
    } else {
        raw_spawn_piece
            .fits_onto(&state.board)
            .then_some(raw_spawn_piece)
    };

    // Try finding `Some` valid spawn piece from the provided options in order.
    let Some(spawn_piece) = spawn_piece else {
        // Otherwise BlockOut
        let blocked_piece = if config.allow_prespawn_actions {
            match config
                .rotation_system
                .rotate(&raw_spawn_piece, &Board::default(), turns)
            {
                Some(rotated_piece) => rotated_piece,
                // This odd case happens when the rotation system does not even do rotation on an empty board.
                None => raw_spawn_piece,
            }
        } else {
            raw_spawn_piece
        };

        return Phase::GameEnd {
            cause: GameEndCause::BlockOut { blocked_piece },
            is_win: false,
        };
    };

    // We're falling if piece could move down.
    let is_fall_not_lock = spawn_piece.offset_on(&state.board, (0, -1)).is_ok();

    let fall_or_lock_time = spawn_time.saturating_add(if is_fall_not_lock {
        // Fall immediately.
        Duration::ZERO
    } else {
        state.lock_delay.saturating_duration()
    });

    // Piece just spawned, lowest y = initial y.
    let lowest_y = spawn_piece.position.1;

    // Piece just spawned, standard full lock time max.
    let capped_lock_time = spawn_time.saturating_add(
        state
            .lock_delay
            .mul_ennf64(config.lock_reset_cap_factor)
            .saturating_duration(),
    );

    // Schedule immediate move after spawning, if any move button held.
    // NOTE: We have no Initial Move System for (mechanics, code) simplicity reasons.
    let auto_move_scheduled = if button_ml || button_mr {
        Some(spawn_time)
    } else {
        None
    };

    Phase::PieceInPlay {
        piece_data: PieceData {
            piece: spawn_piece,
            fall_or_lock_time,
            is_fall_not_lock,
            auto_move_scheduled,
            lowest_y,
            capped_lock_time,
        },
    }
}

fn do_line_clearing(
    config: &Configuration,
    state: &mut State,
    line_clears_finish_time: InGameTime,
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
        spawn_time: line_clears_finish_time.saturating_add(config.spawn_delay),
    }
}

fn check_piece_became_movable_get_moved_piece(
    prev_piece: Piece,
    next_piece: Piece,
    board: &Board,
    dx: isize,
) -> Option<Piece> {
    let moved_prev_piece = prev_piece.offset_on(board, (dx, 0));
    let moved_next_piece = next_piece.offset_on(board, (dx, 0));

    if let (Err(_), Ok(valid_moved_piece)) = (moved_prev_piece, moved_next_piece) {
        Some(valid_moved_piece)

    // No changes need to be made after all.
    // This is the case where neither (³) or (⁴) apply.
    } else {
        None
    }
}

fn do_autonomous_move(
    config: &Configuration,
    state: &mut State,
    previous_piece_data: PieceData,
    auto_move_time: InGameTime,
) -> Phase {
    // Move piece and update all appropriate piece-related values.

    let mut new_piece = previous_piece_data.piece;

    let ensure_lt_lock_delay = (config.ensure_move_delay_lt_lock_delay
        && !previous_piece_data.is_fall_not_lock)
        .then_some(state.lock_delay);

    let opt_dx_and_next_move_time = calc_move_direction_and_next_move_time(
        config,
        &state.buttons_pressed,
        auto_move_time,
        ensure_lt_lock_delay,
    );

    let new_auto_move_scheduled = if let Some((dx, next_move_time)) = opt_dx_and_next_move_time {
        if let Ok(moved_piece) = previous_piece_data.piece.offset_on(&state.board, (dx, 0)) {
            new_piece = moved_piece;
            // Able to do relevant move; Insert autonomous movement.
            Some(next_move_time)
        } else {
            // Unable to move; Remove autonomous movement.
            None
        }
    } else {
        // No sensible movement information received.
        None
    };

    // Horizontal move could not have affected height, so it stays the same!
    let new_lowest_y = previous_piece_data.lowest_y;
    let new_capped_lock_time = previous_piece_data.capped_lock_time;

    let new_is_fall_not_lock = new_piece.offset_on(&state.board, (0, -1)).is_ok();

    let new_fall_or_lock_time = if new_is_fall_not_lock {
        // Calculate scheduled fall time.
        // This implements (¹).
        let was_grounded = previous_piece_data
            .piece
            .offset_on(&state.board, (0, -1))
            .is_err();

        if was_grounded {
            // Refresh fall timer if we *started* falling.
            auto_move_time.saturating_add(
                if state.buttons_pressed[Button::DropSoft].is_some() {
                    state.fall_delay.div_ennf64(config.soft_drop_divisor)
                } else {
                    state.fall_delay
                }
                .saturating_duration(),
            )
        } else {
            // Falling as before.
            previous_piece_data.fall_or_lock_time
        }
    } else {
        // NOTE: capped_lock_time may actually lie in the past, so we first need to cap *it* from below (current time)!
        auto_move_time
            .max(new_capped_lock_time)
            .min(auto_move_time.saturating_add(state.lock_delay.saturating_duration()))
    };

    // Update 'ActionState';
    // Return it to the main state machine with the latest acquired piece data.
    Phase::PieceInPlay {
        piece_data: PieceData {
            piece: new_piece,
            fall_or_lock_time: new_fall_or_lock_time,
            is_fall_not_lock: new_is_fall_not_lock,
            auto_move_scheduled: new_auto_move_scheduled,
            lowest_y: new_lowest_y,
            capped_lock_time: new_capped_lock_time,
        },
    }
}

fn do_fall(
    config: &Configuration,
    state: &mut State,
    previous_piece_data: PieceData,
    fall_time: InGameTime,
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
    let mut new_piece = previous_piece_data.piece;
    if let Ok(fallen_piece) = previous_piece_data.piece.offset_on(&state.board, (0, -1)) {
        new_piece = fallen_piece;
    }

    // Move resumption.
    let ensure_lt_lock_delay = (config.ensure_move_delay_lt_lock_delay
        && !previous_piece_data.is_fall_not_lock)
        .then_some(state.lock_delay);

    let opt_dx_and_next_move_time = calc_move_direction_and_next_move_time(
        config,
        &state.buttons_pressed,
        fall_time,
        ensure_lt_lock_delay,
    );

    let new_auto_move_scheduled = if let Some((dx, next_move_time)) = opt_dx_and_next_move_time {
        if let Some(moved_piece) = check_piece_became_movable_get_moved_piece(
            previous_piece_data.piece,
            new_piece,
            &state.board,
            dx,
        ) {
            // Naïvely, movement direction should be kept;
            // But due to the system mentioned in (⁴), we do need to check
            // if the piece was stuck and became unstuck, and manually do a move in this case!
            new_piece = moved_piece;
            Some(next_move_time)
        } else {
            // No changes need to be made.
            previous_piece_data.auto_move_scheduled
        }
    } else {
        // No sensible movement information received.
        None
    };

    let (new_lowest_y, new_capped_lock_time) =
        if new_piece.position.1 < previous_piece_data.lowest_y {
            // Refresh position and capped_lock_time.
            (
                new_piece.position.1,
                fall_time.saturating_add(
                    state
                        .lock_delay
                        .mul_ennf64(config.lock_reset_cap_factor)
                        .saturating_duration(),
                ),
            )
        } else {
            (
                previous_piece_data.lowest_y,
                previous_piece_data.capped_lock_time,
            )
        };

    let new_is_fall_not_lock = new_piece.offset_on(&state.board, (0, -1)).is_ok();

    let new_fall_or_lock_time = if new_is_fall_not_lock {
        fall_time.saturating_add(
            if state.buttons_pressed[Button::DropSoft].is_some() {
                state.fall_delay.div_ennf64(config.soft_drop_divisor)
            } else {
                state.fall_delay
            }
            .saturating_duration(),
        )
    } else {
        // NOTE: capped_lock_time may actually lie in the past, so we first need to cap *it* from below (current time)!
        fall_time
            .max(new_capped_lock_time)
            .min(fall_time.saturating_add(state.lock_delay.saturating_duration()))
    };

    // 'Update' ActionState;
    // Return it to the main state machine with the latest acquired piece data.
    Phase::PieceInPlay {
        piece_data: PieceData {
            piece: new_piece,
            fall_or_lock_time: new_fall_or_lock_time,
            is_fall_not_lock: new_is_fall_not_lock,
            auto_move_scheduled: new_auto_move_scheduled,
            lowest_y: new_lowest_y,
            capped_lock_time: new_capped_lock_time,
        },
    }
}

fn do_player_input(
    config: &Configuration,
    state: &mut State,
    previous_piece_data: PieceData,
    input: Input,
    new_state_buttons_pressed: [Option<InGameTime>; Button::VARIANTS.len()],
    input_time: InGameTime,
    feedback_msgs: &mut Vec<FeedbackMsg>,
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
    * We want to understand when auto-move time needs to be set anew.
      This can be seen in Table (⁵) in entries marked (ˢ) and coincides with
      a direct move performed.
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
    +-----------+----------------------------------------------+
    |           |     Deact_l   Deact_r   Actv_R    Actv_L     |
    + Old state +----------------------------------------------+
    |           |                                              |
    |   l  r    |      l  r      l  r      l  R      L  r      |
    |   ·  ·    |      ·  ·      ·  ·      · +→ˢ     ←+ ·ˢ     |
    |           |                                              |
    |   L  r    |      l  r      L  r      L  R      L  r      |
    |   ←  ·    |      ←- ·ᶜ     ←  ·      ←-+→ˢ     ←+ ·ˢ     |
    |           |                                              |
    |   L> R    |      l  R      L  r      L  R      L  R      |
    |   ←  ·    |      ←-+→ˢ     ←  ·      ←-+→ˢ     ←+ ·ˢ     |
    |           |                                              |
    |   L==R    |      l  R      L  r      L  R      L  R      |
    |   ·  ·    |      · +→ˢ     ←+ ·ˢ     · +→ˢ     ←+ ·ˢ     |
    |           |                                              |
    |   L <R    |      l  R      L  r      L  R      L  R      |
    |   ·  →    |      ·  →      ←+-→ˢ     · +→ˢ     ←+-→ˢ     |
    |           |                                              |
    |   l  R    |      l  R      l  r      l  R      L  R      |
    |   ·  →    |      ·  →      · -→ᶜ     · +→ˢ     ←+-→ˢ     |
    |           |                                              |
    +-----------+----------------------------------------------+

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

    let mut new_piece = previous_piece_data.piece;
    use {Button as B, Input as I};
    match input {
        // Hold.
        // - If succeeds, changes game action state to spawn different piece.
        // - Otherwise does nothing.
        I::Activate(B::HoldPiece) => {
            if let Some(new_phase) = try_hold(state, new_piece.tetromino, input_time) {
                return new_phase;
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

            new_piece = new_piece.teleported(&state.board, offset);
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
                    .rotate(&new_piece, &state.board, right_turns)
            {
                new_piece = rotated_piece;
            }
        }

        // Hard Drop.
        // Instantly try to move piece all the way down.
        // The locking is handled as part of a different check/system further.
        I::Activate(B::DropHard) => {
            new_piece = new_piece.teleported(&state.board, (0, -1));

            if config.feedback_verbosity != FeedbackVerbosity::Silent {
                feedback_msgs.push((
                    input_time,
                    Feedback::HardDrop {
                        old_piece: previous_piece_data.piece,
                        new_piece,
                    },
                ));
            }
        }

        // Soft Drop.
        // Instantly try to move piece one tile down.
        // The locking is handled as part of a different check/system further.
        I::Activate(B::DropSoft) => {
            if let Ok(fallen_piece) = new_piece.offset_on(&state.board, (0, -1)) {
                new_piece = fallen_piece;
            }
        }

        // Movement.
        // This is relatively very complicated; The logic is based on the comment in (³).
        I::Activate(B::MoveLeft | B::MoveRight) | I::Deactivate(B::MoveLeft | B::MoveRight) => {
            let prev_l = state.buttons_pressed[B::MoveLeft];
            let prev_r = state.buttons_pressed[B::MoveRight];
            let (l, r) = (prev_l.is_some(), prev_r.is_some());

            // *Setting auto-move time* (alt): !(Deact_L !L>=R || Deact_R !L=<R)
            let sentinel_setmvmt = {
                let a = matches!(input, I::Deactivate(B::MoveLeft)) && !(r && prev_l >= prev_r);
                let b = matches!(input, I::Deactivate(B::MoveRight)) && !(l && prev_l <= prev_r);
                !(a || b)
            };

            // *Canceling auto-move time*: L r Deact_l  ||  r L Deact_L
            let sentinel_cancelmvmt = {
                let a = l && !r && matches!(input, I::Deactivate(B::MoveLeft));
                let b = !l && r && matches!(input, I::Deactivate(B::MoveRight));
                a || b
            };

            computed_move_input_data = Some((sentinel_setmvmt, sentinel_cancelmvmt));
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

    let new_auto_move_scheduled = 'exp: {
        let ensure_lt_lock_delay = (config.ensure_move_delay_lt_lock_delay
            && !previous_piece_data.is_fall_not_lock)
            .then_some(state.lock_delay);
        let Some((dx, next_move_time)) = calc_move_direction_and_next_move_time(
            config,
            &new_state_buttons_pressed,
            input_time,
            ensure_lt_lock_delay,
        ) else {
            // No sensible movement information received.
            break 'exp None;
        };

        // Handle case where movement input was activated.
        if let Some((initiate_mvmt, cancel_mvmt)) = computed_move_input_data {
            break 'exp if initiate_mvmt {
                if let Ok(moved_piece) = new_piece.offset_on(&state.board, (dx, 0)) {
                    new_piece = moved_piece;
                    // Able to do relevant move; Set autonomous movement.
                    Some(next_move_time)
                } else {
                    // Unable to move; Unschedule autonomous movement.
                    None
                }
            } else if cancel_mvmt {
                // Buttons deactivated; Cancel autonomous movement.
                None // Buttons unpressed: Remove autonomous movement.
            } else {
                // No relevant movement changes caused by mvmt-related button input: Don't do anything.
                previous_piece_data.auto_move_scheduled
            };
        }

        // Due to the system mentioned in (⁴), we do need to check
        // if the piece was stuck and became unstuck, and manually do a move in this case!
        if let Some(moved_piece) = check_piece_became_movable_get_moved_piece(
            previous_piece_data.piece,
            new_piece,
            &state.board,
            dx,
        ) {
            // (Also note: We use `(dx, next_move_time)` as computed from the *new* button state - but should not change, since this route is only triggered if the piece is able to move again and NOT because of a player move (`maybe_override_auto_move` is `None`).)
            new_piece = moved_piece;
            break 'exp Some(next_move_time);
        }

        // All checks passed, no changes need to be made.
        // This is the case where neither (³) or (⁴) apply.
        previous_piece_data.auto_move_scheduled
    };

    // Update `lowest_y`, re-set `capped_lock_time` if applicable.
    let (new_lowest_y, new_capped_lock_time) =
        if new_piece.position.1 < previous_piece_data.lowest_y {
            // Refresh position and capped_lock_time.
            (
                new_piece.position.1,
                input_time.saturating_add(
                    state
                        .lock_delay
                        .mul_ennf64(config.lock_reset_cap_factor)
                        .saturating_duration(),
                ),
            )
        } else {
            (
                previous_piece_data.lowest_y,
                previous_piece_data.capped_lock_time,
            )
        };

    // Update `is_fall_not_lock`, i.e., whether we are falling (otherwise locking) now.
    // `new_is_fall_not_lock` is needed below.
    let new_is_fall_not_lock = new_piece.offset_on(&state.board, (0, -1)).is_ok();

    let was_grounded = previous_piece_data
        .piece
        .offset_on(&state.board, (0, -1))
        .is_err();

    // Update falltimer and locktimer.
    // See also (¹) and (²).
    let new_fall_or_lock_time = if new_is_fall_not_lock {
        // Calculate scheduled fall time.
        // This implements (¹).
        let fall_reset =
            was_grounded || matches!(input, I::Activate(B::DropSoft) | I::Deactivate(B::DropSoft));
        if fall_reset {
            // Refresh fall timer if we *started* falling, or soft drop just pressed, or soft drop just released.
            input_time.saturating_add(
                if new_state_buttons_pressed[Button::DropSoft].is_some() {
                    state.fall_delay.div_ennf64(config.soft_drop_divisor)
                } else {
                    state.fall_delay
                }
                .saturating_duration(),
            )
        } else {
            // Falling as before.
            previous_piece_data.fall_or_lock_time
        }
    } else {
        // Calculate scheduled lock time.
        // This implements (²).
        let lock_immediately = matches!(input, I::Activate(B::DropHard))
            || (was_grounded && matches!(input, I::Activate(B::DropSoft)));
        let lock_reset_piecechange = new_piece != previous_piece_data.piece;
        let lock_reset_lenience = config.lenient_lock_delay_reset
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
            // NOTE: capped_lock_time may actually lie in the past, so we first need to cap *it* from below (current time)!
            input_time
                .max(new_capped_lock_time)
                .min(input_time.saturating_add(state.lock_delay.saturating_duration()))
        } else {
            // Previous lock time.
            previous_piece_data.fall_or_lock_time
        }
    };

    // 'Update' ActionState;
    // Return it to the main state machine with the latest acquired piece data.
    Phase::PieceInPlay {
        piece_data: PieceData {
            piece: new_piece,
            fall_or_lock_time: new_fall_or_lock_time,
            is_fall_not_lock: new_is_fall_not_lock,
            auto_move_scheduled: new_auto_move_scheduled,
            lowest_y: new_lowest_y,
            capped_lock_time: new_capped_lock_time,
        },
    }
}

fn try_hold(
    state: &mut State,
    tetromino: Tetromino,
    new_piece_spawn_time: InGameTime,
) -> Option<Phase> {
    match state.piece_held {
        // Nothing held yet, just hold spawned tetromino.
        None => {
            state.piece_held = Some((tetromino, false));
            // Issue a spawn.
            Some(Phase::Spawning {
                spawn_time: new_piece_spawn_time,
            })
        }
        // Swap spawned tetromino, push held back into next pieces queue.
        Some((held_tet, true)) => {
            state.piece_held = Some((tetromino, false));
            // Cause the next spawn to specially be the piece we held.
            state.piece_preview.push_front(held_tet);
            // Issue a spawn.
            Some(Phase::Spawning {
                spawn_time: new_piece_spawn_time,
            })
        }
        // Else can't hold, don't do anything.
        _ => None,
    }
}

fn do_lock(
    config: &Configuration,
    state: &mut State,
    piece: Piece,
    lock_time: InGameTime,
    feedback_msgs: &mut Vec<FeedbackMsg>,
) -> Phase {
    if config.feedback_verbosity != FeedbackVerbosity::Silent {
        feedback_msgs.push((lock_time, Feedback::PieceLocked { piece }));
    }

    // Before board is changed, precompute whether a piece was 'spun' into position;
    // - 'Spun' pieces give higher score bonus.
    // - Only locked pieces can yield bonus (i.e. can't possibly move down).
    // - Only locked pieces clearing lines can yield bonus (i.e. can't possibly move left/right).
    // Thus, if a piece cannot move back up at lock time, it must have gotten there by rotation.
    // That's what a 'spin' is.
    let is_spin = piece.offset_on(&state.board, (0, 1)).is_err();

    let fits_below_skyline = piece
        .tiles()
        .iter()
        .all(|&((_, y), _)| (y as usize) < Game::LOCK_OUT_HEIGHT);

    // If all minos of the tetromino were locked entirely outside the `SKYLINE` bounding height, it's game over.
    if !fits_below_skyline {
        return Phase::GameEnd {
            cause: GameEndCause::LockOut {
                locked_out_piece: piece,
            },
            is_win: false,
        };
    }

    // Locking.
    for ((x, y), tile_type_id) in piece.tiles() {
        // Put tile into board.
        state.board[y as usize][x as usize] = Some(tile_type_id);
    }

    // Update tally of pieces_locked.
    state.pieces_locked[piece.tetromino as usize] += 1;

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
    } else {
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

        // Update score.
        state.score += score_bonus;

        if config.feedback_verbosity != FeedbackVerbosity::Silent {
            feedback_msgs.push((
                lock_time,
                Feedback::LinesClearing {
                    y_coords: cleared_y_coords,
                    line_clear_start: config.line_clear_duration,
                },
            ));

            feedback_msgs.push((
                lock_time,
                Feedback::Accolade {
                    score_bonus,
                    tetromino: piece.tetromino,
                    is_spin,
                    lineclears,
                    is_perfect_clear,
                    combo,
                },
            ));
        }
    }

    // Update ability to hold piece.
    if let Some((_held_tet, swap_allowed)) = &mut state.piece_held {
        *swap_allowed = true;
    }

    // 'Update' ActionState;
    // Return it to the main state machine with all newly acquired piece data.
    if lineclears == 0 {
        // No lines cleared, directly proceed to spawn.
        Phase::Spawning {
            spawn_time: lock_time.saturating_add(config.spawn_delay),
        }
    } else {
        // Lines cleared, enter line clearing state.
        Phase::LinesClearing {
            line_clears_finish_time: lock_time.saturating_add(config.line_clear_duration),
        }
    }
}

/// This function may return an integer = `-1` | `1` and a time at or after `move_time` for the next designated auto-move.
/// It returns `None` when it cannot determine a direction to move to, which happens when:
/// * Both directions were pressed at the exact same in-game time, or
/// * No direction is pressed.
fn calc_move_direction_and_next_move_time(
    config: &Configuration,
    button_state: &[Option<InGameTime>; Button::VARIANTS.len()],
    move_time: InGameTime,
    ensure_lt_lock_delay: Option<ExtDuration>,
) -> Option<(isize, InGameTime)> {
    let (dx, how_long_relevant_direction_pressed) = match (
        button_state[Button::MoveLeft],
        button_state[Button::MoveRight],
    ) {
        (Some(time_actvd_left), Some(time_actvd_right)) => {
            match time_actvd_left.cmp(&time_actvd_right) {
                // 'Right' was pressed more recently, go right.
                std::cmp::Ordering::Less => (1, move_time.saturating_sub(time_actvd_right)),
                // Both pressed at exact same time, don't move.
                std::cmp::Ordering::Equal => return None,
                // 'Left' was pressed more recently, go left.
                std::cmp::Ordering::Greater => (-1, move_time.saturating_sub(time_actvd_left)),
            }
        }
        // Only 'left' pressed.
        (Some(time_prsd_left), None) => (-1, move_time.saturating_sub(time_prsd_left)),
        // Only 'right' pressed.
        (None, Some(time_prsd_right)) => (1, move_time.saturating_sub(time_prsd_right)),
        // None pressed. No movement.
        (None, None) => return None,
    };

    let mut move_delay = if how_long_relevant_direction_pressed >= config.delayed_auto_shift {
        config.auto_repeat_rate
    } else {
        config.delayed_auto_shift
    };

    if let Some(ExtDuration::Finite(lock_delay)) = ensure_lt_lock_delay {
        // Ensure moves occur faster than locks.
        // FIXME: Is there a more elegant approach than trying to subtract the smallest possible nonzero `Duration`?
        move_delay = move_delay.min(lock_delay.saturating_sub(Duration::from_nanos(1)));
    }

    Some((dx, move_time.saturating_add(move_delay)))
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
