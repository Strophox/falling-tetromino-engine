/*!
Rotation of tetromino [`Piece`]s.
*/

use crate::{Board, CoordAdd, Offset, Orientation, Piece, Tetromino};

/// Handles the logic of how to rotate a tetromino in play.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RotationSystem {
    /// The 'Ocular' rotation system.
    #[default]
    Ocular,
    /// The raw rotation system resulting from simply reorienting the internal piece representation.
    /// This can be seen as a debug implementation.
    /// The rotation itself looks similar to [`RotationSystem::ClassicL`].
    Debug,
    /// The left-handed variant of the classic, kick-less rotation system, e.g. used in the Gameboy version.
    ClassicL,
    /// The right-handed variant of the classic, kick-less rotation system, e.g. used in the NES version.
    ClassicR,
    /// The Super Rotation System.
    Super,
}

impl RotationSystem {
    /// Tries to rotate a piece with the chosen `RotationSystem`.
    ///
    /// This will return `Ok(new_piece)` if the `old_piece`, when rotated
    /// `right_turns`-times from its position, fit onto the board, ending up as `new_piece`;
    /// It will return None if not.
    ///
    /// In particular, rotating a piece `0` times tests whether piece fits in its *current* position.
    pub fn rotate(&self, piece: &Piece, board: &Board, right_turns: i8) -> Option<Piece> {
        match self {
            RotationSystem::Debug => debug_rotate(piece, Some(board), right_turns),
            RotationSystem::Ocular => ocular_rotate(piece, Some(board), right_turns),
            RotationSystem::ClassicL => classic_rotate(piece, Some(board), right_turns, false),
            RotationSystem::ClassicR => classic_rotate(piece, Some(board), right_turns, true),
            RotationSystem::Super => super_rotate(piece, Some(board), right_turns),
        }
    }

    /// This rotates the piece as if it were in free space ('completely freely').
    /// This should correspond to [`RotationSystem::rotate`] if the first kick never fails.
    pub fn free_rotate(&self, piece: &Piece, right_turns: i8) -> Piece {
        match self {
            RotationSystem::Debug => debug_rotate(piece, None, right_turns),
            RotationSystem::Ocular => ocular_rotate(piece, None, right_turns),
            RotationSystem::ClassicL => classic_rotate(piece, None, right_turns, false),
            RotationSystem::ClassicR => classic_rotate(piece, None, right_turns, true),
            RotationSystem::Super => super_rotate(piece, None, right_turns),
        }
        .unwrap()
    }
}

fn debug_rotate(piece: &Piece, board: Option<&Board>, right_turns: i8) -> Option<Piece> {
    let piece = Piece {
        tetromino: piece.tetromino,
        orientation: piece.orientation.turn_right(right_turns),
        position: piece.position,
    };
    if let Some(board) = board {
        piece.fits_on(board).then_some(piece)
    } else {
        Some(piece)
    }
}

fn classic_rotate(
    piece: &Piece,
    board: Option<&Board>,
    right_turns: i8,
    is_r_not_l: bool,
) -> Option<Piece> {
    use Orientation::*;

    let r_variant_offset = if is_r_not_l { 1 } else { 0 };

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
                N | S => (1+r_variant_offset, -1), // ⠤⠤ -> ⡇
                E | W => (-1-r_variant_offset, 1), // ⡇  -> ⠤⠤
            },
            Tetromino::S | Tetromino::Z => match piece.orientation {
                N | S => (r_variant_offset, 0),  // ⠴⠂ -> ⠳  // ⠲⠄ -> ⠞
                E | W => (-r_variant_offset, 0), // ⠳  -> ⠴⠂ // ⠞  -> ⠲⠄
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

fn super_rotate(piece: &Piece, board: Option<&Board>, right_turns: i8) -> Option<Piece> {
    use Orientation::*;

    #[rustfmt::skip]
    let kicks = match right_turns.rem_euclid(4) {
        // "Rotate into same orientation".
        0 => &[(0, 0)],
        // Some basic 180 rotation I came up with.
        2 => match piece.tetromino {
            Tetromino::O | Tetromino::I | Tetromino::S | Tetromino::Z => &[(0, 0)][..],
            Tetromino::T | Tetromino::L | Tetromino::J => match piece.orientation {
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
fn ocular_rotate(piece: &Piece, board: Option<&Board>, right_turns: i8) -> Option<Piece> {
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
