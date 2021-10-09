use crate::{GameState, PoppedData, StateChange, SdlRenderer};
use crate::audio::MusicCallback;
use libmzx::audio::AudioEngine;
use libmzx::board::{enter_board, run_board_update};
use libmzx::robot::{Robots, RobotId, BuiltInLabel, EvaluatedByteString, send_robot_to_label};
use libmzx::{
    World, Board, Thing, CardinalDirection, Coordinate, Counters, ByteString, KeyPress, WorldState,
    render, draw_messagebox, MessageBoxLine, DoorStatus, door_from_param, param_from_door,
    bullet_param, BulletType, adjust_coordinate, MessageBoxLineType, Robot, adjust_coordinate_diff,
};
use libmzx::board::{NORTH, SOUTH, EAST, WEST, ExternalStateChange, LabelAction, move_level, put_at, move_level_to, reset_view};
use num_traits::ToPrimitive;
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::render::Canvas;
use sdl2::video::Window;
use std::path::Path;

fn render_game(
    world: &World,
    board_id: usize,
    canvas: &mut Canvas<Window>,
    is_title_screen: bool,
) {
    let mut renderer = SdlRenderer { canvas };
    let (ref board, ref robots) = world.boards[board_id];
    render(
        &world.state,
        (
            board.upper_left_viewport,
            board.viewport_size,
        ),
        board.scroll_offset,
        &board,
        robots,
        &mut renderer,
        is_title_screen,
    );
}

pub(crate) struct TitleState(pub MusicCallback);
impl GameState for TitleState {
    fn init(&mut self, world: &mut World, board_id: &mut usize) {
        let (ref mut board, ref mut robots) = world.boards[*board_id];
        let player_pos = board.player_pos;
        enter_board(
            &mut world.state,
            &self.0,
            board,
            player_pos,
            robots,
            &mut world.global_robot,
        );
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
                Some(StateChange::Replace(Box::new(PlayState::new(self.0.clone(), None)))),
            _ => None,
        }
    }

    fn tick(
        &mut self,
        world: &mut World,
        world_path: &Path,
        input_state: &InputState,
        counters: &mut Counters,
        boards: &[ByteString],
        board_id: &mut usize,
    ) -> Option<StateChange> {
        tick_game_loop(
            world, &self.0, world_path, input_state, counters, boards, board_id, &mut false,
        )
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

pub struct PlayState {
    music: MusicCallback,
    accept_player_input: bool,
    starting_board: Option<usize>,
}
impl PlayState {
    pub fn new(music: MusicCallback, starting_board: Option<usize>) -> PlayState {
        PlayState {
            music,
            starting_board,
            accept_player_input: true,
        }
    }
}

impl GameState for PlayState {
    fn init(&mut self, world: &mut World, board_id: &mut usize) {
        *board_id = self.starting_board.unwrap_or(world.starting_board_number.0 as usize);
        let (ref mut board, ref mut robots) = world.boards[*board_id];
        let pos = board.player_pos;
        enter_board(&mut world.state, &self.music, board, pos, robots, &mut world.global_robot);
        world.state.charset = world.state.initial_charset;
        world.state.palette = world.state.initial_palette.clone();
    }

    fn popped(&mut self, world: &mut World, board_id: usize, data: PoppedData) {
        match data {
            PoppedData::MessageBox(rid, label) => {
                let mut robots = Robots::new(&mut world.boards[board_id].1, &mut world.global_robot);
                let robot = robots.get_mut(rid);
                send_robot_to_label(robot, EvaluatedByteString::no_eval_needed(label));

            }
            PoppedData::Scroll(pos) => {
                let (ref mut board, ref mut robots) = world.boards[board_id];
                board.remove_thing_at(&pos).unwrap();
                let player = board.player_pos;
                move_level_to(board, robots, &player, &pos, &mut *world.state.update_done).unwrap();
                board.player_pos = pos;
                reset_view(board);
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
        boards: &[ByteString],
        board_id: &mut usize,
    ) -> Option<StateChange> {
        tick_game_loop(
            world, &self.music, world_path, input_state, counters, boards, board_id, &mut self.accept_player_input
        )
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

impl InputState {
    pub fn new_from(state: &InputState) -> InputState {
        InputState {
            right_pressed: state.right_pressed,
            left_pressed: state.left_pressed,
            up_pressed: state.up_pressed,
            down_pressed: state.down_pressed,
            delete_pressed: state.delete_pressed,
            space_pressed: state.space_pressed,
            pressed_keycode: None,
        }
    }
}

enum OldGameStateChange {
    Speed(u64),
}

pub(crate) fn update_key_states(input_state: &mut InputState, keycode: Option<Keycode>, down: bool) {
    if down {
        input_state.pressed_keycode = keycode;
    } else {
        input_state.pressed_keycode = None;
    }

    //println!("{:?} {}", keycode, if down { "down" } else { "up" });

    match keycode {
        Some(Keycode::Up) => input_state.up_pressed = down,
        Some(Keycode::Down) => input_state.down_pressed = down,
        Some(Keycode::Left) => input_state.left_pressed = down,
        Some(Keycode::Right) => input_state.right_pressed = down,
        Some(Keycode::Space) => input_state.space_pressed = down,
        Some(Keycode::Delete) => input_state.delete_pressed = down,
        _ => (),
    }
}

fn handle_key_input(
    _input_state: &mut InputState,
    _timestamp: u32,
    keycode: Option<Keycode>,
    _keymod: Mod,
    _repeat: bool,
    _down: bool,
) -> Option<OldGameStateChange> {
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
    None
}

enum InputResult {
    ExitBoard(CardinalDirection),
    Collide(Coordinate<u16>),
    Transport(u8, u8, u8),
    KeyLabel(u8),
    Shoot(CardinalDirection),
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
    robots: &mut [Robot],
    input_state: &InputState,
    world_state: &mut WorldState,
    allow_move_player: &mut bool,
) -> Option<InputResult> {
    world_state.key_pressed = input_state.pressed_keycode.map_or(0, |k| k as i32);

    if let Some(key) = input_state.pressed_keycode.and_then(|k| keycode_to_key(k)) {
        return Some(InputResult::KeyLabel(key));
    }

    if !*allow_move_player {
        *allow_move_player = true;
        return None;
    }

    if !board.player_locked_attack && input_state.space_pressed {
        *allow_move_player = false;
        if input_state.up_pressed {
            return Some(InputResult::Shoot(CardinalDirection::North));
        }
        if input_state.down_pressed {
            return Some(InputResult::Shoot(CardinalDirection::South));
        }
        if input_state.right_pressed {
            return Some(InputResult::Shoot(CardinalDirection::East));
        }
        if input_state.left_pressed {
            return Some(InputResult::Shoot(CardinalDirection::West));
        }
    }

    let player_pos = board.player_pos;
    let xdiff  = if !board.player_locked_ew && input_state.left_pressed {
        world_state.player_face_dir = 3;
        if player_pos.0 > 0 {
            -1i8
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::West));
        }
    } else if !board.player_locked_ew && input_state.right_pressed {
        world_state.player_face_dir = 2;
        if (player_pos.0 as usize) < board.width - 1 {
            1i8
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::East));
        }
    } else {
        0i8
    };

    let ydiff  = if !board.player_locked_ns && xdiff == 0 && input_state.up_pressed {
        world_state.player_face_dir = 0;
        if (player_pos.1 as usize) > 0 {
            -1
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::North));
        }
    } else if !board.player_locked_ns && xdiff == 0 && input_state.down_pressed {
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
        let thing = board.thing_at(&new_player_pos).unwrap();
        if thing.is_pushable() {
            if let Some(pushed_pos) = adjust_coordinate_diff(new_player_pos, board, xdiff as i16, ydiff as i16) {
                move_level_to(board, robots, &new_player_pos, &pushed_pos, &mut *world_state.update_done).unwrap();
            } else {
                return Some(InputResult::Collide(new_player_pos));
            }
        } else if thing.is_solid() {
            return Some(InputResult::Collide(new_player_pos));
        }
        // FIXME: figure out what kind of delay makes sense for accepting player movement.
        //*allow_move_player = false;
        move_level(board, robots, &player_pos, xdiff, ydiff, &mut *world_state.update_done).unwrap();
        board.player_pos = new_player_pos;

        // FIXME: move this to the start of the game update loop so that a frame is
        //        rendered with the player on top of the transport.
        let under_thing = board.under_thing_at(&board.player_pos).unwrap();
        if under_thing.is_teleporter() {
            let &(under_id, under_color, under_param) = board.under_at(&board.player_pos).unwrap();
            return Some(InputResult::Transport(under_id, under_color, under_param));
        }
    }

    None
}

pub(crate) fn tick_game_loop(
    world: &mut World,
    audio: &dyn AudioEngine,
    world_path: &Path,
    input_state: &InputState,
    counters: &mut Counters,
    boards: &[ByteString],
    board_id: &mut usize,
    accept_player_input: &mut bool,
) -> Option<StateChange> {
    let num_boards = world.boards.len();
    let (ref mut board, ref mut robots) = world.boards[*board_id];
    let orig_player_pos = board.player_pos;

    let key = convert_input(input_state);
    let result = process_input(
        board,
        robots,
        &input_state,
        &mut world.state,
        accept_player_input,
    );
    match result {
        Some(InputResult::ExitBoard(dir)) => {
            let id = {
                match dir {
                    CardinalDirection::North => board.exits.0,
                    CardinalDirection::South => board.exits.1,
                    CardinalDirection::East => board.exits.2,
                    CardinalDirection::West => board.exits.3,
                }
            };
            match id {
                Some(id) if id.0 < num_boards as u8 => {
                    let old_player_pos = board.player_pos;
                    *board_id = id.0 as usize;
                    let (ref mut board, ref mut robots) = world.boards[*board_id];
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
                    enter_board(&mut world.state, audio, board, player_pos, robots, &mut world.global_robot);
                }
                _ => {
                    warn!("Edge of board with no exit.");
                }
            }
        }

        Some(InputResult::Transport(id, color, dest_board_id)) => {
            let (ref mut dest_board, ref mut robots) = &mut world.boards[dest_board_id as usize];
            let coord = dest_board.find(id, color).unwrap_or(dest_board.player_pos);
            *board_id = dest_board_id as usize;
            enter_board(&mut world.state, audio, dest_board, coord, robots, &mut world.global_robot);
        }

        Some(InputResult::Collide(pos)) => {
            let (ref mut board, ref mut robots) = &mut world.boards[*board_id];
            let (_id, color, param) = *board.level_at(&pos).unwrap();
            let thing = board.thing_at(&pos).unwrap();
            match thing {
                Thing::Robot | Thing::RobotPushable => {
                    let robot_id = RobotId::from(param);
                    let mut robots = Robots::new(robots, &mut world.global_robot);
                    let robot = robots.get_mut(robot_id);
                    send_robot_to_label(robot, BuiltInLabel::Touch);
                }

                Thing::Scroll | Thing::Sign => {
                    let scroll = &board.scrolls[param as usize - 1];
                    let text = if !scroll.text.is_empty() {
                        &scroll.text[1..]
                    } else {
                        &scroll.text
                    };
                    let lines = text
                        .split(|&c| c == b'\n')
                        .map(|s| MessageBoxLine::Text(s.to_owned().into(), MessageBoxLineType::Plain))
                        .collect();
                    let source = if thing == Thing::Scroll {
                        MessageBoxSource::Scroll(pos)
                    } else {
                        MessageBoxSource::Sign
                    };
                    return Some(StateChange::Push(Box::new(
                        MessageBoxState::new("Scroll".into(), lines, source)
                    )));
                }

                Thing::Gate => {
                    let mut unlocked = param == 0;
                    if !unlocked {
                        if world.state.take_key(color & 0x0F).is_ok() {
                            board.level_at_mut(&pos).unwrap().2 = 0;
                            board.set_message_line("You unlock and open the gate.".into());
                            unlocked = true;
                        } else {
                            board.set_message_line("The gate is locked!".into());
                        }
                    }
                    if unlocked {
                        let (ref mut id, _, ref mut param) = board.level_at_mut(&pos).unwrap();
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
                            board.level_at_mut(&pos).unwrap().2 = param_from_door(orientation, dir, DoorStatus::Unlocked);
                            board.set_message_line("You unlock and open the door.".into());
                            unlocked = true;

                        } else {
                            board.set_message_line("The door is locked!".into());
                        }
                    }

                    if unlocked {
                        {
                            let (ref mut id, _, ref mut param) = board.level_at_mut(&pos).unwrap();
                            *id = Thing::OpenDoor.to_u8().unwrap();
                            *param = *param & 7;
                        }
                        let movement = DOOR_FIRST_MOVEMENT[(param & 7) as usize];
                        // FIXME: support pushing
                        // FIXME: check for blocked, act appropriately.
                        move_level(board, robots, &pos, movement.0, movement.1, &mut *world.state.update_done).unwrap();
                    }
                }

                _ => warn!("ignoring collision with {:?} at {:?}", thing, pos)

            }
        }

        Some(InputResult::KeyLabel(k)) => {
            let mut name = b"key".to_vec();
            name.push(k);
            let label = ByteString::from(name);
            let mut robots = Robots::new(&mut world.boards[*board_id].1, &mut world.global_robot);
            robots.foreach(|robot, _id| {
                send_robot_to_label(robot, EvaluatedByteString::no_eval_needed(label.clone()));
            });
        }

        Some(InputResult::Shoot(dir)) => {
            // TODO: check world.state.ammo
            let board = &mut world.boards[*board_id].0;
            let adjusted = adjust_coordinate(board.player_pos, board, dir);
            if let Some(ref bullet_pos) = adjusted {
                // FIXME: shoot blocking object at initial position instead of overwriting
                if !board.thing_at(bullet_pos).unwrap().is_solid() {
                    put_at(
                        board,
                        bullet_pos,
                        0x07,
                        Thing::Bullet,
                        bullet_param(BulletType::Player, dir),
                        &mut *world.state.update_done,
                    ).unwrap();
                }
            }
        }

        None => (),
    }

    let (ref mut board, _) = world.boards[*board_id];
    if board.player_pos != orig_player_pos &&
        !world.state.scroll_locked
    {
        reset_view(board);
    }

    let change = run_board_update(
        world,
        audio,
        world_path,
        counters,
        boards,
        board_id,
        key,
    );

    match change {
        Some(ExternalStateChange::MessageBox(lines, title, rid)) => {
            Some(StateChange::Push(Box::new(
                MessageBoxState::new(title, lines, MessageBoxSource::Robot(rid))
            )))
        }
        None => None,
    }
}

enum MessageBoxSource {
    Robot(Option<RobotId>),
    Scroll(Coordinate<u16>),
    Sign,
}

struct MessageBoxState {
    lines: Vec<MessageBoxLine>,
    title: ByteString,
    pos: usize,
    source: MessageBoxSource
}

impl MessageBoxState {
    pub fn new(title: ByteString, lines: Vec<MessageBoxLine>, source: MessageBoxSource) -> MessageBoxState {
        MessageBoxState {
            lines: lines,
            title: title,
            pos: 0,
            source,
        }
    }

    fn pop_state(&self) -> Option<PoppedData> {
        match self.source {
            MessageBoxSource::Scroll(pos) => Some(PoppedData::Scroll(pos)),
            _ => None,
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
                return Some(StateChange::PopCurrent(self.pop_state())),

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
                if let MessageBoxSource::Robot(Some(rid)) = self.source {
                    if let MessageBoxLine::Option { ref label, .. } = self.lines[self.pos] {
                        return Some(StateChange::PopCurrent(Some(
                            PoppedData::MessageBox(rid, label.clone())
                        )));
                    }
                }
                return Some(StateChange::PopCurrent(self.pop_state()));
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
        _boards: &[ByteString],
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
