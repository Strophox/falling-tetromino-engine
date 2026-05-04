# Falling Tetromino Engine examples

All examples can be run with the syntax `cargo run --example nameOfExample`.

## `tui`

This example implements a most minimal way to play a game in the terminal using the engine.

Terminal input is used to control the game, and terminal output is used to display the game state.

> | Keybind | Action | Keybind' | Action' |
> | -: | :- | -: | :- |
> | `←` | **Move left** | `→` | **Move right** |
> | `Q` | Teleport left | `E` | Teleport right |
> | `W` | Teleport down | `S` | Rotate around (180°) |
> | `A` | **Rotate left** (CCW) | `D` | **Rotate right** (CW) |
> | `↓` | Soft drop | `↑` | Hard drop |
> | `Space` | Hold piece | `Esc` | **Exit program** |

## `tui_customized_engine`

This example is completely anologous to [tui](#tui), but it fully customizes the `Game` type to additionally demonstrate the engine's compile-time flexibility (code).
