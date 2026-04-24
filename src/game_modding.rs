/*!
Modding facilities for the engine.
*/

use crate::core::{Configuration, Game, Phase, State, StateInitialization};

use super::*;

/// Helper struct to enable [`GameModifier`]s to access to the game's internals.
#[derive(PartialEq, Eq, Debug)]
#[allow(unused, missing_docs)]
pub struct GameAccess<'a, TetGen, PceRot> {
    pub config: &'a mut Configuration<PceRot>,
    pub state_init: &'a StateInitialization<TetGen>,
    pub state: &'a mut State<TetGen>,
    pub phase: &'a mut Phase,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) enum Hook<'a> {
    GameBuilt,
    GameEnded,
    ReceivePlayerInput(&'a mut InGameTime, &'a mut Option<Input>),
    ProgressTimeStatePre(&'a mut InGameTime),
    ProgressTimeStatePost,
    CheckGameLimitsPre,
    CheckGameLimitsPost,
    SpawnPre(&'a mut InGameTime),
    SpawnPost,
    PlayerActionPre(Input, &'a mut InGameTime),
    PlayerActionPost(Input),
    AutoShiftPre(&'a mut InGameTime),
    AutoShiftPost,
    FallPre(&'a mut InGameTime),
    FallPost,
    LockPre(&'a mut InGameTime),
    LockPost,
    LinesClearPre(&'a mut InGameTime),
    LinesClearPost,
}

impl<TetGen, PceRot> Game<TetGen, PceRot> {
    pub(crate) fn run_mods(&mut self, mut hook_point: Hook, feed: &mut NotificationFeed) {
        for modifier in &mut self.modifiers {
            let modify = modifier.as_mut();
            let game = GameAccess {
                config: &mut self.config,
                state_init: &self.state_init,
                state: &mut self.state,
                phase: &mut self.phase,
            };
            match &mut hook_point {
                Hook::GameBuilt => modify.on_game_built(game),
                Hook::GameEnded => modify.on_game_end(game, feed),
                Hook::ReceivePlayerInput(time, player_input) => {
                    modify.on_receive_player_input(game, feed, time, player_input)
                }
                Hook::ProgressTimeStatePre(time) => {
                    modify.on_progress_time_state_pre(game, feed, time)
                }
                Hook::ProgressTimeStatePost => modify.on_progress_time_state_post(game, feed),
                Hook::CheckGameLimitsPre => modify.on_check_game_limits_pre(game, feed),
                Hook::CheckGameLimitsPost => modify.on_check_game_limits_post(game, feed),
                Hook::SpawnPre(time) => modify.on_spawn_pre(game, feed, time),
                Hook::SpawnPost => modify.on_spawn_post(game, feed),
                Hook::PlayerActionPre(input, time) => {
                    modify.on_player_action_pre(game, feed, *input, time)
                }
                Hook::PlayerActionPost(input) => modify.on_player_action_post(game, feed, *input),
                Hook::AutoShiftPre(time) => modify.on_autoshift_pre(game, feed, time),
                Hook::AutoShiftPost => modify.on_autoshift_post(game, feed),
                Hook::FallPre(time) => modify.on_fall_pre(game, feed, time),
                Hook::FallPost => modify.on_fall_post(game, feed),
                Hook::LockPre(time) => modify.on_lock_pre(game, feed, time),
                Hook::LockPost => modify.on_lock_post(game, feed),
                Hook::LinesClearPre(time) => modify.on_lines_clear_pre(game, feed, time),
                Hook::LinesClearPost => modify.on_lines_clear_post(game, feed),
            };
        }
    }
}

/// Trait that allows direct interaction with the engine at runtime, used for 'modding'.
///
/// The trait's main functionalities are:
/// * [`GameModifier::id`] - Convention to identify a mod by name.
/// * [`GameModifier::cfg`] - Convention to serialize a mod's configuration of some sort, in particular for reproducibility.
/// * [`GameModifier::try_clone`] - Possibility to duplicate a mod (and therefore the game) at runtime.
/// * Many hooks such as [`GameModifier::on_spawn_pre`] - Capability to systematically hook into engine process and modify the game state.
///
/// # Reproducibility
///
/// Note that for gameplay to be reproducible, it has to be ensured that that modifiers run deterministically according to how they are called.
/// In particular:
/// * One should **not** use `rand::random()` and similar local thread-randomness.
///   Instead, consider using the [`State::rng`] accessible in [`GameAccess::state`],
///   which is the engine's internal PRNG source (used for reproducibility).
/// * Only make visible changes that do not depend on special engine hook methods which can be
///   called a indeterminate number of times for a given game.
///   In particular, [`GameModifier::on_progress_time_state_pre`]/[`GameModifier::on_progress_time_state_post`] should **not** be used to keep track of
///   e.g. the number of times [`Game::update`] has been called specifically. For example:
///   - O.k.: Use time-related information to simulate the modifier's own events
///     which should happen at arbitrary but deterministic points on the engine timeline.
///     (e.g. the tetromino type is converted to `Tetromino::I` after 1s of `Piece` spawn.)
///   - **Not** O.k.: Keep track of the frontend's approximate framerate (by counting number of time update calls)
///     and turn the active piece into `Tetromino::O` for certain values.
pub trait GameModifier<TetGen = StdTetGen, PceRot = StdPceRot>: std::fmt::Debug {
    /// Convention to identify a mod by name.
    // FIXME: This could be -> Cow<String, 'a> or a type determined by user.
    fn id(&self) -> String;

    /// Convention to reconstruct an identified mod's starting configuration.
    ///
    /// Given a constructor for a modifier ready to be attached to a game,
    /// ```ignore
    /// fn modifier(cfg: MyModCfg) -> Box<dyn GameModifier>;
    /// ```
    /// or alternatively, a build finalizer which constructs a game,
    /// ```ignore
    /// fn build(builder: &GameBuilder, cfg: MyModCfg) -> Game;
    /// ```
    /// Then, by convention, the modifier may make available a String
    /// which serializes its configuration.
    /// In particular, this can be used for reproducibility. For example,
    /// ```ignore
    /// fn cfg(&self) -> String {
    ///     let initial_config = MyModCfg { start_fooing_at: self.stored_initial_cfg.start_fooing_at };
    ///     serde_json::to_string(&initial_config).unwrap();
    /// }
    /// ```
    /// Such that it may be reconstructable using
    /// ```ignore
    /// let cfg: String = my_mod.cfg();
    ///
    /// /* Store cfg to file and load it again etc. */
    ///
    /// let initial_config = serde_json::from_str(cfg);
    /// ```
    // FIXME: This could be -> Cow<String, 'a> or a type determined by user.
    fn cfg(&self) -> String;

    /// This method allows a modifier to provide access to internal state the modifier would like to display.
    // FIXME: This could be more general (e.g. key-value store-like type) or a type determined by user.
    fn stats(&self) -> &[&str];

    /// Try to clone the modifier if possible.
    /// Otherwise return an error.
    fn try_clone(&self) -> Result<Box<dyn GameModifier<TetGen, PceRot>>, String>;

    /// This function gets called anytime [`Game::update`] is called with `Some` actual [`Input`].
    ///
    /// Note that the `&mut Option<Input>` allows this call to [`Option::take`] the input and nullify it.
    fn on_receive_player_input(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
        _player_input: &mut Option<Input>,
    ) {
    }

    /// This function gets called once, when the game has finished getting constructed by `GameBuilder`.
    fn on_game_built(&mut self, _game: GameAccess<TetGen, PceRot>) {}

    /// This function gets called once, when the game has entered [`Phase::GameEnd`].
    fn on_game_end(&mut self, _game: GameAccess<TetGen, PceRot>, _feed: &mut NotificationFeed) {}

    /// This function gets called anytime and immediately before any step inside the engine is applied which will update the game state in a way where time is moved forward.
    fn on_progress_time_state_pre(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called anytime and immediately after any step inside the engine is applied which will update the game state in a way where time has been moved forward.
    fn on_progress_time_state_post(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
    ) {
    }

    /// This function gets called immediately bfore the engine checks all its limiting stats and possibly ends.
    fn on_check_game_limits_pre(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
    ) {
    }

    /// This function gets called immediately after the engine has checked all its limiting stats and possibly ended.
    fn on_check_game_limits_post(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
    ) {
    }

    /// This function gets called immediately before [`Phase::Spawning`] is handled.
    fn on_spawn_pre(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after [`Phase::Spawning`] has been handled.
    fn on_spawn_post(&mut self, _game: GameAccess<TetGen, PceRot>, _feed: &mut NotificationFeed) {}

    /// This function gets called immediately before a player action in [`Phase::PieceInPlay`] is handled.
    fn on_player_action_pre(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
        _input: Input,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after a player action in [`Phase::PieceInPlay`] has been handled.
    fn on_player_action_post(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
        _input: Input,
    ) {
    }

    /// This function gets called immediately before an autonomous move of the piece in [`Phase::PieceInPlay`] is handled.
    fn on_autoshift_pre(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after an autonomous move of the piece in [`Phase::PieceInPlay`] has been handled.
    fn on_autoshift_post(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
    ) {
    }

    /// This function gets called immediately before falling of the piece in [`Phase::PieceInPlay`] is handled.
    fn on_fall_pre(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after falling of the piece in [`Phase::PieceInPlay`] has been handled.
    fn on_fall_post(&mut self, _game: GameAccess<TetGen, PceRot>, _feed: &mut NotificationFeed) {}

    /// This function gets called immediately before locking of the piece in [`Phase::PieceInPlay`] is handled.
    fn on_lock_pre(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after locking of the piece in [`Phase::PieceInPlay`] has been handled.
    fn on_lock_post(&mut self, _game: GameAccess<TetGen, PceRot>, _feed: &mut NotificationFeed) {}

    /// This function gets called immediately before [`Phase::ClearingLines`] is handled.
    fn on_lines_clear_pre(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after [`Phase::ClearingLines`] has been handled.
    fn on_lines_clear_post(
        &mut self,
        _game: GameAccess<TetGen, PceRot>,
        _feed: &mut NotificationFeed,
    ) {
    }
}

/// A debug modifier implementation.
#[derive(Debug)]
pub struct DebugMod;

impl<TetGen, PceRot> GameModifier<TetGen, PceRot> for DebugMod {
    fn id(&self) -> String {
        stringify!(DebugMod).to_owned()
    }

    fn cfg(&self) -> String {
        "".to_owned()
    }

    fn stats(&self) -> &[&str] {
        &[]
    }

    fn try_clone(&self) -> Result<Box<dyn GameModifier<TetGen, PceRot>>, String> {
        Ok(Box::new(DebugMod))
    }

    fn on_receive_player_input(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
        _time: &mut InGameTime,
        _player_input: &mut Option<Input>,
    ) {
        feed.push((
            Notification::Custom("on_receive_player_input".to_owned()),
            game.state.time,
        ));
    }

    fn on_game_built(&mut self, _game: GameAccess<TetGen, PceRot>) {}

    fn on_game_end(&mut self, game: GameAccess<TetGen, PceRot>, feed: &mut NotificationFeed) {
        feed.push((
            Notification::Custom("on_game_end".to_owned()),
            game.state.time,
        ));
    }

    fn on_progress_time_state_pre(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
        feed.push((
            Notification::Custom("on_progress_time_state_pre".to_owned()),
            game.state.time,
        ));
    }
    fn on_progress_time_state_post(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
    ) {
        feed.push((
            Notification::Custom("on_progress_time_state_post".to_owned()),
            game.state.time,
        ));
    }

    fn on_check_game_limits_pre(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
    ) {
        feed.push((
            Notification::Custom("on_check_game_limits_pre".to_owned()),
            game.state.time,
        ));
    }

    fn on_check_game_limits_post(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
    ) {
        feed.push((
            Notification::Custom("on_check_game_limits_post".to_owned()),
            game.state.time,
        ));
    }

    fn on_spawn_pre(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
        feed.push((
            Notification::Custom("on_spawn_pre".to_owned()),
            game.state.time,
        ));
    }
    fn on_spawn_post(&mut self, game: GameAccess<TetGen, PceRot>, feed: &mut NotificationFeed) {
        feed.push((
            Notification::Custom("on_spawn_post".to_owned()),
            game.state.time,
        ));
    }

    fn on_player_action_pre(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
        _input: Input,
        _time: &mut InGameTime,
    ) {
        feed.push((
            Notification::Custom("on_player_action_pre".to_owned()),
            game.state.time,
        ));
    }
    fn on_player_action_post(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
        _input: Input,
    ) {
        feed.push((
            Notification::Custom("on_player_action_post".to_owned()),
            game.state.time,
        ));
    }

    fn on_autoshift_pre(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
        feed.push((
            Notification::Custom("on_autoshift_pre".to_owned()),
            game.state.time,
        ));
    }
    fn on_autoshift_post(&mut self, game: GameAccess<TetGen, PceRot>, feed: &mut NotificationFeed) {
        feed.push((
            Notification::Custom("on_autoshift_post".to_owned()),
            game.state.time,
        ));
    }

    fn on_fall_pre(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
        feed.push((
            Notification::Custom("on_fall_pre".to_owned()),
            game.state.time,
        ));
    }
    fn on_fall_post(&mut self, game: GameAccess<TetGen, PceRot>, feed: &mut NotificationFeed) {
        feed.push((
            Notification::Custom("on_fall_post".to_owned()),
            game.state.time,
        ));
    }

    fn on_lock_pre(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
        feed.push((
            Notification::Custom("on_lock_pre".to_owned()),
            game.state.time,
        ));
    }
    fn on_lock_post(&mut self, game: GameAccess<TetGen, PceRot>, feed: &mut NotificationFeed) {
        feed.push((
            Notification::Custom("on_lock_post".to_owned()),
            game.state.time,
        ));
    }

    fn on_lines_clear_pre(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
        feed.push((
            Notification::Custom("on_lines_clear_pre".to_owned()),
            game.state.time,
        ));
    }
    fn on_lines_clear_post(
        &mut self,
        game: GameAccess<TetGen, PceRot>,
        feed: &mut NotificationFeed,
    ) {
        feed.push((
            Notification::Custom("on_lines_clear_post".to_owned()),
            game.state.time,
        ));
    }
}
