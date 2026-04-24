/*! Run with `cargo run --example tui_customized_engine` */

use std::time::{Duration, Instant};

use crossterm::{
    ExecutableCommand,
    cursor::MoveTo,
    event::{self, KeyCode},
    style::Print,
    terminal,
};
use falling_tetromino_engine::{
    Board, Button, GameLimits, GameRng, Input, Phase, Piece, PieceRotator, Stat, Tetromino,
    TetrominoGenerator, UpdateGameError, game_core,
};
use rand::RngExt;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // NOTE: Here we re-define the *core* Game type using our own generator and rotators.
    type Game = game_core::Game<UniformGenerator, DebugRotator>;

    // Initialize game. In-game time starts at 0s.
    let mut game = Game::builder()
        .seed(1234)
        .game_limits(GameLimits::single(Stat::LinesCleared(40), true))
        .build();

    // Prepare terminal to read user inputs directly.
    terminal::enable_raw_mode()?;
    let game_start = Instant::now();
    let mut board_state = game.state().board;

    // Main game loop.
    'game_loop: while !game.has_ended() {
        std::thread::sleep(Duration::from_secs_f32(1. / 60.));

        // Go ahead and process all available inputs, where the updated in-game time = IRL-time-elapsed.
        let in_game_update_time = game_start.elapsed();
        let mut at_least_one_button_input = false;
        'process_inputs: while event::poll(Duration::ZERO)? {
            let event::Event::Key(key_event) = event::read()? else {
                continue; // Don't care about non-keyboard `Event`s.
            };
            if key_event.is_release() {
                continue; // Don't care about `KeyEventKind::Release`.
            }
            let button = match key_event.code {
                KeyCode::Esc => break 'game_loop,
                KeyCode::Left => Button::MoveLeft,
                KeyCode::Right => Button::MoveRight,
                KeyCode::Up => Button::DropHard,
                KeyCode::Down => Button::DropSoft,
                KeyCode::Char('q') => Button::TeleLeft,
                KeyCode::Char('e') => Button::TeleRight,
                KeyCode::Char('w') => Button::TeleDown,
                KeyCode::Char('a') => Button::RotateLeft,
                KeyCode::Char('s') => Button::Rotate180,
                KeyCode::Char('d') => Button::RotateRight,
                KeyCode::Char(' ') => Button::HoldPiece,
                _ => continue, // Don't care about: other `KeyEvent`s.
            };
            match game.update(in_game_update_time, Some(Input::Activate(button))) {
                Err(UpdateGameError::AlreadyEnded) => break 'process_inputs,
                Err(UpdateGameError::TargetTimeInPast) => unreachable!(),
                Ok(_notifs) => {}
            }
            // To not leave a button 'held' indefinitely, we unpress it immediately.
            let _ = game.update(in_game_update_time, Some(Input::Deactivate(button)));
            at_least_one_button_input = true;
        }

        // Ensure game is updated even when no new inputs have been processed.
        if !at_least_one_button_input {
            let _ = game.update(in_game_update_time, None);
        }

        // Calculate board state to show.
        let mut new_board_state = game.state().board;
        if let Some(piece) = game.phase().piece() {
            for ((x, y), tile_id) in piece.tiles() {
                new_board_state[y as usize][x as usize] = Some(tile_id);
            }
        }

        // Redraw board only if necessary - This simple optimization avoids having to access the terminal if we don't need to.
        if new_board_state != board_state {
            for (y, line) in new_board_state.iter().take(20).rev().enumerate() {
                for (x, tile) in line.iter().enumerate() {
                    let tile_str = if tile.is_some() { "[]" } else { " ." };
                    std::io::stdout()
                        .execute(MoveTo((2 * x) as u16, y as u16))?
                        .execute(Print(tile_str))?;
                }
            }
            std::io::stdout().execute(Print(format!(
                "{} lines left",
                40u32.saturating_sub(game.state().lineclears)
            )))?;
            board_state = new_board_state;
        }
    }

    terminal::disable_raw_mode()?;
    match game.phase() {
        Phase::GameEnd { is_win: true, .. } => println!(" + You won!"),
        Phase::GameEnd { is_win: false, .. } => println!(" - Game over!"),
        _ => println!(" ~ Stopped"),
    }
    Ok(())
}

// Our custom rotation system.
// It is very simple: It doesn't actually do any kicks when changing orientation*.
//
// (*Instead, it just relies on how the engine outputs raw piece offset data,
// which is always aligned to bottom left coordinate of a piece.
// For example, 'I' will always pivot around it lower left unit. Try it out!)
struct DebugRotator;

impl PieceRotator for DebugRotator {
    fn rotate(&self, piece: &Piece, board: &Board, right_turns: i8) -> Option<Piece> {
        let rotated_piece = self.free_rotate(piece, right_turns);
        rotated_piece.fits_on(board).then_some(rotated_piece)
    }

    fn free_rotate(&self, piece: &Piece, right_turns: i8) -> Piece {
        Piece {
            orientation: piece.orientation.turn_right(right_turns),
            ..*piece
        }
    }
}

// Our custom tetromino generator.
// It is very simple: It just picks tetrominos completely randomly*.
//
// (*This means it doesn't need to remember any history or other data: It is memoryless.)
#[derive(Clone)]
struct UniformGenerator;

impl TetrominoGenerator for UniformGenerator {
    fn from_rng(_rng: &mut GameRng) -> Self {
        // We don't need rng to randomize the initial state of this generator.
        UniformGenerator
    }

    fn using_rng<'a>(&'a mut self, rng: &'a mut GameRng) -> impl Iterator<Item = Tetromino> + 'a {
        std::iter::from_fn(|| {
            let tet = Tetromino::VARIANTS[rng.random_range(0..7)];
            Some(tet)
        })
    }
}
