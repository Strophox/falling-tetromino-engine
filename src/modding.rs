/*!
This module handles the modding facilities of the engine.
*/

use crate::{
    Configuration, Game, InGameTime, Input, Notification, NotificationFeed, NotificationLevel,
    Phase, State, StateInitialization,
};

/// Helper struct to enable [`GameModifier`]s to access to the game's internals.
#[derive(PartialEq, Eq, Debug)]
#[allow(unused, missing_docs)]
pub struct GameAccess<'a> {
    pub config: &'a mut Configuration,
    pub state_init: &'a StateInitialization,
    pub state: &'a mut State,
    pub phase: &'a mut Phase,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) enum Hook<'a> {
    GameBuilt,
    GameEnded,
    PlayerInputReceived(&'a mut InGameTime, &'a mut Option<Input>),
    TimeStateProgressionPre(&'a mut InGameTime),
    TimeStateProgressionPost,
    CheckGameLimitsPost,
    SpawnPre(&'a mut InGameTime),
    SpawnPost,
    PlayerActionPre(Input, &'a mut InGameTime),
    PlayerActionPost(Input),
    AutoMovePre(&'a mut InGameTime),
    AutoMovePost,
    FallPre(&'a mut InGameTime),
    FallPost,
    LockPre(&'a mut InGameTime),
    LockPost,
    LinesClearPre(&'a mut InGameTime),
    LinesClearPost,
}

impl Game {
    pub(crate) fn run_mods(&mut self, mut hook_point: Hook, feed: &mut NotificationFeed) {
        if self.config.notification_level == NotificationLevel::Debug {
            feed.push((
                Notification::Debug(format!("{hook_point:?}")),
                self.state.time,
            ));
        }
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
                Hook::GameEnded => modify.on_game_ended(game, feed),
                Hook::PlayerInputReceived(time, player_input) => {
                    modify.on_player_input_received(game, feed, time, player_input)
                }
                Hook::TimeStateProgressionPre(time) => {
                    modify.on_time_state_progression_pre(game, feed, time)
                }
                Hook::TimeStateProgressionPost => modify.on_time_state_progression_post(game, feed),
                Hook::CheckGameLimitsPost => modify.on_check_game_limits_post(game, feed),
                Hook::SpawnPre(time) => modify.on_spawn_pre(game, feed, time),
                Hook::SpawnPost => modify.on_spawn_post(game, feed),
                Hook::PlayerActionPre(input, time) => {
                    modify.on_player_action_pre(game, feed, *input, time)
                }
                Hook::PlayerActionPost(input) => modify.on_player_action_post(game, feed, *input),
                Hook::AutoMovePre(time) => modify.on_auto_move_pre(game, feed, time),
                Hook::AutoMovePost => modify.on_auto_move_post(game, feed),
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
/// * [`GameModifier::descriptor`] - Convention to identify a mod and its initial settings by name.
/// * [`GameModifier::try_clone`] - Possibility to duplicate a mod (and therefore the game) at runtime.
/// * Many hooks such as [`GameModifier::on_spawn_pre`] - Capability to systematically hook into engine process and modify the game state.
///
/// # Reproducibility
///
/// Note that for gameplay to be reproducible, it has to be ensure modification of the [`Game`] at runtime must be deterministic, in particular:
/// * One should not use `rand::random()` (thread-randomness) from the `rand` crate.
///   Consider accessing the available [`State::rng`] (of type [`GameRng`] accessible in [`GameAccess::state`]),
///   which is the engines internal PRNG source used for reproducibility.
/// * Only change the modifier's relevant state in engine hook methods which will be called the same number of times
///   whenever a game is run with the same exact player input sequence and timings.
///   In particular, [`GameModifier::on_time_state_progression_pre`]/[`GameModifier::on_time_state_progression_post`] should not be used to keep track of the granularity of
///   Update calls. For example:
///   - **Ok**: Use time-related information to simulate the modifier's own events
///     which should happen at arbitrary but deterministic points on the engine timeline.
///     (e.g. the tetromino type is converted to `Tetromino::I` )
pub trait GameModifier: std::fmt::Debug {
    /// Convention to identify a mod by name.
    ///
    fn id(&self) -> String;

    /// Convention to reconstruct an identified mod's constructor arguments.
    ///
    /// Given a constructor for a modifier ready to be attached to a game,
    /// ```ignore
    /// fn modifier(arg1: T1, ..., argX: TX) -> Box<dyn GameModifier>;
    /// ```
    /// or alternatively, a build finalizer which constructs a game,
    /// ```ignore
    /// fn build(builder: &GameBuilder, arg1: T1, ..., argX: TX) -> Game;
    /// ```
    /// Then, by convention, the modifier may store and produce a String
    /// which serializes its original constructor arguments. For example,
    /// ```ignore
    /// let constructor_args = (arg1, ..., argX);
    /// let args = serde_json::to_string(&constructor_args).unwrap();
    /// ```
    /// Such that it may be reconstructable using
    /// ```ignore
    /// let (arg1, ..., argX) = serde_json::from_str(args);
    /// ```
    fn args(&self) -> String;

    /// Try to clone the modifier if possible.
    /// Otherwise return an error.
    fn try_clone(&self) -> Result<Box<dyn GameModifier>, String>;

    /// This function gets called anytime [`Game::update`] is called with `Some` actual [`Input`].
    ///
    /// Note that the `&mut Option<Input>` allows this call to [`Option::take`] the input and nullify it.
    fn on_player_input_received(
        &mut self,
        _game: GameAccess,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
        _player_input: &mut Option<Input>,
    ) {
    }

    /// This function gets called once, when the game has finished getting constructed by [`GameBuilder`].
    fn on_game_built(&mut self, _game: GameAccess) {}

    /// This function gets called once, when the game has entered [`Phase::GameEnd`].
    fn on_game_ended(&mut self, _game: GameAccess, _feed: &mut NotificationFeed) {}

    /// This function gets called anytime and immediately before any step inside the engine is applied which will update the game state in a way where time is moved forward.
    fn on_time_state_progression_pre(
        &mut self,
        _game: GameAccess,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called anytime and immediately after any step inside the engine is applied which will update the game state in a way where time has been moved forward.
    fn on_time_state_progression_post(&mut self, _game: GameAccess, _feed: &mut NotificationFeed) {}

    /// This function gets called immediately after the engine has checked all its limiting stats and possibly ended.
    fn on_check_game_limits_post(&mut self, _game: GameAccess, _feed: &mut NotificationFeed) {}

    /// This function gets called immediately before [`Phase::Spawning`] is handled.
    fn on_spawn_pre(
        &mut self,
        _game: GameAccess,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after [`Phase::Spawning`] has been handled.
    fn on_spawn_post(&mut self, _game: GameAccess, _feed: &mut NotificationFeed) {}

    /// This function gets called immediately before a player action in [`Phase::PieceInPlay`] is handled.
    fn on_player_action_pre(
        &mut self,
        _game: GameAccess,
        _feed: &mut NotificationFeed,
        _input: Input,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after a player action in [`Phase::PieceInPlay`] has been handled.
    fn on_player_action_post(
        &mut self,
        _game: GameAccess,
        _feed: &mut NotificationFeed,
        _input: Input,
    ) {
    }

    /// This function gets called immediately before an autonomous move of the piece in [`Phase::PieceInPlay`] is handled.
    fn on_auto_move_pre(
        &mut self,
        _game: GameAccess,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after an autonomous move of the piece in [`Phase::PieceInPlay`] has been handled.
    fn on_auto_move_post(&mut self, _game: GameAccess, _feed: &mut NotificationFeed) {}

    /// This function gets called immediately before falling of the piece in [`Phase::PieceInPlay`] is handled.
    fn on_fall_pre(
        &mut self,
        _game: GameAccess,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after falling of the piece in [`Phase::PieceInPlay`] has been handled.
    fn on_fall_post(&mut self, _game: GameAccess, _feed: &mut NotificationFeed) {}

    /// This function gets called immediately before locking of the piece in [`Phase::PieceInPlay`] is handled.
    fn on_lock_pre(
        &mut self,
        _game: GameAccess,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after locking of the piece in [`Phase::PieceInPlay`] has been handled.
    fn on_lock_post(&mut self, _game: GameAccess, _feed: &mut NotificationFeed) {}

    /// This function gets called immediately before [`Phase::LinesClearing`] is handled.
    fn on_lines_clear_pre(
        &mut self,
        _game: GameAccess,
        _feed: &mut NotificationFeed,
        _time: &mut InGameTime,
    ) {
    }
    /// This function gets called immediately after [`Phase::LinesClearing`] has been handled.
    fn on_lines_clear_post(&mut self, _game: GameAccess, _feed: &mut NotificationFeed) {}
}
