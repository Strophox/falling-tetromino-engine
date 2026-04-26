/*!
Customizing, templating and constructing [`Game`]s.
 */

use std::{collections::VecDeque, time::Duration};

use either::Either;
use rand::Rng;
use rand_chacha::rand_core::SeedableRng;

use crate::{
    core::{
        Configuration, DelayTable, ExtDelayData, Game, Phase, SoftDropSpeedup, State,
        StateInitialization,
    },
    game_modding::Hook,
    tetromino_generation::StdTetGen,
};

use super::*;

/// This builder exposes the ability to configure a new [`Game`] to varying degrees.
///
/// Generally speaking, when using `GameBuilder`, you’ll:
/// 1. first call [`GameBuilder::new`] or [`Game::builder`],
/// 2. then chain calls to methods to set configurations,
/// 3. then call [`GameBuilder::build`] or [`GameBuilder::build_modded`].
///
/// This will give you a [`Game`] as specified in the process that you can then use as normal.
/// The `GameBuilder` is not used up and its configuration can be re-used to initialize more [`Game`]s.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GameBuilder<TetGen = StdTetGen, PceRot = StdPceRot> {
    seed: Option<u64>,
    tetromino_generator: Option<TetGen>,
    config: Configuration<PceRot>,
}

impl<TetGen, PceRot: Default> Default for GameBuilder<TetGen, PceRot> {
    fn default() -> Self {
        Self {
            seed: Default::default(),
            tetromino_generator: Default::default(),
            config: Default::default(),
        }
    }
}

impl<TetGen, PceRot: Default> GameBuilder<TetGen, PceRot> {
    /// Creates a blank new template representing a yet-to-be-started [`Game`] ready for configuration.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<TetGen: TetrominoGenerator + Clone, PceRot: Clone> GameBuilder<TetGen, PceRot> {
    /// Creates a [`Game`] with the information specified by `self`.
    pub fn build(&self) -> Game<TetGen, PceRot> {
        self.build_modded(Vec::new())
    }

    /// Creates a [`Game`] with the information specified by `self` and some one-time `modifiers`.
    pub fn build_modded(
        &self,
        modifiers: Vec<Box<dyn GameModifier<TetGen, PceRot>>>,
    ) -> Game<TetGen, PceRot> {
        let seed = self.seed.unwrap_or_else(|| rand::rng().next_u64());
        let mut rng = GameRng::seed_from_u64(seed);
        let tetromino_generator = self
            .tetromino_generator
            .clone()
            .unwrap_or_else(|| TetGen::from_rng(&mut rng));
        let config = self.config.clone();

        let (fall_delay, fall_lowerbound_hit) = config
            .fall_delay_curve
            .retrieve_and_check(0, config.update_delays_every_n_lineclears);
        let lock_delay = if let Some(lock_delay_curve) = &config.lock_delay_curve {
            lock_delay_curve
                .retrieve_and_check(0, config.update_delays_every_n_lineclears)
                .0
        } else {
            fall_delay
        };

        let mut game = Game {
            modifiers,
            phase: Phase::Spawning {
                spawn_time: InGameTime::ZERO,
            },
            state: State {
                time: InGameTime::ZERO,
                active_buttons: [None; Button::VARIANTS.len()],
                rng,
                piece_generator: tetromino_generator.clone(),
                piece_preview: VecDeque::new(),
                piece_held: None,
                board: Board::default(),
                fall_delay,
                fall_delay_lowerbound_hit_at_n_lineclears: fall_lowerbound_hit.then_some(0),
                lock_delay,
                pieces_locked: [0; Tetromino::VARIANTS.len()],
                lineclears: 0,
                consecutive_lineclears: 0,
                points: 0,
            },
            state_init: StateInitialization {
                seed,
                tetromino_generator,
            },
            config,
        };

        // Initialize mods.
        game.run_mods(Hook::GameBuilt, &mut Vec::new());

        game
    }
}

// Getting a `GameBuilder` blueprint back from an existing `Game`.
impl<TetGen: Clone, PceRot: Clone> Game<TetGen, PceRot> {
    /// Creates a blueprint [`GameBuilder`] and an iterator over current modifier identifiers ([`&str`]s) from which the exact game can potentially be rebuilt.
    ///
    /// Note that the `&str`s serve the *client* to identify the modifiers and reapply them onto the `GameBuilder`, as the base engine does not know how to do so.
    pub fn blueprint(&self) -> (GameBuilder<TetGen, PceRot>, Vec<(String, String)>) {
        let builder = GameBuilder {
            seed: Some(self.state_init.seed),
            tetromino_generator: Some(self.state_init.tetromino_generator.clone()),
            config: self.config.clone(),
        };

        let mod_ids_cfgs = self.modifiers.iter().map(|m| (m.id(), m.cfg())).collect();

        (builder, mod_ids_cfgs)
    }
}

// Gamebuilder: Setter methods.
impl<TetGen, PceRot> GameBuilder<TetGen, PceRot> {
    /// The value to seed the game's PRNG with.
    pub fn seed(&mut self, x: u64) -> &mut Self {
        self.seed = Some(x);
        self
    }

    /// The method (and internal state) of tetromino generation used.
    pub fn tetromino_generator(&mut self, x: TetGen) -> &mut Self {
        self.tetromino_generator = Some(x);
        self
    }

    /// Sets the [`Configuration`] that will be used by [`Game`].
    pub fn config(&mut self, x: Configuration<PceRot>) -> &mut Self {
        self.config = x;
        self
    }

    /// How many pieces should be pre-generated and accessible/visible in the game state.
    pub fn generate_piece_preview(&mut self, x: usize) -> &mut Self {
        self.config.generate_piece_preview = x;
        self
    }
    /// Whether holding a 'rotate' button lets a piece be smoothly spawned in a rotated state,
    /// or holding the 'hold' button lets a piece be swapped immediately before it evens spawns.
    pub fn allow_spawn_manipulation(&mut self, x: bool) -> &mut Self {
        self.config.allow_spawn_manipulation = x;
        self
    }
    /// The method of tetromino rotation used.
    pub fn rotation_system(&mut self, x: PceRot) -> &mut Self {
        self.config.rotation_system = x;
        self
    }
    /// How long the game should take to spawn a new piece.
    pub fn spawn_delay(&mut self, x: Duration) -> &mut Self {
        self.config.spawn_delay = x;
        self
    }
    /// How long it takes for the active piece to start automatically shifting more to the side
    /// after the initial time a 'move' button has been pressed.
    pub fn delayed_auto_shift(&mut self, x: Duration) -> &mut Self {
        self.config.delayed_auto_shift = x;
        self
    }
    /// How long it takes for automatic side movement to repeat once it has started.
    pub fn auto_repeat_rate(&mut self, x: Duration) -> &mut Self {
        self.config.auto_repeat_rate = x;
        self
    }
    /// Specification of how fall delay gets calculated from the rest of the state.
    pub fn fall_delay_curve(&mut self, x: Either<DelayParameters, DelayTable>) -> &mut Self {
        self.config.fall_delay_curve = x;
        self
    }
    /// How soft drop should speed up the falling of a piece should speed up while [`Button::DropSoft`] is held.
    /// - One variant describes how many times faster than the current gravity falling should be.
    /// - The other variant describes the fall delay that should be used, if it is faster than current gravity. Otherwise no change.
    pub fn soft_drop_speedup(&mut self, x: SoftDropSpeedup) -> &mut Self {
        self.config.soft_drop_speedup = x;
        self
    }
    /// Specification of how fall delay gets calculated from the rest of the state.
    pub fn lock_delay_curve(
        &mut self,
        x: Option<Either<DelayParameters, DelayTable>>,
    ) -> &mut Self {
        self.config.lock_delay_curve = x;
        self
    }
    /// Whether engine should try to ensure that delays for autonomous moves - which are determined by
    /// `delayed_auto_shift` and `auto_repeat_rate` - should be less than `lock_delay` runs out.
    /// This allows DAS and ARR to function at extreme game speeds.
    pub fn ensure_shift_delay_lt_lock_delay(&mut self, x: bool) -> &mut Self {
        self.config.ensure_shift_delay_lt_lock_delay = x;
        self
    }
    /// Whether just pressing a rotation- or movement button is enough to refresh lock delay.
    /// Normally, lock delay only resets if rotation or movement actually succeeds.
    pub fn allow_lenient_lock_reset(&mut self, x: bool) -> &mut Self {
        self.config.allow_lenient_lock_reset = x;
        self
    }
    /// How long each spawned active piece may touch the ground in total until it should lock down
    /// immediately.
    pub fn lock_reset_cap_factor(&mut self, x: ExtNonNegF64) -> &mut Self {
        self.config.lock_reset_cap_factor = x;
        self
    }
    /// How long the game should take to clear a line.
    pub fn line_clear_duration(&mut self, x: InGameTime) -> &mut Self {
        self.config.line_clear_duration = x;
        self
    }
    /// When to update the fall and lock delays in [`State`].
    pub fn update_delays_every_n_lineclears(&mut self, x: u32) -> &mut Self {
        self.config.update_delays_every_n_lineclears = x;
        self
    }
    /// Stores the ways in which a round of the game should be limited.
    ///
    /// Each limitation may be either of positive ('game completed') or negative ('game over'), as
    /// designated by the `bool` stored with it.
    ///
    /// No limitations may allow for endless games.
    pub fn game_limits(&mut self, x: GameLimits) -> &mut Self {
        self.config.game_limits = x;
        self
    }
    /// The amount of feedback information that is to be generated.
    pub fn send_notifications(&mut self, x: bool) -> &mut Self {
        self.config.send_notifications = x;
        self
    }
}
