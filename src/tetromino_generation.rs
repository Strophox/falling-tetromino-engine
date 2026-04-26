/*!
Random generation of [`Tetromino`]s.
*/

use std::num::NonZeroU32;

use rand::{
    self, RngExt,
    distr::{Distribution, weighted::WeightedIndex},
};

use crate::{ExtNonNegF64, GameRng, Tetromino};

/// Handles the information of which pieces to spawn during a game.
///
/// To actually generate [`Tetromino`]s, the [`TetrominoGenerator::using_rng`] method needs to be used to yield something that is an [`Iterator`].
pub trait TetrominoGenerator {
    /// Method to construct and initialize the `TetrominoGenerator`.
    fn from_rng(rng: &mut GameRng) -> Self;

    /// Method that allows `TetrominoGenerator` to be used as [`Iterator`].
    fn using_rng<'a>(&'a mut self, rng: &'a mut GameRng) -> impl Iterator<Item = Tetromino> + 'a;
}

/// Standard tetromino generator implementations.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StdTetGen {
    /// Uniform random piece generator that might try to avoid repetition at least once.
    Reroll(RerollGen),

    /// Standard 'bag' generator.
    ///
    /// Stock works by picking `n` copies of each [`Tetromino`] type, and then uniformly randomly
    /// handing them out until a lower stock threshold is reached and restocked with `n` copies.
    /// A multiplicity of `1` and restock threshold of `0` corresponds to the common 7-Bag.
    Stock(StockGen),

    /// Experimental generator based off of how many times each [`Tetromino`] type has been seen
    /// so far, *relative* to the others.
    BalanceOut(BalanceOutGen),

    /// Recency/history-based piece generator.
    ///
    /// This generator keeps track of the last time each [`Tetromino`] type has been seen.
    /// It picks pieces by weighing them by this information as given by the `snap` field, which is
    /// used as the exponent of the last time the piece was seen. Note that this makes it impossible
    /// for a piece that was just played (index `0`) to be played again.
    Recency(RecencyGen),
}

impl StdTetGen {
    /// Initialize a uniformly random generator variant.
    pub const fn uniform() -> Self {
        Self::Reroll(RerollGen {
            tet_last_emitted: None,
            aversion_to_last: 0,
        })
    }

    /// Initialize a classic random generator that is uniformly random but retries once.
    pub const fn classic() -> Self {
        Self::Reroll(RerollGen {
            tet_last_emitted: None,
            aversion_to_last: 1,
        })
    }

    /// Initialize a typical 7-Bag instance of the [`StdTetGen::Stock`] variant.
    pub const fn bag() -> Self {
        Self::Stock(StockGen {
            tets_stocked: [1; Tetromino::VARIANTS.len()],
            restock_multiplicity: NonZeroU32::MIN,
        })
    }

    /// Initialize an instance of the [`StdTetGen::BalanceOut`] variant.
    pub const fn balance_out() -> Self {
        Self::BalanceOut(BalanceOutGen {
            tets_relative_tallies: [0; Tetromino::VARIANTS.len()],
        })
    }

    /// Initialize a default instance of the [`StdTetGen::Recency`] variant.
    pub const fn snappy() -> Self {
        // SAFETY: `+0.0 <= 2.5`.
        let factor = unsafe { ExtNonNegF64::new_unchecked(2.5) };
        Self::Recency(RecencyGen {
            tets_last_emitted: [0; Tetromino::VARIANTS.len()],
            factor,
            is_base_not_exp: false,
        })
    }
}

impl Default for StdTetGen {
    fn default() -> Self {
        StdTetGen::snappy()
    }
}

/// Struct produced from [`TetrominoGenerator::using_rng`] which implements [`Iterator`].
pub struct StdUsingRng<'a> {
    /// Selected tetromino generator to use as information source.
    pub std_tet_gen: &'a mut StdTetGen,
    /// Thread random number generator for raw soure of randomness.
    pub rng: &'a mut GameRng,
}

impl TetrominoGenerator for StdTetGen {
    fn from_rng(_rng: &mut GameRng) -> Self {
        Self::snappy()
    }

    fn using_rng<'a>(&'a mut self, rng: &'a mut GameRng) -> impl Iterator<Item = Tetromino> + 'a {
        StdUsingRng {
            std_tet_gen: self,
            rng,
        }
    }
}

impl<'a> Iterator for StdUsingRng<'a> {
    type Item = Tetromino;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.std_tet_gen {
            StdTetGen::Reroll(reroll_gen) => reroll_gen.using_rng(self.rng).next(),
            StdTetGen::Stock(stock_gen) => stock_gen.using_rng(self.rng).next(),
            StdTetGen::BalanceOut(balance_out_gen) => balance_out_gen.using_rng(self.rng).next(),
            StdTetGen::Recency(recency_gen) => recency_gen.using_rng(self.rng).next(),
        }
    }
}

/// Uniform random piece generator that might try to avoid repetition at least once.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RerollGen {
    /// The tetromino that was last generated.
    #[cfg_attr(feature = "serde", serde(rename = "lasttet"))]
    pub tet_last_emitted: Option<Tetromino>,

    /// How many times we may reroll to get a different piece until we give up.
    #[cfg_attr(feature = "serde", serde(rename = "aversion"))]
    pub aversion_to_last: u32,
}

impl TetrominoGenerator for RerollGen {
    fn from_rng(_rng: &mut GameRng) -> Self {
        RerollGen {
            tet_last_emitted: None,
            aversion_to_last: 0,
        }
    }

    fn using_rng<'a>(&'a mut self, rng: &'a mut GameRng) -> impl Iterator<Item = Tetromino> + 'a {
        let RerollGen {
            tet_last_emitted,
            aversion_to_last,
        } = self;
        std::iter::from_fn(|| {
            let mut new_tet = Tetromino::VARIANTS[rng.random_range(0..=6)];

            if let Some(old_tet) = *tet_last_emitted {
                for _ in 0..*aversion_to_last {
                    // New tetromino found, we're done.
                    if new_tet != old_tet {
                        break;
                    }
                    // Retry.
                    new_tet = Tetromino::VARIANTS[rng.random_range(0..=6)];
                }
            }

            Some(new_tet)
        })
    }
}

/// Standard 'bag' generator.
///
/// Stock works by picking `n` copies of each [`Tetromino`] type, and then uniformly randomly
/// handing them out until a lower stock threshold is reached and restocked with `n` copies.
/// A multiplicity of `1` and restock threshold of `0` corresponds to the common 7-Bag.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StockGen {
    /// The number of each  piece type left in the bag.
    #[cfg_attr(feature = "serde", serde(rename = "stocked"))]
    pub tets_stocked: [u32; Tetromino::VARIANTS.len()],

    /// How many of each piece type to refill with.
    #[cfg_attr(feature = "serde", serde(rename = "bagsize"))]
    pub restock_multiplicity: NonZeroU32,
}

impl TetrominoGenerator for StockGen {
    fn from_rng(_rng: &mut GameRng) -> Self {
        StockGen {
            tets_stocked: [0; Tetromino::VARIANTS.len()],
            restock_multiplicity: NonZeroU32::MIN,
        }
    }

    fn using_rng<'a>(&'a mut self, rng: &'a mut GameRng) -> impl Iterator<Item = Tetromino> + 'a {
        let StockGen {
            tets_stocked,
            restock_multiplicity,
        } = self;
        std::iter::from_fn(|| {
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
            let idx = WeightedIndex::new(weights).unwrap().sample(rng);

            // Update tetromino availability.
            tets_stocked[idx] -= 1;

            Some(Tetromino::VARIANTS[idx])
        })
    }
}

/// Experimental generator based off of how many times each [`Tetromino`] type has been seen
/// so far, *relative* to the others.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BalanceOutGen {
    /// The relative number of times each piece type has been seen more/less than the others.
    ///
    /// Note that this gets normalized, i.e. all entries are decremented together until
    /// one is `0` and we only get the offset between the lowest count and the others.
    #[cfg_attr(feature = "serde", serde(rename = "tallies"))]
    pub tets_relative_tallies: [u32; Tetromino::VARIANTS.len()],
}

impl TetrominoGenerator for BalanceOutGen {
    fn from_rng(_rng: &mut GameRng) -> Self {
        BalanceOutGen {
            tets_relative_tallies: [0; Tetromino::VARIANTS.len()],
        }
    }

    fn using_rng<'a>(&'a mut self, rng: &'a mut GameRng) -> impl Iterator<Item = Tetromino> + 'a {
        let BalanceOutGen {
            tets_relative_tallies,
        } = self;
        std::iter::from_fn(|| {
            // SAFETY: `self.relative_counts` always has a minimum.
            let min = *tets_relative_tallies.iter().min().unwrap();
            if min > 0 {
                for x in tets_relative_tallies.iter_mut() {
                    *x -= min;
                }
            }

            // Alternative get_weight's: 2.0f64.powf(f64::from(n)).recip() ; f64::from(1 + n).recip()
            let get_weight = |&n| f64::from(n).exp().recip().max(f64::MIN);
            let weights = tets_relative_tallies.iter().map(get_weight);

            // SAFETY:
            // * `InvalidInput`: Iterator `weights` is nonempty (7).
            // * `InvalidWeight`: No weights are NaN or negative (exp(u32) >= 1 then .recip().max(f64::MIN)).
            // * `InsufficientNonZero`: Sum of weights is always nonzero (at least one pieces_relative_counts entry is 0 and therefore weight 1).
            // * `Overflow`: `f64`s do not lead to overflow errors in `WeightedIndex::new`.
            let idx = WeightedIndex::new(weights).unwrap().sample(rng);

            // Update individual tetromino counter.
            tets_relative_tallies[idx] = tets_relative_tallies[idx].saturating_add(1);

            Some(Tetromino::VARIANTS[idx])
        })
    }
}

/// Recency/history-based piece generator.
///
/// This generator keeps track of the last time each [`Tetromino`] type has been seen.
/// It picks pieces by weighing them by this information as given by the `snap` field, which is
/// used as the exponent of the last time the piece was seen. Note that this makes it impossible
/// for a piece that was just played (index `0`) to be played again.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RecencyGen {
    /// The last time a piece was seen.
    ///
    /// `0` here denotes that it was the most recent piece generated.
    #[cfg_attr(feature = "serde", serde(rename = "lasttets"))]
    pub tets_last_emitted: [u32; Tetromino::VARIANTS.len()],

    /// Determines how strongly it weighs pieces not generated in a while.
    #[cfg_attr(feature = "serde", serde(rename = "factor"))]
    pub factor: ExtNonNegF64,

    /// Whether factor is used as base or exponent.
    #[cfg_attr(feature = "serde", serde(rename = "is_base"))]
    pub is_base_not_exp: bool,
}

impl TetrominoGenerator for RecencyGen {
    fn from_rng(_rng: &mut GameRng) -> Self {
        RecencyGen {
            tets_last_emitted: [0; Tetromino::VARIANTS.len()],
            factor: 2.5.try_into().unwrap(),
            is_base_not_exp: false,
        }
    }

    fn using_rng<'a>(&'a mut self, rng: &'a mut GameRng) -> impl Iterator<Item = Tetromino> + 'a {
        let RecencyGen {
            tets_last_emitted,
            factor,
            is_base_not_exp,
        } = self;
        std::iter::from_fn(|| {
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
            let idx = WeightedIndex::new(weights).unwrap().sample(rng);

            // Update individual tetromino counter.
            tets_last_emitted[idx] = 0;

            Some(Tetromino::VARIANTS[idx])
        })
    }
}
