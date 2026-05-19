/*!
Rotation of tetromino [`Piece`]s.
*/

use crate::core::{Board, CoordExt, Offset, Orientation, Piece, Tetromino};

/// Handles the logic of how to rotate a tetromino in play.
pub trait PieceRotator {
    /// Tries to rotate a piece with the chosen `PieceRotator`.
    ///
    /// This will return `Some(new_piece)` if the `old_piece`, when rotated
    /// `right_turns`-times from its position, fits onto the board in the form of `new_piece`.
    /// It should return `None` otherwise.
    ///
    /// In particular, rotating a piece `0` times tests whether piece fits in its *current* position.
    fn rotate<TileData>(
        &self,
        piece: &Piece,
        board: &Board<TileData>,
        right_turns: i8,
    ) -> Option<Piece>;

    /// This rotates the piece as if it were in free space ('completely freely').
    /// This should correspond to [`PieceRotator::rotate`] if the first kick never fails.
    fn free_rotate(&self, piece: &Piece, right_turns: i8) -> Piece;
}

/// Standard piece rotator implementations.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MiscPceRots {
    /// The 'Ocular' rotation system.
    #[default]
    Ocular,
    /// The left-handed variant of the classic, kick-less rotation system, e.g. used in the Gameboy version.
    ClassicL,
    /// The right-handed variant of the classic, kick-less rotation system, e.g. used in the NES version.
    ClassicR,
    /// The Super Rotation System.
    Super,
}

impl PieceRotator for MiscPceRots {
    fn rotate<TileData>(
        &self,
        piece: &Piece,
        board: &Board<TileData>,
        right_turns: i8,
    ) -> Option<Piece> {
        match self {
            MiscPceRots::Ocular => OcularRot.rotate(piece, board, right_turns),
            MiscPceRots::ClassicL => ClassicLRot.rotate(piece, board, right_turns),
            MiscPceRots::ClassicR => ClassicRRot.rotate(piece, board, right_turns),
            MiscPceRots::Super => SuperRot.rotate(piece, board, right_turns),
        }
    }

    fn free_rotate(&self, piece: &Piece, right_turns: i8) -> Piece {
        match self {
            MiscPceRots::Ocular => ocular_rotate::<()>(piece, None, right_turns),
            MiscPceRots::ClassicL => classic_rotate::<()>(piece, None, right_turns, true),
            MiscPceRots::ClassicR => classic_rotate::<()>(piece, None, right_turns, false),
            MiscPceRots::Super => super_rotate::<()>(piece, None, right_turns),
        }
        .unwrap()
    }
}

/// The left-handed variant of the classic, kick-less rotation system, e.g. used in the Gameboy version.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClassicLRot;

impl PieceRotator for ClassicLRot {
    fn rotate<TileData>(
        &self,
        piece: &Piece,
        board: &Board<TileData>,
        right_turns: i8,
    ) -> Option<Piece> {
        classic_rotate(piece, Some(board), right_turns, true)
    }

    fn free_rotate(&self, piece: &Piece, right_turns: i8) -> Piece {
        classic_rotate::<()>(piece, None, right_turns, true).unwrap()
    }
}

/// The right-handed variant of the classic, kick-less rotation system, e.g. used in the NES version.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClassicRRot;

impl PieceRotator for ClassicRRot {
    fn rotate<TileData>(
        &self,
        piece: &Piece,
        board: &Board<TileData>,
        right_turns: i8,
    ) -> Option<Piece> {
        classic_rotate(piece, Some(board), right_turns, false)
    }

    fn free_rotate(&self, piece: &Piece, right_turns: i8) -> Piece {
        classic_rotate::<()>(piece, None, right_turns, false).unwrap()
    }
}

fn classic_rotate<TileData>(
    piece: &Piece,
    board: Option<&Board<TileData>>,
    right_turns: i8,
    is_l_not_r: bool,
) -> Option<Piece> {
    use Orientation::*;

    let variant_offset = if is_l_not_r { 0 } else { 1 };

    #[rustfmt::skip]
    let kick = match right_turns.rem_euclid(4) {
        // "Rotate into same orientation".
        0 => (0, 0),
        // Check if the simulated 180 rotation fits.
        2 => match piece.tetromino {
            Tetromino::O | Tetromino::I | Tetromino::S | Tetromino::Z => (0, 0),
            Tetromino::T | Tetromino::L | Tetromino::J => match piece.orientation {
                N => (0, -1), S => (0, 1), E => (-1, 0), W => (1, 0),
            },
        }
        // One right or left rotation.
        rot => match piece.tetromino {
            Tetromino::O => (0, 0), // ⠶
            Tetromino::I => match piece.orientation {
                N | S => (1+variant_offset, -1), // ⠤⠤ -> ⡇
                E | W => (-1-variant_offset, 1), // ⡇  -> ⠤⠤
            },
            Tetromino::S | Tetromino::Z => match piece.orientation {
                N | S => (variant_offset, 0),  // ⠴⠂ -> ⠳  // ⠲⠄ -> ⠞
                E | W => (-variant_offset, 0), // ⠳  -> ⠴⠂ // ⠞  -> ⠲⠄
            },
            Tetromino::T | Tetromino::L | Tetromino::J => match piece.orientation {
                N => if rot == 3 { ( 0,-1) } else { ( 1,-1) }, // ⠺  <- ⠴⠄ -> ⠗  // ⠹  <- ⠤⠆ -> ⠧  // ⠼  <- ⠦⠄ -> ⠏
                E => if rot == 3 { (-1, 1) } else { (-1, 0) }, // ⠴⠄ <- ⠗  -> ⠲⠂ // ⠤⠆ <- ⠧  -> ⠖⠂ // ⠦⠄ <- ⠏  -> ⠒⠆
                S => if rot == 3 { ( 1, 0) } else { ( 0, 0) }, // ⠗  <- ⠲⠂ -> ⠺  // ⠧  <- ⠖⠂ -> ⠹  // ⠏  <- ⠒⠆ -> ⠼
                W => if rot == 3 { ( 0, 0) } else { ( 0, 1) }, // ⠲⠂ <- ⠺  -> ⠴⠄ // ⠖⠂ <- ⠹  -> ⠤⠆ // ⠒⠆ <- ⠼  -> ⠦⠄
            },
        }
    };

    if let Some(board) = board {
        // Explicitly check piece if testing against board.
        piece.reoriented_offset_on(board, right_turns, kick).ok()
    } else {
        // Otherwise always return kicked piece.
        Some(Piece {
            tetromino: piece.tetromino,
            orientation: piece.orientation.turn_right(right_turns),
            position: piece.position.add(kick),
        })
    }
}

/// The Super Rotation System.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SuperRot;

impl PieceRotator for SuperRot {
    fn rotate<TileData>(
        &self,
        piece: &Piece,
        board: &Board<TileData>,
        right_turns: i8,
    ) -> Option<Piece> {
        super_rotate(piece, Some(board), right_turns)
    }

    fn free_rotate(&self, piece: &Piece, right_turns: i8) -> Piece {
        super_rotate::<()>(piece, None, right_turns).unwrap()
    }
}

fn super_rotate<TileData>(
    piece: &Piece,
    board: Option<&Board<TileData>>,
    right_turns: i8,
) -> Option<Piece> {
    use Orientation::*;

    #[rustfmt::skip]
    let kicks = match right_turns.rem_euclid(4) {
        // "Rotate into same orientation".
        0 => &[(0, 0)],
        // Some basic 180 rotation I came up with.
        2 => match piece.tetromino {
            Tetromino::O => &[(0, 0)][..],
            Tetromino::I | Tetromino::S | Tetromino::Z | Tetromino::T | Tetromino::L | Tetromino::J => match piece.orientation {
                N => &[( 0,-1), ( 0, 0)],
                E => &[(-1, 0), ( 0, 0)],
                S => &[( 0, 1), ( 0, 0)],
                W => &[( 1, 0), ( 0, 0)],
            },
        }
        // One right or left rotation.
        rot => {
            let left = rot == 3;
            match piece.tetromino {
                Tetromino::O => &[(0, 0)][..],
                Tetromino::I => match piece.orientation {
                    N => if left { &[( 1,-2), ( 0,-2), ( 3,-2), ( 0, 0), ( 3,-3)] }
                            else { &[( 2,-2), ( 0,-2), ( 3,-2), ( 0,-3), ( 3, 0)] },
                    E => if left { &[(-2, 2), ( 0, 2), (-3, 2), ( 0, 3), (-3, 0)] }
                            else { &[(-2, 1), (-3, 1), ( 0, 1), (-3, 3), ( 0, 0)] },
                    S => if left { &[( 2,-1), ( 3,-1), ( 0,-1), ( 3,-3), ( 0, 0)] }
                            else { &[( 1,-1), ( 3,-1), ( 0,-1), ( 3, 0), ( 0,-3)] },
                    W => if left { &[(-1, 1), (-3, 1), ( 0, 1), (-3, 0), ( 0, 3)] }
                            else { &[(-1, 2), ( 0, 2), (-3, 2), ( 0, 0), (-3, 3)] },
                },
                Tetromino::S | Tetromino::Z | Tetromino::T | Tetromino::L | Tetromino::J => match piece.orientation {
                    N => if left { &[( 0,-1), ( 1,-1), ( 1, 0), ( 0,-3), ( 1,-3)] }
                            else { &[( 1,-1), ( 0,-1), ( 0, 0), ( 1,-3), ( 0,-3)] },
                    E => if left { &[(-1, 1), ( 0, 1), ( 0, 0), (-1, 3), ( 0, 3)] }
                            else { &[(-1, 0), ( 0, 0), ( 0,-1), (-1, 2), ( 0, 2)] },
                    S => if left { &[( 1, 0), ( 0, 0), (-1, 1), ( 1,-2), ( 0,-2)] }
                            else { &[( 0, 0), ( 1, 0), ( 1, 1), ( 0,-2), ( 1,-2)] },
                    W => if left { &[( 0, 0), (-1, 0), (-1,-1), ( 0, 2), (-1, 2)] }
                            else { &[( 0, 1), (-1, 1), (-1, 0), ( 0, 3), (-1, 3)] },
                },
            }
        },
    };

    if let Some(board) = board {
        // Explicitly check piece if testing against board.
        piece.find_reoriented_offset_on(board, right_turns, kicks.iter().copied())
    } else {
        // Otherwise always return kicked piece.
        Some(Piece {
            tetromino: piece.tetromino,
            orientation: piece.orientation.turn_right(right_turns),
            position: piece.position.add(kicks[0]),
        })
    }
}

/// The 'Ocular' rotation system.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OcularRot;

impl PieceRotator for OcularRot {
    fn rotate<TileData>(
        &self,
        piece: &Piece,
        board: &Board<TileData>,
        right_turns: i8,
    ) -> Option<Piece> {
        ocular_rotate(piece, Some(board), right_turns)
    }

    fn free_rotate(&self, piece: &Piece, right_turns: i8) -> Piece {
        ocular_rotate::<()>(piece, None, right_turns).unwrap()
    }
}

/*
The basic ideas of Ocular Rotation are not that hard:
1. Use symmetry for kick tables (e.g. O↻ := ⇔O↺).
2. For the remaining, unique entries, list out kicks that look intuitive or desirable.

Rotation Symmetries to figure out kicks from existing kicks:
    Notation:
        OISZTLJ   (↑→↓←)         ↺↻
        ^shapes.  ^orientation.  ^rotation direction.
    And "⇔" means "mirrored horizontally".

Given we know how  O↺  then we can figure out [the rest of O]:
         O↻  :=  ⇔ O↺

Given we know how  I(↑→)↺  then we can figure out [the rest of I]:
     I(↑→)↻  :=  ⇔ I(↑→)↺

Given we know how  S(↑→)↺↻  then we can figure out [all of Z]:
    Z(↑→)↺↻  :=  ⇔ S(↑→)↻↺

Given we know how  T(↑→↓←)↺  then we can figure out [the rest of T]:
     T(↑↓)↻  :=  ⇔ T(↑↓)↺
     T(→←)↻  :=  ⇔ T(←→)↺

Given we know how  L(↑→↓←)↺↻  then we can figure out [all of J]:
    J(↑↓)↺↻  :=  ⇔ L(↑↓)↻↺"
    J(→←)↺↻  :=  ⇔ L(←→)↻↺"
*/
#[rustfmt::skip]
fn ocular_rotate<TileData>(piece: &Piece, board: Option<&Board<TileData>>, right_turns: i8) -> Option<Piece> {
    use Orientation::*;

    // Figure out whether to turn 'right' (90° CW), 'left' (90° CCW), 'around' (180°) or not at all (0°).
    let mut kicks: Box<dyn Iterator<Item = Offset>> = match right_turns.rem_euclid(4) {
        // 0° - "Rotate into same orientation".
        0 => {
            let kicks = [(0, 0)].iter().copied();

            Box::new(kicks)
        }

        // 180° - Rotate 'around'.
        2 => {
            let mut lookup_tetromino = piece.tetromino;
            let mut lookup_orientation = piece.orientation;
            let mut apply_mirror = false;
            // Precompute mirror / horizontal reorientation to possibly change lookup_orientation once (see T, J).
            let reorient_horizontally = match piece.orientation { N => N, E => W, S => S, W => E };

            let unadjusted_kicks = 'lookup: loop {
                break match lookup_tetromino {

                    // Note: O and I have a default, successful 180° rotation due to 180° symmetry.
                    Tetromino::O | Tetromino::I => &[( 0, 0)][..],

                    // Note: S has special 180° rotations that can 'nudge' it diagonally into fitting gaps.
                    // Note: S has a default, successful 180° rotation due to 180° symmetry.
                    Tetromino::S => match lookup_orientation {
                        N | S => &[(-1,-1), ( 0, 0)],
                        E | W => &[( 1,-1), ( 0, 0)],
                    },

                    Tetromino::Z => {
                        // Symmetry: Z's 180° rotation is a mirrored version of S'.
                        lookup_tetromino = Tetromino::S;
                        apply_mirror = true;
                        continue 'lookup;
                    },

                    Tetromino::T => match lookup_orientation {
                        N => &[( 0,-1), ( 0, 0)][..],
                        E => &[(-1, 0), ( 0, 0), (-1,-1)],
                        S => &[( 0, 1), ( 0, 0), ( 0,-1)],
                        W => {
                             // Symmetry: T's 180° rotation oriented West is same as mirrored East.
                            lookup_orientation = reorient_horizontally;
                            apply_mirror = true;
                            continue 'lookup;
                        },
                    },

                    Tetromino::L => match lookup_orientation {
                        N => &[( 0,-1), ( 1,-1), (-1,-1), ( 0, 0), ( 1, 0)][..],
                        E => &[(-1, 0), (-1,-1), ( 0, 0), ( 0,-1)],
                        S => &[( 0, 1), ( 0, 0), (-1, 1), (-1, 0)],
                        W => &[( 1, 0), ( 0, 0), ( 1,-1), ( 1, 1), ( 0, 1)],
                    },

                    Tetromino::J => {
                        // Symmetry: J's 180° rotation is a mirrored version of L's.
                        lookup_tetromino = Tetromino::L;
                        lookup_orientation = reorient_horizontally;
                        apply_mirror = true;
                        continue 'lookup;
                    }
                }
            };

            // Mirror kicks in case we used symmetry to figure out what to do.
            let kicks = unadjusted_kicks.iter().copied().map(move |(x, y)| if apply_mirror { (-x, y) } else { (x, y) });

            Box::new(kicks)
        }

        // ± 90° - Rotate 'right'/'left'.
        rot => {
            // `rot` at this point can only be 1 ('right') or 3 ('left').
            let mut lookup_leftrot = rot == 3;
            let mut lookup_tetromino = piece.tetromino;
            let mut lookup_orientation = piece.orientation;
            // Unlike 180°, mirroring a piece may involve adding a manual offset to make it look symmetric as desired.
            let mut apply_mirror = None;
            // Precompute mirror / horizontal reorientation to possibly change lookup_orientation once (see T, J).
            let reorient_horizontally = match lookup_orientation { N => N, E => W, S => S, W => E };

            let unadjusted_kicks = 'lookup: loop {
                match lookup_tetromino {
                    Tetromino::O => {
                        if lookup_leftrot {
                            break 'lookup &[(-1, 0), (-1,-1), (-1, 1), ( 0, 0)][..];
                        } else  {
                            // Symmetry: O's right rotation is a mirrored version of left rotation.
                            apply_mirror = Some(0);
                            lookup_leftrot = true;
                            continue 'lookup;
                        }
                    },

                    Tetromino::I => {
                        if lookup_leftrot {
                            break 'lookup match lookup_orientation {
                                N | S => &[( 1,-1), ( 1,-2), ( 1,-3), ( 0,-1), ( 0,-2), ( 0,-3), ( 1, 0), ( 0, 0), ( 2,-1), ( 2,-2)],
                                E | W => &[(-2, 1), (-3, 1), (-2, 0), (-3, 0), (-1, 1), (-1, 0), ( 0, 1), ( 0, 0)],
                            };
                        } else  {
                            // Symmetry: I's right rotation is a mirrored version of left rotation.
                            // (Manual x offset due to how engine naïvely positions base shapes.)
                            let dx = match lookup_orientation { N | S => 3, E | W => -3 };
                            apply_mirror = Some(dx);
                            lookup_leftrot = true;
                            continue 'lookup;
                        }
                    },

                    Tetromino::S => break 'lookup match lookup_orientation {
                        N | S => if lookup_leftrot { &[( 0, 0), ( 0,-1), ( 1, 0), (-1,-1)] }
                                              else { &[( 1, 0), ( 1,-1), ( 1, 1), ( 0, 0), ( 0,-1)] },
                        E | W => if lookup_leftrot { &[(-1, 0), ( 0, 0), (-1,-1), (-1, 1), ( 0, 1)] }
                                              else { &[( 0, 0), (-1, 0), ( 0,-1), ( 1, 0), ( 0, 1), (-1, 1)] },
                    },

                    Tetromino::Z => {
                        // Symmetry: Z's left/right rotation is a mirrored version of S' right/left rotation.
                        // (Manual x offset due to how engine naïvely positions base shapes.)
                        let dx = match lookup_orientation { N | S => 1, E | W => -1 };
                        apply_mirror = Some(dx);
                        lookup_tetromino = Tetromino::S;
                        lookup_leftrot = !lookup_leftrot;
                        continue 'lookup;
                    },

                    Tetromino::T => {
                        if lookup_leftrot {
                            break 'lookup match lookup_orientation {
                                N => &[( 0,-1), ( 0, 0), (-1,-1), ( 1,-1), (-1,-2), ( 1, 0)],
                                E => &[(-1, 1), (-1, 0), ( 0, 1), ( 0, 0), (-1,-1), (-1, 2)],
                                S => &[( 1, 0), ( 0, 0), ( 1,-1), ( 0,-1), ( 1,-2), ( 2, 0)],
                                W => &[( 0, 0), (-1, 0), ( 0,-1), (-1,-1), ( 1,-1), ( 0, 1), (-1, 1)],
                            };
                        } else  {
                            // Symmetry: T's right rotation is a mirrored version of left rotation if reoriented.
                            let dx = match lookup_orientation { N | S => 1, E | W => -1 };
                            apply_mirror = Some(dx);
                            lookup_orientation = reorient_horizontally;
                            lookup_leftrot = true;
                            continue 'lookup;
                        }
                    },

                    Tetromino::L => break match lookup_orientation {
                        N => if lookup_leftrot { &[( 0,-1), ( 1,-1), ( 0,-2), ( 1,-2), ( 0, 0), ( 1, 0)] }
                                          else { &[( 1,-1), ( 1, 0), ( 1,-1), ( 2, 0), ( 0,-1), ( 0, 0)] },
                        E => if lookup_leftrot { &[(-1, 1), (-1, 0), (-2, 1), (-2, 0), ( 0, 0), ( 0, 1)] }
                                          else { &[(-1, 0), ( 0, 0), ( 0,-1), (-1,-1), ( 0, 1), (-1, 1)] },
                        S => if lookup_leftrot { &[( 1, 0), ( 0, 0), ( 1,-1), ( 0,-1), ( 0, 1), ( 1, 1)] }
                                          else { &[( 0, 0), ( 0,-1), ( 1,-1), (-1,-1), ( 1, 0), (-1, 0), ( 0, 1)] },
                        W => if lookup_leftrot { &[( 0, 0), (-1, 0), ( 0, 1), ( 1, 0), (-1, 1), ( 1, 1), ( 0,-1), (-1,-1)] }
                                          else { &[( 0, 1), (-1, 1), ( 0, 0), (-1, 0), ( 0, 2), (-1, 2)] },
                    },

                    Tetromino::J => {
                        // Symmetry: J's left/right rotation is a mirrored version of L's right/left rotation if reoriented.
                        let dx = match lookup_orientation { N | S => 1, E | W => -1 };
                        apply_mirror = Some(dx);
                        lookup_tetromino = Tetromino::L;
                        lookup_orientation = reorient_horizontally;
                        lookup_leftrot = !lookup_leftrot;
                        continue 'lookup;
                    }
                }
            };

            // Mirror kicks in case we used symmetry to figure out what to do.
            let kicks = unadjusted_kicks.iter().copied().map(move |(x, y)| if let Some(mx) = apply_mirror { (mx - x, y) } else { (x, y) });

            Box::new(kicks)
        },
    };

    // Using kick table, actually find whether piece fits at a given place.
    if let Some(board) = board {
        // Explicitly check piece if testing against board.
        piece.find_reoriented_offset_on(board, right_turns, kicks)
    } else {
        // Otherwise always return kicked piece.
        Some(Piece {
            tetromino: piece.tetromino,
            orientation: piece.orientation.turn_right(right_turns),
            position: piece.position.add(kicks.next().unwrap())
        })
    }
}
