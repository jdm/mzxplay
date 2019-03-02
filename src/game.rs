use crate::{GameState, PoppedData, StateChange, SdlRenderer};
use crate::audio::{AudioEngine, MusicCallback};
use crate::board::{update_board, enter_board};
use crate::robot::{Robots, RobotId, BuiltInLabel, EvaluatedByteString, send_robot_to_label};
use libmzx::{
    World, Board, Thing, CardinalDirection, Coordinate, Counters, ByteString, KeyPress, WorldState,
    render, draw_messagebox, MessageBoxLine, DoorStatus, door_from_param, param_from_door,
};
use num_traits::ToPrimitive;
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::render::Canvas;
use sdl2::video::Window;
use std::path::Path;

pub const NORTH: (i8, i8) = (0, -1);
pub const SOUTH: (i8, i8) = (0, 1);
pub const EAST: (i8, i8) = (1, 0);
pub const WEST: (i8, i8) = (-1, 0);
pub const IDLE: (i8, i8) = (0, 0);

fn render_game(
    world: &World,
    board_id: usize,
    canvas: &mut Canvas<Window>,
    is_title_screen: bool,
) {
    let mut renderer = SdlRenderer { canvas };
    let robots_start = world.boards[board_id].robot_range.0;
    let robots_end = robots_start + world.boards[board_id].robot_range.1;
    let robots = &world.all_robots[robots_start..robots_end];
    render(
        &world.state,
        (
            world.boards[board_id].upper_left_viewport,
            world.boards[board_id].viewport_size,
        ),
        world.boards[board_id].scroll_offset,
        &world.boards[board_id],
        robots,
        &mut renderer,
        is_title_screen,
    );
}

pub(crate) struct TitleState(pub MusicCallback);
impl GameState for TitleState {
    fn init(&mut self, _world: &mut World, _board_id: &mut usize) {
    }

    fn popped(&mut self, _world: &mut World, _board_id: usize, _data: PoppedData) {
    }

    fn input(
        &mut self,
        event: Event,
        _input_state: &mut InputState,
    ) -> Option<StateChange> {
        match event {
            Event::KeyDown {keycode: Some(Keycode::Escape), ..} =>
                Some(StateChange::PopCurrent(None)),
            Event::KeyDown {keycode: Some(Keycode::P), ..} =>
                Some(StateChange::Replace(Box::new(PlayState(self.0.clone())))),
            _ => None,
        }
    }

    fn tick(
        &mut self,
        world: &mut World,
        world_path: &Path,
        input_state: &InputState,
        counters: &mut Counters,
        board_id: &mut usize,
    ) -> Option<StateChange> {
        tick_game_loop(world, &self.0, world_path, input_state, counters, board_id)
    }

    fn render(
        &mut self,
        world: &World,
        board_id: usize,
        canvas: &mut Canvas<Window>,
    ) {
        render_game(world, board_id, canvas, true);
    }
}

pub struct PlayState(pub MusicCallback);
impl GameState for PlayState {
    fn init(&mut self, world: &mut World, board_id: &mut usize) {
        *board_id = world.starting_board_number.0 as usize;
        let pos = world.boards[*board_id].player_pos;
        enter_board(&mut world.state, &self.0, &mut world.boards[*board_id], pos, &mut world.all_robots);
        world.state.charset = world.state.initial_charset;
        world.state.palette = world.state.initial_palette.clone();
    }

    fn popped(&mut self, world: &mut World, board_id: usize, data: PoppedData) {
        match data {
            PoppedData::MessageBox(rid, label) => {
                let mut robots = Robots::new(&mut world.boards[board_id], &mut world.all_robots);
                let robot = robots.get_mut(rid);
                send_robot_to_label(robot, EvaluatedByteString::no_eval_needed(label));

            }
        }
    }

    fn input(
        &mut self,
        event: Event,
        input_state: &mut InputState,
    ) -> Option<StateChange> {
        match event {
            Event::KeyDown {keycode: Option::Some(Keycode::Escape), ..} =>
                Some(StateChange::PopCurrent(None)),
            Event::KeyDown {timestamp, keycode, keymod, repeat, ..} => {
                let _ = handle_key_input(
                    input_state,
                    timestamp,
                    keycode,
                    keymod,
                    repeat,
                    true,
                );
                None
            }
            Event::KeyUp {timestamp, keycode, keymod, repeat, ..} => {
                let _ = handle_key_input(
                    input_state,
                    timestamp,
                    keycode,
                    keymod,
                    repeat,
                    false,
                );
                None
            }
            _ => None,
        }
    }

    fn tick(
        &mut self,
        world: &mut World,
        world_path: &Path,
        input_state: &InputState,
        counters: &mut Counters,
        board_id: &mut usize,
    ) -> Option<StateChange> {
        tick_game_loop(world, &self.0, world_path, input_state, counters, board_id)
    }

    fn render(
        &mut self,
        world: &World,
        board_id: usize,
        canvas: &mut Canvas<Window>,
    ) {
        render_game(world, board_id, canvas, false);
    }
}

#[derive(Default)]
pub(crate) struct InputState {
    left_pressed: bool,
    right_pressed: bool,
    up_pressed: bool,
    down_pressed: bool,
    space_pressed: bool,
    delete_pressed: bool,
    pressed_keycode: Option<Keycode>,
}

pub(crate) fn reset_view(board: &mut Board) {
    let vwidth = board.viewport_size.0 as u16;
    let vheight = board.viewport_size.1 as u16;

    let xpos = (board.player_pos.0.checked_sub(vwidth / 2))
        .unwrap_or(0)
        .min(board.width as u16 - vwidth);

    let ypos = (board.player_pos.1.checked_sub(vheight / 2))
        .unwrap_or(0)
        .min(board.height as u16 - vheight);

    board.scroll_offset = Coordinate(xpos, ypos);
}

enum OldGameStateChange {
    Speed(u64),
}

fn handle_key_input(
    input_state: &mut InputState,
    _timestamp: u32,
    keycode: Option<Keycode>,
    _keymod: Mod,
    _repeat: bool,
    down: bool,
) -> Option<OldGameStateChange> {
    if down {
        input_state.pressed_keycode = keycode;
    } else {
        input_state.pressed_keycode = None;
    }

    let keycode = match keycode {
        Some(k) => k,
        None => return None,
    };
    match keycode {
        Keycode::Num1 => return Some(OldGameStateChange::Speed(1)),
        Keycode::Num2 => return Some(OldGameStateChange::Speed(2)),
        Keycode::Num3 => return Some(OldGameStateChange::Speed(3)),
        Keycode::Num4 => return Some(OldGameStateChange::Speed(4)),
        _ => (),
    }
    match keycode {
        Keycode::Up => input_state.up_pressed = down,
        Keycode::Down => input_state.down_pressed = down,
        Keycode::Left => input_state.left_pressed = down,
        Keycode::Right => input_state.right_pressed = down,
        Keycode::Space => input_state.space_pressed = down,
        Keycode::Delete => input_state.delete_pressed = down,
        _ => (),
    }
    None
}

enum InputResult {
    ExitBoard(CardinalDirection),
    Collide(Coordinate<u16>),
    Transport(u8, u8, u8),
    KeyLabel(u8),
}

fn convert_input(input_state: &InputState) -> Option<KeyPress> {
    Some(if input_state.up_pressed {
        KeyPress::Up
    } else if input_state.down_pressed {
        KeyPress::Down
    } else if input_state.left_pressed {
        KeyPress::Left
    } else if input_state.right_pressed {
        KeyPress::Right
    } else if input_state.space_pressed {
        KeyPress::Space
    } else if input_state.delete_pressed {
        KeyPress::Delete
    } else {
        return None;
    })
}

fn keycode_to_key(keycode: Keycode) -> Option<u8> {
    Some(match keycode {
        Keycode::A => b'a',
        Keycode::B => b'b',
        Keycode::C => b'c',
        Keycode::D => b'd',
        Keycode::E => b'e',
        Keycode::F => b'f',
        Keycode::G => b'g',
        Keycode::H => b'h',
        Keycode::I => b'i',
        Keycode::J => b'j',
        Keycode::K => b'k',
        Keycode::L => b'l',
        Keycode::M => b'm',
        Keycode::N => b'n',
        Keycode::O => b'o',
        Keycode::P => b'p',
        Keycode::Q => b'q',
        Keycode::R => b'r',
        Keycode::S => b's',
        Keycode::T => b't',
        Keycode::U => b'u',
        Keycode::V => b'v',
        Keycode::W => b'w',
        Keycode::X => b'x',
        Keycode::Y => b'y',
        Keycode::Z => b'z',
        Keycode::Num0 => b'0',
        Keycode::Num1 => b'1',
        Keycode::Num2 => b'2',
        Keycode::Num3 => b'3',
        Keycode::Num4 => b'4',
        Keycode::Num5 => b'5',
        Keycode::Num6 => b'6',
        Keycode::Num7 => b'7',
        Keycode::Num8 => b'8',
        Keycode::Num9 => b'9',
        _ => return None,
    })
}

fn process_input(
    board: &mut Board,
    input_state: &InputState,
    world_state: &mut WorldState,
) -> Option<InputResult> {
    world_state.key_pressed = input_state.pressed_keycode.map_or(0, |k| k as i32);

    if let Some(key) = input_state.pressed_keycode.and_then(|k| keycode_to_key(k)) {
        return Some(InputResult::KeyLabel(key));
    }

    let player_pos = board.player_pos;
    let xdiff  = if !world_state.player_locked_ew && input_state.left_pressed {
        world_state.player_face_dir = 3;
        if player_pos.0 > 0 {
            -1i8
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::West));
        }
    } else if !world_state.player_locked_ew && input_state.right_pressed {
        world_state.player_face_dir = 2;
        if (player_pos.0 as usize) < board.width - 1 {
            1i8
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::East));
        }
    } else {
        0i8
    };

    let ydiff  = if !world_state.player_locked_ns && xdiff == 0 && input_state.up_pressed {
        world_state.player_face_dir = 0;
        if (player_pos.1 as usize) > 0 {
            -1
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::North));
        }
    } else if !world_state.player_locked_ns && xdiff == 0 && input_state.down_pressed {
        world_state.player_face_dir = 1;
        if (player_pos.1 as usize) < board.height - 1 {
            1
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::South));
        }
    } else {
        0
    };

    let new_player_pos = Coordinate(
        (player_pos.0 as i16 + xdiff as i16) as u16,
        (player_pos.1 as i16 + ydiff as i16) as u16
    );
    if new_player_pos != player_pos {
        let thing = board.thing_at(&new_player_pos);
        if thing.is_solid() {
            return Some(InputResult::Collide(new_player_pos));
        }
        board.move_level(&player_pos, xdiff, ydiff);
        board.player_pos = new_player_pos;

        let under_thing = board.under_thing_at(&board.player_pos);
        if under_thing == Thing::Cave || under_thing == Thing::Stairs {
            let &(under_id, under_color, under_param) = board.under_at(&board.player_pos);
            return Some(InputResult::Transport(under_id, under_color, under_param));
        }
    }

    None
}

pub(crate) enum GameStateChange {
    Teleport(ByteString, Coordinate<u16>),
    Restore(usize, Coordinate<u16>),
    MessageBox(Vec<MessageBoxLine>, ByteString, Option<RobotId>),
}

pub(crate) fn tick_game_loop(
    world: &mut World,
    audio: &AudioEngine,
    world_path: &Path,
    input_state: &InputState,
    counters: &mut Counters,
    board_id: &mut usize,
) -> Option<StateChange> {
    let orig_player_pos = world.boards[*board_id].player_pos;

    let key = convert_input(input_state);
    let result = process_input(&mut world.boards[*board_id], &input_state, &mut world.state);
    match result {
        Some(InputResult::ExitBoard(dir)) => {
            let id = {
                let board = &world.boards[*board_id];
                match dir {
                    CardinalDirection::North => board.exits.0,
                    CardinalDirection::South => board.exits.1,
                    CardinalDirection::East => board.exits.2,
                    CardinalDirection::West => board.exits.3,
                }
            };
            if let Some(id) = id {
                let old_player_pos = world.boards[*board_id].player_pos;
                *board_id = id.0 as usize;
                let board = &mut world.boards[*board_id];
                let player_pos = match dir {
                    CardinalDirection::North =>
                        Coordinate(old_player_pos.0, board.height as u16 - 1),
                    CardinalDirection::South =>
                        Coordinate(old_player_pos.0, 0),
                    CardinalDirection::East =>
                        Coordinate(0, old_player_pos.1),
                    CardinalDirection::West =>
                        Coordinate(board.width as u16 - 1, old_player_pos.1),
                };
                enter_board(&mut world.state, audio, board, player_pos, &mut world.all_robots);
            } else {
                warn!("Edge of board with no exit.");
            }
        }

        Some(InputResult::Transport(id, color, dest_board_id)) => {
            let dest_board = &mut world.boards[dest_board_id as usize];
            let coord = dest_board.find(id, color).unwrap_or(dest_board.player_pos);
            *board_id = dest_board_id as usize;
            enter_board(&mut world.state, audio, dest_board, coord, &mut world.all_robots);
        }

        Some(InputResult::Collide(pos)) => {
            let board = &mut world.boards[*board_id];
            let (_id, color, param) = *board.level_at(&pos);
            let thing = board.thing_at(&pos);
            match thing {
                Thing::Robot | Thing::RobotPushable => {
                    let robot_id = RobotId::from(param);
                    let mut robots = Robots::new(board, &mut world.all_robots);
                    let robot = robots.get_mut(robot_id);
                    send_robot_to_label(robot, BuiltInLabel::Touch);
                }

                Thing::Gate => {
                    let mut unlocked = param == 0;
                    if !unlocked {
                        if world.state.take_key(color & 0x0F).is_ok() {
                            board.level_at_mut(&pos).2 = 0;
                            board.set_message_line("You unlock and open the gate.".into());
                            unlocked = true;
                        } else {
                            board.set_message_line("The gate is locked!".into());
                        }
                    }
                    if unlocked {
                        let (ref mut id, _, ref mut param) = board.level_at_mut(&pos);
                        *id = Thing::OpenGate.to_u8().unwrap();
                        *param = 22;
                    }
                }

                Thing::Door => {
                    const DOOR_FIRST_MOVEMENT: &[(i8, i8)] = &[
                        NORTH, WEST, NORTH, EAST, SOUTH, WEST, SOUTH, EAST,
                    ];
                    let (orientation, dir, status) = door_from_param(param);
                    let mut unlocked = status == DoorStatus::Unlocked;
                    if !unlocked {
                        if world.state.take_key(color & 0x0F).is_ok() {
                            board.level_at_mut(&pos).2 = param_from_door(orientation, dir, DoorStatus::Unlocked);
                            board.set_message_line("You unlock and open the door.".into());
                            unlocked = true;

                        } else {
                            board.set_message_line("The door is locked!".into());
                        }
                    }

                    if unlocked {
                        {
                            let (ref mut id, _, ref mut param) = board.level_at_mut(&pos);
                            *id = Thing::OpenDoor.to_u8().unwrap();
                            *param = *param & 7;
                        }
                        let movement = DOOR_FIRST_MOVEMENT[(param & 7) as usize];
                        // FIXME: support pushing
                        // FIXME: check for blocked, act appropriately.
                        board.move_level(&pos, movement.0, movement.1);
                    }
                }

                _ => warn!("ignoring collision with {:?} at {:?}", thing, pos)

            }
        }

        Some(InputResult::KeyLabel(k)) => {
            let mut name = b"key".to_vec();
            name.push(k);
            let label = ByteString::from(name);
            let board = &world.boards[*board_id];
            let mut robots = Robots::new(board, &mut world.all_robots);
            robots.foreach(|robot, _id| {
                send_robot_to_label(robot, EvaluatedByteString::no_eval_needed(label.clone()));
            });
        }

        None => (),
    }

    if world.boards[*board_id].player_pos != orig_player_pos &&
        !world.state.scroll_locked
    {
        reset_view(&mut world.boards[*board_id]);
    }

    let change = update_board(
        &mut world.state,
        audio,
        key,
        world_path,
        counters,
        &mut world.boards[*board_id],
        *board_id,
        &mut world.all_robots,
    );

    if let Some(change) = change {
        let new_board = match change {
            GameStateChange::Teleport(board, coord) => {
                let id = world.boards.iter().position(|b| b.title == board);
                if let Some(id) = id {
                    Some((id, coord))
                } else {
                    warn!("Couldn't find board {:?}", board);
                    None
                }
            }
            GameStateChange::Restore(id, coord) => {
                Some((id, coord))
            }

            GameStateChange::MessageBox(lines, title, rid) => {
                return Some(StateChange::Push(Box::new(MessageBoxState::new(title, lines, rid))));
            }
        };
        if let Some((id, coord)) = new_board {
            *board_id = id;
            enter_board(&mut world.state, audio, &mut world.boards[id], coord, &mut world.all_robots);
        }
    }

    None
}

struct MessageBoxState {
    lines: Vec<MessageBoxLine>,
    title: ByteString,
    pos: usize,
    rid: Option<RobotId>,
}

impl MessageBoxState {
    pub fn new(title: ByteString, lines: Vec<MessageBoxLine>, rid: Option<RobotId>) -> MessageBoxState {
        MessageBoxState {
            lines: lines,
            title: title,
            pos: 0,
            rid,
        }
    }
}

impl GameState for MessageBoxState {
    fn init(&mut self, _world: &mut World, _board_id: &mut usize) {
    }

    fn popped(&mut self, _world: &mut World, _board_id: usize, _data: PoppedData) {
    }

    fn input(
        &mut self,
        event: Event,
        _input_state: &mut InputState,
    ) -> Option<StateChange> {
        match event {
            Event::KeyDown {keycode: Some(Keycode::Escape), ..} =>
                return Some(StateChange::PopCurrent(None)),

            Event::KeyDown {keycode: Some(Keycode::Up), ..} => {
                if self.pos > 0 {
                    self.pos -= 1;
                }
            }

            Event::KeyDown {keycode: Some(Keycode::Down), ..} => {
                if self.pos < self.lines.len() - 1 {
                    self.pos += 1;
                }
            }

            Event::KeyDown {keycode: Some(Keycode::Return), ..} => {
                if let Some(rid) = self.rid {
                    if let MessageBoxLine::Option { ref label, .. } = self.lines[self.pos] {
                        return Some(StateChange::PopCurrent(Some(
                            PoppedData::MessageBox(rid, label.clone())
                        )));
                    }
                }
                return Some(StateChange::PopCurrent(None));
            }

            _ => (),
        }

        None
    }

    fn tick(
        &mut self,
        _world: &mut World,
        _world_path: &Path,
        _input_state: &InputState,
        _counters: &mut Counters,
        _board_id: &mut usize,
    ) -> Option<StateChange> {
        None
    }

    fn render(
        &mut self,
        world: &World,
        _board_id: usize,
        canvas: &mut Canvas<Window>,
    ) {
        let mut renderer = SdlRenderer { canvas };
        draw_messagebox(&world.state, &self.title, &self.lines, self.pos, &mut renderer);
    }
}
