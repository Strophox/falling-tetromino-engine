/*!
This module handles random generation of [`Tetromino`]s.
*/

use std::num::NonZeroU32;

use rand::{
    self,
    distr::{weighted::WeightedIndex, Distribution},
    Rng, RngExt,
};

use crate::{ExtNonNegF64, Tetromino};

/// Handles the information of which pieces to spawn during a game.
///
/// To actually generate [`Tetromino`]s, the [`TetrominoGenerator::with_rng`] method needs to be used to yield an [`Iterator`].
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TetrominoGenerator {
    /// Uniformly random piece generator.
    Uniform,
    /// Standard 'bag' generator.
    ///
    /// Stock works by picking `n` copies of each [`Tetromino`] type, and then uniformly randomly
    /// handing them out until a lower stock threshold is reached and restocked with `n` copies.
    /// A multiplicity of `1` and restock threshold of `0` corresponds to the common 7-Bag.
    Stock {
        /// The number of each  piece type left in the bag.
        tets_stocked: [u32; Tetromino::VARIANTS.len()],
        /// How many of each piece type to refill with.
        restock_multiplicity: NonZeroU32,
    },

    /// Experimental generator based off of how many times each [`Tetromino`] type has been seen
    /// so far, *relative* to the others.
    BalanceOut {
        /// The relative number of times each piece type has been seen more/less than the others.
        ///
        /// Note that this gets normalized, i.e. all entries are decremented together until
        /// one is `0` and we only get the offset between the lowest count and the others.
        tets_relative_counts: [u32; Tetromino::VARIANTS.len()],
    },

    /// Recency/history-based piece generator.
    ///
    /// This generator keeps track of the last time each [`Tetromino`] type has been seen.
    /// It picks pieces by weighing them by this information as given by the `snap` field, which is
    /// used as the exponent of the last time the piece was seen. Note that this makes it impossible
    /// for a piece that was just played (index `0`) to be played again.
    Recency {
        /// The last time a piece was seen.
        ///
        /// `0` here denotes that it was the most recent piece generated.
        tets_last_emitted: [u32; Tetromino::VARIANTS.len()],
        /// Determines how strongly it weighs pieces not generated in a while.
        factor: ExtNonNegF64,
        /// Whether factor is used as base or exponent.
        is_base_not_exp: bool,
    },
}

impl Default for TetrominoGenerator {
    fn default() -> Self {
        Self::snappy_recency()
    }
}

impl TetrominoGenerator {
    /// Initialize a typical 7-Bag instance of the [`TetrominoGenerator::Stock`] variant.
    pub const fn bag() -> Self {
        Self::Stock {
            tets_stocked: [1; Tetromino::VARIANTS.len()],
            restock_multiplicity: NonZeroU32::MIN,
        }
    }

    /// Initialize a default instance of the [`TetrominoGenerator::Recency`] variant.
    pub const fn snappy_recency() -> Self {
        // SAFETY: `+0.0 <= 2.5`.
        let factor = unsafe { ExtNonNegF64::new_unchecked(2.5) };
        Self::Recency {
            tets_last_emitted: [0; Tetromino::VARIANTS.len()],
            factor,
            is_base_not_exp: false,
        }
    }

    /// Initialize an instance of the [`TetrominoGenerator::BalanceRelative`] variant.
    pub const fn balance_out() -> Self {
        Self::BalanceOut {
            tets_relative_counts: [0; Tetromino::VARIANTS.len()],
        }
    }

    /// Method that allows `TetrominoGenerator` to be used as [`Iterator`].
    pub fn with_rng<'a, 'b, R: Rng>(&'a mut self, rng: &'b mut R) -> WithRng<'a, 'b, R> {
        WithRng {
            tetromino_generator: self,
            rng,
        }
    }
}

/// Struct produced from [`TetrominoGenerator::with_rng`] which implements [`Iterator`].
pub struct WithRng<'a, 'b, R: Rng> {
    /// Selected tetromino generator to use as information source.
    pub tetromino_generator: &'a mut TetrominoGenerator,
    /// Thread random number generator for raw soure of randomness.
    pub rng: &'b mut R,
}

impl<'a, 'b, R: Rng> Iterator for WithRng<'a, 'b, R> {
    type Item = Tetromino;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.tetromino_generator {
            TetrominoGenerator::Uniform => Some(Tetromino::VARIANTS[self.rng.random_range(0..=6)]),

            TetrominoGenerator::Stock {
                tets_stocked,
                restock_multiplicity,
            } => {
                // Restock all pieces if stock empty.
                if tets_stocked.iter().sum::<u32>() == 0 {
                    for tet_stock in tets_stocked.iter_mut() {
                        *tet_stock = tet_stock.saturating_add(restock_multiplicity.get());
                    }
                }

                // Produce proportional weights for all remaining pieces.
                let weights = tets_stocked.iter();

                // SAFETY:
                // * `InvalidInput`: Iterator `weights` is nonempty (7).
                // * `InvalidWeight`: No weights are NaN or negative (all 0 or 1).
                // * `InsufficientNonZero`: Sum of weights is always nonzero (we had restocked).
                // * `Overflow`: Sum of weights can't overflow (1 <= .. <= 7).
                let idx = WeightedIndex::new(weights).unwrap().sample(&mut self.rng);

                // Update tetromino availability.
                tets_stocked[idx] -= 1;

                Some(Tetromino::VARIANTS[idx])
            }

            TetrominoGenerator::BalanceOut {
                tets_relative_counts,
            } => {
                // SAFETY: `self.relative_counts` always has a minimum.
                let min = *tets_relative_counts.iter().min().unwrap();
                if min > 0 {
                    for x in tets_relative_counts.iter_mut() {
                        *x -= min;
                    }
                }

                // Alternative get_weight's: 2.0f64.powf(f64::from(n)).recip() ; f64::from(1 + n).recip()
                let get_weight = |&n| f64::from(n).exp().recip().max(f64::MIN);
                let weights = tets_relative_counts.iter().map(get_weight);

                // SAFETY:
                // * `InvalidInput`: Iterator `weights` is nonempty (7).
                // * `InvalidWeight`: No weights are NaN or negative (exp(u32) >= 1 then .recip().max(f64::MIN)).
                // * `InsufficientNonZero`: Sum of weights is always nonzero (at least one pieces_relative_counts entry is 0 and therefore weight 1).
                // * `Overflow`: `f64`s do not lead to overflow errors in `WeightedIndex::new`.
                let idx = WeightedIndex::new(weights).unwrap().sample(&mut self.rng);

                // Update individual tetromino counter.
                tets_relative_counts[idx] = tets_relative_counts[idx].saturating_add(1);

                Some(Tetromino::VARIANTS[idx])
            }

            TetrominoGenerator::Recency {
                tets_last_emitted,
                factor,
                is_base_not_exp,
            } => {
                // Update all tetromino last_played values.
                for piece_last_emitted in tets_last_emitted.iter_mut() {
                    *piece_last_emitted = piece_last_emitted.saturating_add(1);
                }

                // Alternative get_weight's: f64::from(n).exp()
                let get_weight = |&n| {
                    if *is_base_not_exp {
                        // Ensure weight is positive.
                        factor.get().powf(f64::from(n)).max(f64::MIN_POSITIVE)
                    } else {
                        f64::from(n).powf(factor.get())
                    }
                };
                let weights = tets_last_emitted.iter().map(get_weight);

                // SAFETY:
                // * `InvalidInput`: Iterator `weights` is nonempty (7).
                // * `InvalidWeight`: No weights are NaN or negative (...).
                // * `InsufficientNonZero`: Sum of weights is always nonzero (...).
                // * `Overflow`: `f64`s do not lead to overflow errors in `WeightedIndex::new`.
                let idx = WeightedIndex::new(weights).unwrap().sample(&mut self.rng);

                // Update individual tetromino counter.
                tets_last_emitted[idx] = 0;

                Some(Tetromino::VARIANTS[idx])
            }
        }
    }
}
