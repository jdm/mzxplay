use libmzx::robot::{send_robot_to_label, BuiltInLabel, Robots};
use libmzx::{WorldState, Board, Robot, Coordinate};
use libmzx::audio::AudioEngine;
use libmzx::board::{move_level_to, reset_view, reset_update_done};

pub(crate) fn enter_board(
    state: &mut WorldState,
    audio: &dyn AudioEngine,
    board: &mut Board,
    player_pos: Coordinate<u16>,
    robots: &mut [Robot],
) {
    reset_update_done(board, &mut state.update_done);

    if board.mod_file != "*" {
        audio.load_module(&board.mod_file);
    }
    let old_pos = board.player_pos;
    if old_pos != player_pos {
        move_level_to(board, robots, &old_pos, &player_pos, &mut *state.update_done);
    }
    board.player_pos = player_pos;
    reset_view(board);
    state.scroll_locked = false;

    Robots::new(board, robots).foreach(|robot, _id| {
        send_robot_to_label(robot, BuiltInLabel::JustEntered);
    })
}
