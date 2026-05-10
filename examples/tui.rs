/*! Run with `cargo run --example tui` */

use std::time::{Duration, Instant};

use crossterm::{
    ExecutableCommand,
    cursor::MoveTo,
    event::{self, KeyCode},
    style::Print,
    terminal,
};
use falling_tetromino_engine::{Button, Game, GameLimits, Input, Phase, Stat, UpdateGameError};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize game. In-game time starts at 0s.
    let mut game = Game::builder()
        .seed(1234)
        .game_limits(GameLimits::single(Stat::LinesCleared(40), true))
        .build();

    // Prepare terminal to read user inputs directly.

    terminal::enable_raw_mode()?;
    let game_start = Instant::now();
    let mut board_state = game.state().board.clone();
    board_state.resize(20, Default::default());

    // Main game loop.
    'game_loop: while !game.has_ended() {
        std::thread::sleep(Duration::from_secs_f32(1. / 60.));

        // Go ahead and process all available inputs, where the updated in-game time = IRL-time-elapsed.
        let in_game_target_time = game_start.elapsed();
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
            match game.update(in_game_target_time, Some(Input::Activate(button))) {
                Err(UpdateGameError::AlreadyEnded) => break 'process_inputs,
                Err(UpdateGameError::TargetTimeInPast) => unreachable!(),
                Ok(_notifs) => {}
            }
            // To not leave a button 'held' indefinitely, we unpress it immediately.
            let _ = game.update(in_game_target_time, Some(Input::Deactivate(button)));
            at_least_one_button_input = true;
        }

        // Ensure game is updated even when no new inputs have been processed.
        if !at_least_one_button_input {
            let _ = game.update(in_game_target_time, None);
        }

        // Calculate board state to show.
        let mut new_board_state = game.state().board.clone();
        new_board_state.resize(20, Default::default());
        if let Some(piece) = game.phase().piece() {
            for (x, y) in piece.coords() {
                new_board_state[y as usize].0[x as usize] = Some(piece.tetromino.into());
            }
        }

        // Redraw board only if necessary - This simple optimization avoids having to access the terminal if we don't need to.
        if new_board_state != board_state {
            for (y, (line, _)) in new_board_state.iter().take(20).rev().enumerate() {
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
