#![feature(nll)]

extern crate env_logger;
extern crate libmzx;
#[macro_use] extern crate log;
extern crate num_traits;
extern crate rand;
extern crate sdl2;
extern crate time;

use libmzx::{
    Renderer, render, load_world, CardinalDirection, Coordinate, Board, Robot, Command, Thing,
    WorldState, Counters, Resolve, Operator, ExtendedColorValue, ExtendedParam, RelativeDirBasis,
    ColorValue, ParamValue, CharId, ByteString, Explosion, ExplosionResult, RelativePart,
    SignedNumeric, Color as MzxColor, RunStatus, Size, dir_to_cardinal_dir, dir_to_cardinal_dir_rel,
    adjust_coordinate, KeyPress, CounterContext, CounterContextMut,
};
use num_traits::{FromPrimitive, ToPrimitive};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::pixels::Color;
use sdl2::render::Canvas;
use sdl2::video::Window;
use std::env;
use std::fs::File;
use std::io::Read;
use std::ops::Deref;
use std::path::Path;
use std::process::exit;
use std::time::Duration;

struct SdlRenderer<'a> {
    canvas: &'a mut Canvas<Window>,
}

impl<'a> Renderer for SdlRenderer<'a> {
    fn put_pixel(
        &mut self,
        x: usize,
        y: usize,
        r: u8,
        g: u8,
        b: u8,
    ) {
        self.canvas.set_draw_color(Color::RGB(r, g, b));
        self.canvas.draw_point((x as i32, y as i32)).unwrap();
    }

    fn clear(&mut self) {
        self.canvas.set_draw_color(Color::RGB(0, 0, 0));
        self.canvas.clear();
    }
}

fn reset_view(board: &mut Board) {
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

#[derive(Default)]
struct InputState {
    left_pressed: bool,
    right_pressed: bool,
    up_pressed: bool,
    down_pressed: bool,
    space_pressed: bool,
    delete_pressed: bool,
    pressed_keycode: Option<Keycode>,
}

enum GameStateChange {
    BeginGame,
    Speed(u64),
}

fn handle_key_input(
    input_state: &mut InputState,
    _timestamp: u32,
    keycode: Option<Keycode>,
    _keymod: Mod,
    _repeat: bool,
    down: bool,
    is_title_screen: bool,
) -> Option<GameStateChange> {
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
        Keycode::Num1 => return Some(GameStateChange::Speed(1)),
        Keycode::Num2 => return Some(GameStateChange::Speed(2)),
        Keycode::Num3 => return Some(GameStateChange::Speed(3)),
        Keycode::Num4 => return Some(GameStateChange::Speed(4)),
        _ => (),
    }
    if is_title_screen {
        if down && keycode == Keycode::P {
            return Some(GameStateChange::BeginGame);
        }
        return None;
    } else {
        match keycode {
            Keycode::Up => input_state.up_pressed = down,
            Keycode::Down => input_state.down_pressed = down,
            Keycode::Left => input_state.left_pressed = down,
            Keycode::Right => input_state.right_pressed = down,
            Keycode::Space => input_state.space_pressed = down,
            Keycode::Delete => input_state.delete_pressed = down,
            _ => (),
        }
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

enum CoordinatePart {
    X,
    Y,
}

trait CoordinateExtractor {
    type CoordinateType;
    fn extract(&self, part: CoordinatePart) -> Self::CoordinateType;
}

impl<T: Copy> CoordinateExtractor for Coordinate<T> {
    type CoordinateType = T;
    fn extract(&self, part: CoordinatePart) -> T {
        match part {
            CoordinatePart::X => self.0,
            CoordinatePart::Y => self.1,
        }
    }
}

enum Relative {
    None,
    Coordinate(Option<RelativePart>, Coordinate<u16>),
}

impl Relative {
    fn resolve_xy<'a>(
        &self,
        x_value: &SignedNumeric,
        y_value: &SignedNumeric,
        counters: &Counters,
        context: CounterContext<'a>,
        part: RelativePart,
    ) -> Coordinate<u16> {
        let x = self.resolve(x_value, counters, context, part, CoordinatePart::X);
        let y = self.resolve(y_value, counters, context, part, CoordinatePart::Y);
        Coordinate(x.max(0) as u16, y.max(0) as u16)
    }

    fn resolve<'a>(
        &self,
        value: &SignedNumeric,
        counters: &Counters,
        context: CounterContext<'a>,
        part: RelativePart,
        coord_part: CoordinatePart,
    ) -> i16 {
        let v = value.resolve(counters, context) as i16;
        match *self {
            Relative::None => v,
            Relative::Coordinate(ref value_part, ref coord) => {
                if value_part.map_or(true, |p| p == part) {
                    v + coord.extract(coord_part) as i16
                } else {
                    v
                }
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
enum RobotId {
    Board(usize),
    Global,
}

impl RobotId {
    fn from(param: u8) -> RobotId {
        if param == 0 {
            RobotId::Global
        } else {
            RobotId::Board(param as usize - 1)
        }
    }

    fn is_global(&self) -> bool {
        *self == RobotId::Global
    }
}

struct Robots<'a> {
    robots_start: usize,
    num_robots: usize,
    robots: &'a mut [Robot],
}

impl<'a> Robots<'a> {
    fn new(board: &Board, robots: &'a mut [Robot]) -> Robots<'a> {
        Robots {
            robots_start: board.robot_range.0,
            num_robots: board.robot_range.1,
            robots,
        }
    }

    fn foreach<F: FnMut(&mut Robot, RobotId)>(&mut self, mut f: F) {
        f(&mut self.robots[0], RobotId::Global);
        let end = self.robots_start + self.num_robots;
        for (idx, robot) in self.robots[self.robots_start..end].iter_mut().enumerate() {
            f(robot, RobotId::Board(idx));
        }
    }

    fn get(&self, id: RobotId) -> &Robot {
        match id {
            RobotId::Board(id) => &self.robots[self.robots_start + id],
            RobotId::Global => &self.robots[0],
        }
    }

    fn get_mut(&mut self, id: RobotId) -> &mut Robot {
        match id {
            RobotId::Board(id) => &mut self.robots[self.robots_start + id],
            RobotId::Global => &mut self.robots[0],
        }
    }

    fn find(&self, name: &ByteString) -> Option<RobotId> {
        if &self.robots[0].name == name {
            return Some(RobotId::Global);
        }
        let robots = &self.robots[self.robots_start..self.robots_start + self.num_robots];
        robots.iter().position(|r| &r.name == name).map(RobotId::Board)
    }
}

fn update_robot(
    state: &mut WorldState,
    key: Option<KeyPress>,
    world_path: &Path,
    counters: &mut Counters,
    board: &mut Board,
    board_id: usize,
    mut robots: Robots,
    robot_id: RobotId,
) -> Option<StateChange> {
    let robot = robots.get_mut(robot_id);
    if !robot.alive || robot.status == RunStatus::FinishedRunning {
        return None;
    }

    robot.cycle_count += 1;
    if robot.cycle_count < robot.cycle {
        debug!("delaying {:?} (cycle {}/{})", robot.name, robot.cycle_count, robot.cycle);
        return None;
    }
    robot.cycle_count = 0;

    debug!("executing {:?}", robot.name);

    if let Some(dir) = robot.walk {
        move_robot(robot, board, dir);
    }

    let mut lines_run = 0;
    let mut mode = Relative::None;
    let mut state_change = None;

    const CYCLES: u8 = 40;
    loop {
        let robot = robots.get_mut(robot_id);
        if !robot_id.is_global() {
            if !board.thing_at(&robot.position).is_robot() {
                info!("current robot not present at reported position; terminating");
                robot.alive = false;
            } else if RobotId::from(board.level_at(&robot.position).2) != robot_id {
                info!("current robot does not match robot ID at reported position; terminating");
                robot.alive = false;
            }
        }

        if lines_run >= CYCLES || !robot.alive || robot.current_line as usize >= robot.program.len() {
            break;
        }
        let mut advance = true;
        let cmd = robot.program[robot.current_line as usize].clone();
        debug!("evaluating {:?} ({})", cmd, robot.current_line);
        let mut no_end_cycle = false;
        let mut reset_mode = true;

        lines_run += 1;

        match cmd {
            Command::End => advance = false,

            Command::Die(as_item) => {
                let robot = robots.get_mut(robot_id);
                board.remove_thing_at(&robot.position);
                robot.alive = false;
                if as_item {
                    board.player_pos = robot.position;
                }
            }

            Command::Wait(ref n) => {
                let robot = robots.get(robot_id);
                let new_loc = if robot.current_loc > 0 {
                    robot.current_loc - 1
                } else {
                    let context = CounterContext::from(board, robot, state);
                    n.resolve(counters, context) as u8
                };
                let robot = robots.get_mut(robot_id);
                robot.current_loc = new_loc;
                advance = robot.current_loc == 0;
                no_end_cycle = advance;
            }

            Command::PlayerChar(ref c) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state,
                );
                let c = c.resolve(counters, context);
                state.set_char_id(CharId::PlayerNorth, c);
                state.set_char_id(CharId::PlayerSouth, c);
                state.set_char_id(CharId::PlayerEast, c);
                state.set_char_id(CharId::PlayerWest, c);
            }

            Command::PlayerCharDir(ref d, ref c) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let c = c.resolve(counters, context);
                match dir_to_cardinal_dir(robots.get(robot_id), d) {
                    Some(CardinalDirection::North) => state.set_char_id(CharId::PlayerNorth, c),
                    Some(CardinalDirection::South) => state.set_char_id(CharId::PlayerSouth, c),
                    Some(CardinalDirection::East) => state.set_char_id(CharId::PlayerEast, c),
                    Some(CardinalDirection::West) => state.set_char_id(CharId::PlayerWest, c),
                    None => (),
                }
            }

            Command::PlayerColor(ref c) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let c = c.resolve(counters, context);
                state.set_char_id(CharId::PlayerColor, c.0);
            }

            Command::CharEdit(ref c, ref bytes) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let c = c.resolve(counters, context);
                let bytes: Vec<u8> = bytes.iter().map(|b| b.resolve(counters, context)).collect();
                state.charset.nth_mut(c).copy_from_slice(&bytes);
            }

            Command::ScrollChar(ref c, ref dir) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let c = c.resolve(counters, context);
                let (x, y) = match dir_to_cardinal_dir(robots.get(robot_id), dir) {
                    Some(CardinalDirection::North) => (0, -1),
                    Some(CardinalDirection::South) => (0, 1),
                    Some(CardinalDirection::East) => (1, 0),
                    Some(CardinalDirection::West) => (-1, 0),
                    None => (0, 0),
                };
                let data = state.charset.nth_mut(c);
                if x < 0 {
                    for row in data.iter_mut() {
                        let tmp = (*row & 0b1000_0000) >> 7;
                        *row <<= 1;
                        *row = (*row & 0b1111_1110) | tmp;
                    }
                } else if x > 0 {
                    for row in data.iter_mut() {
                        let tmp = (*row & 0b0000_0001) << 7;
                        *row >>= 1;
                        *row = (*row & 0b0111_1111) | tmp;
                    }
                }

                if y < 0 {
                    let tmp = data[0].clone();
                    for i in 1..=data.len() {
                        data[i - 1] = if i < data.len() { data[i] } else { tmp };
                    }
                } else if y > 0 {
                    let tmp = data[data.len() - 1].clone();
                    for i in (1..data.len()).rev() {
                        data[i] = data[i - 1];
                    }
                    data[0] = tmp;
                }
            }

            Command::CopyChar(ref c1, ref c2) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let c1 = c1.resolve(counters, context);
                let c2 = c2.resolve(counters, context);
                let bytes: Vec<u8> = state.charset.nth(c1).to_owned();
                state.charset.nth_mut(c2).copy_from_slice(&bytes);
            }

            Command::LoadCharSet(ref c) => {
                let path = world_path.join(c.to_string());
                match File::open(&path) {
                    Ok(mut file) => {
                        let mut v = vec![];
                        file.read_to_end(&mut v).unwrap();
                        state.charset.data.copy_from_slice(&v);
                    }
                    Err(e) => {
                        info!("Error opening charset {} ({})", path.display(), e);
                    }
                }
            }

            Command::SetColor(ref c, ref r, ref g, ref b) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let c = c.resolve(counters, context);
                let r = r.resolve(counters, context) as u8;
                let g = g.resolve(counters, context) as u8;
                let b = b.resolve(counters, context) as u8;
                if r > 63 || g > 63 || b > 63 {
                    warn!("bad colors: {},{},{}", r, g, b);
                }
                state.palette.colors[c as usize].0 = MzxColor { r, g, b };
            }

            Command::ColorIntensity(ref c, ref n) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let n = n.resolve(counters, context);
                let intensity = (n as f32 / 100.).min(1.0);
                match *c {
                    Some(ref c) => {
                        let c = c.resolve(counters, context);
                        if (c as usize) < state.palette.colors.len() {
                            state.palette.colors[c as usize].1 = intensity;
                        }
                    }
                    None => {
                        for &mut (_, ref mut i) in state.palette.colors.iter_mut() {
                            *i = intensity;
                        }
                    }
                }
            }

            Command::LoadPalette(ref p) => {
                let path = world_path.join(p.to_string());
                match File::open(&path) {
                    Ok(mut file) => {
                        let mut v = vec![];
                        file.read_to_end(&mut v).unwrap();
                        for (new, (ref mut old, _)) in v.chunks(3).zip(state.palette.colors.iter_mut()) {
                            old.r = new[0];
                            old.g = new[1];
                            old.b = new[2];
                        }
                    }
                    Err(e) => {
                        info!("Error opening palette {} ({})", path.display(), e);
                    }
                }
            }

            Command::ViewportSize(ref w, ref h) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let w = w.resolve(counters, context) as u8;
                let h = h.resolve(counters, context) as u8;
                board.viewport_size = Size(w, h);
            }

            Command::Viewport(ref x, ref y) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let x = x.resolve(counters, context) as u8;
                let y = y.resolve(counters, context) as u8;
                board.upper_left_viewport = Coordinate(x, y);
            }

            Command::ResetView => {
                reset_view(board);
            }

            Command::LockScroll => {
                state.scroll_locked = true;
            }

            Command::UnlockScroll => {
                state.scroll_locked = false;
            }

            Command::ScrollViewXY(ref x, ref y) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let x = x.resolve(counters, context);
                let y = y.resolve(counters, context);
                board.scroll_offset = Coordinate(x as u16, y as u16);
            }

            Command::ScrollView(ref dir, ref n) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let n = n.resolve(counters, context);
                let dir = dir_to_cardinal_dir(robots.get(robot_id), dir);
                match dir {
                    Some(CardinalDirection::West) => if (board.scroll_offset.0 as u32) < n {
                        board.scroll_offset.0 = 0;
                    } else {
                        board.scroll_offset.0 -= n as u16;
                    },
                    Some(CardinalDirection::East) => if (board.scroll_offset.0 as u32 + n) as usize > board.width - board.viewport_size.0 as usize {
                        board.scroll_offset.0 = board.width as u16 - board.viewport_size.0 as u16;
                    } else {
                        board.scroll_offset.0 += n as u16;
                    },
                    Some(CardinalDirection::North) => if (board.scroll_offset.1 as u32) < n {
                        board.scroll_offset.1 = 0;
                    } else {
                        board.scroll_offset.1 -= n as u16;
                    },
                    Some(CardinalDirection::South) => if (board.scroll_offset.1 as u32 + n) as usize > board.height - board.viewport_size.1 as usize {
                        board.scroll_offset.1 = board.height as u16 - board.viewport_size.1 as u16;
                    } else {
                        board.scroll_offset.1 += n as u16;
                    }
                    None => (),
                };
            }

            Command::LockPlayer => {
                state.player_locked_ns = true;
                state.player_locked_ew = true;
            }

            Command::UnlockPlayer => {
                state.player_locked_ns = false;
                state.player_locked_ew = false;
            }

            Command::LockPlayerNS => {
                state.player_locked_ns = true;
            }

            Command::LockPlayerEW => {
                state.player_locked_ew = true;
            }

            Command::Set(ref s, ref n, ref n2) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContextMut::from(
                    board, robot, state
                );
                let mut val = n.resolve(counters, context.as_immutable()) as i32;
                if let Some(ref n2) = *n2 {
                    let upper = n2.resolve(counters, context.as_immutable());
                    let range = (upper - val).abs() as u32;
                    val = (rand::random::<u32>() % range) as i32 + val;
                }
                counters.set(s.clone(), context, val);
            }

            Command::Dec(ref s, ref n, ref n2) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let initial = counters.get(s, context);
                let mut val = n.resolve(counters, context) as i32;
                if let Some(ref n2) = *n2 {
                    let upper = n2.resolve(counters, context);
                    let range = (upper - val).abs() as u32;
                    val = (rand::random::<u32>() % range) as i32 + val;
                }
                let context = CounterContextMut::from(
                    board, robots.get_mut(robot_id), state
                );
                counters.set(s.clone(), context, initial.wrapping_sub(val));
            }

            Command::Inc(ref s, ref n, ref n2) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let initial = counters.get(s, context);
                let mut val = n.resolve(counters, context);
                if let Some(ref n2) = *n2 {
                    let upper = n2.resolve(counters, context);
                    let range = (upper - val).abs() as u32;
                    val = (rand::random::<u32>() % range) as i32 + val;
                }
                let context = CounterContextMut::from(
                    board, robots.get_mut(robot_id), state
                );
                counters.set(s.clone(), context, initial.wrapping_add(val));
            }

            Command::Multiply(ref s, ref n) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let initial = counters.get(s, context);
                let val = n.resolve(counters, context);
                let context = CounterContextMut::from(
                    board, robots.get_mut(robot_id), state
                );
                counters.set(s.clone(), context, initial.wrapping_mul(val));
            }

            Command::Divide(ref s, ref n) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let initial = counters.get(s, context);
                let val = n.resolve(counters, context);
                let context = CounterContextMut::from(
                    board, robots.get_mut(robot_id), state
                );
                counters.set(s.clone(), context, initial / val);
            }

            Command::Modulo(ref s, ref n) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let initial = counters.get(s, context);
                let val = n.resolve(counters, context);
                let context = CounterContextMut::from(
                    board, robots.get_mut(robot_id), state
                );
                counters.set(s.clone(), context, initial % val);
            }

            Command::If(ref s, op, ref n, ref l) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                let val = counters.get(s, context);
                let cmp = n.resolve(counters, context);
                let l = l.eval(counters, context);
                let result = match op {
                    Operator::Equals => val == cmp,
                    Operator::NotEquals => val != cmp,
                    Operator::LessThan => val < cmp,
                    Operator::GreaterThan => val > cmp,
                    Operator::LessThanEquals => val <= cmp,
                    Operator::GreaterThanEquals => val >= cmp,
                };
                if result {
                    advance = !jump_robot_to_label(robot, l);
                }
            }

            Command::IfCondition(ref condition, ref l, invert) => {
                let robot = robots.get_mut(robot_id);
                let mut result = robot.is(condition, board, key);
                debug!("condition {:?}: {}", condition, result);
                if invert {
                    result = !result;
                }
                if result {
                    let context = CounterContext::from(board, robot, state);
                    let l = l.eval(counters, context);
                    advance = !jump_robot_to_label(robot, l);
                }
            }

            Command::IfThingXY(ref color, ref thing, ref param, ref x, ref y, ref l) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                let color = color.resolve(counters, context);
                let param = param.resolve(counters, context);
                let pos = mode.resolve_xy(x, y, counters, context, RelativePart::First);
                let l = l.eval(counters, context);
                let &(board_thing, board_color, board_param) = board.level_at(&pos);
                if board_thing == thing.to_u8().unwrap() &&
                    color.matches(ColorValue(board_color)) &&
                    param.matches(ParamValue(board_param))
                {
                    advance = !jump_robot_to_label(robot, l);
                }
            }

            Command::IfPlayerXY(ref x, ref y, ref l) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                let pos = mode.resolve_xy(x, y, counters, context, RelativePart::First);
                let l = l.eval(counters, context);
                if board.player_pos == pos {
                    advance = !jump_robot_to_label(robot, l);
                }
            }

            Command::Change(ref c1, ref t1, ref p1, ref c2, ref t2, ref p2) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let c1 = c1.resolve(counters, context);
                let c2 = c2.resolve(counters, context);
                let p1 = p1.resolve(counters, context);
                let p2 = p2.resolve(counters, context);
                for &mut (ref mut id, ref mut color, ref mut param) in &mut board.level {
                    if c1.matches(ColorValue(*color)) &&
                        p1.matches(ParamValue(*param)) &&
                        *t1 == Thing::from_u8(*id).unwrap()
                    {
                        match c2 {
                            ExtendedColorValue::Known(c) =>
                                *color = c.0,
                            ExtendedColorValue::Unknown(Some(bg), None) =>
                                *color = (*color & 0x0F) | (bg.0 << 4),
                            ExtendedColorValue::Unknown(None, Some(fg)) =>
                                *color = (*color & 0xF0) | fg.0,
                            ExtendedColorValue::Unknown(None, None) => (),
                            ExtendedColorValue::Unknown(Some(_), Some(_)) =>
                                unreachable!(),
                        }
                        *id = t2.to_u8().unwrap();
                        match p2 {
                            ExtendedParam::Any => (),
                            ExtendedParam::Specific(p) => *param = p.0,
                        }
                    }
                }
            }

            Command::ChangeOverlay(ref c1, ref c2, ref chars) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let c1 = c1.resolve(counters, context);
                let c2 = c2.resolve(counters, context);
                let chars = chars.as_ref().map(|(ch1, ch2)| (
                    ch1.resolve(counters, context),
                    ch2.resolve(counters, context),
                ));
                if let Some((_, ref mut overlay)) = board.overlay {
                    for &mut (ref mut ch, ref mut color) in overlay.iter_mut() {
                        if c1.matches(ColorValue(*color)) &&
                            chars.as_ref().map_or(true, |(ch1, _)| ch1 == ch)
                        {
                            match c2 {
                                ExtendedColorValue::Known(c) =>
                                    *color = c.0,
                                ExtendedColorValue::Unknown(Some(bg), None) =>
                                    *color = (*color & 0x0F) | (bg.0 << 4),
                                ExtendedColorValue::Unknown(None, Some(fg)) =>
                                    *color = (*color & 0xF0) | fg.0,
                                ExtendedColorValue::Unknown(None, None) => (),
                                ExtendedColorValue::Unknown(Some(_), Some(_)) =>
                                    unreachable!(),
                            }
                            if let Some((_, ch2)) = chars {
                                *ch = ch2;
                            }
                        }
                    }
                }
            }

            Command::PutOverlay(ref c, ref ch, ref x, ref y) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let c = c.resolve(counters, context);
                let ch = ch.resolve(counters, context);
                let pos = mode.resolve_xy(x, y, counters, context, RelativePart::First);
                if let Some((_, ref mut overlay)) = board.overlay {
                    let overlay = &mut overlay[pos.1 as usize * board.width + pos.0 as usize];
                    let color = match c {
                        ExtendedColorValue::Known(c) =>
                            c.0,
                        ExtendedColorValue::Unknown(Some(bg), None) =>
                            (overlay.0 & 0x0F) | (bg.0 << 4),
                        ExtendedColorValue::Unknown(None, Some(fg)) =>
                            (overlay.0 & 0xF0) | fg.0,
                        ExtendedColorValue::Unknown(None, None) => 0x07,
                        ExtendedColorValue::Unknown(Some(_), Some(_)) =>
                            unreachable!(),
                    };

                    *overlay = (ch, color);
                }
            }

            Command::Color(ref c) => {
                let robot = robots.get(robot_id);
                let context = CounterContext::from(board, robot, state);
                board.level_at_mut(&robot.position).1 = c.resolve(counters, context).0;
            }

            Command::Char(ref c) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                robot.ch = c.resolve(counters, context);
            }

            Command::Goto(ref l) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                let l = l.eval(counters, context);
                advance = !jump_robot_to_label(robot, l);
            }

            Command::Zap(ref l, ref n) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                let n = n.resolve(counters, context);
                for _ in 0..n {
                    let label = robot
                        .program
                        .iter_mut()
                        .find(|c| **c == Command::Label(l.clone()));
                    if let Some(cmd) = label {
                        *cmd = Command::ZappedLabel(l.clone());
                    }
                }
            }

            Command::Restore(ref l, ref n) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                let n = n.resolve(counters, context);
                for _ in 0..n {
                    let label = robot
                        .program
                        .iter_mut()
                        .rev()
                        .find(|c| **c == Command::ZappedLabel(l.clone()));
                    if let Some(cmd) = label {
                        *cmd = Command::Label(l.clone());
                    } else {
                        break;
                    }
                }
            }

            Command::Send(ref r, ref l) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let r = r.evaluate(counters, context);
                let l = l.eval(counters, context);
                robots.foreach(|robot, id| {
                    if r.as_ref() == b"all" || robot.name == r {
                        let did_send = send_robot_to_label(robot, l.clone());
                        if id == robot_id {
                            advance = !did_send;
                        }
                    }
                });
            }

            Command::SendDir(ref d, ref l, player) => {
                let robot = robots.get(robot_id);
                let basis = if player {
                    RelativeDirBasis::from_player(board)
                } else {
                    RelativeDirBasis::from_robot(robot)
                };
                if let Some(dir) = dir_to_cardinal_dir_rel(basis, d) {
                    let source_pos = if player {
                        board.player_pos
                    } else {
                        robot.position
                    };
                    let adjusted = adjust_coordinate(source_pos, board, dir);
                    if let Some(coord) = adjusted {
                        let thing = board.thing_at(&coord);
                        if thing == Thing::Robot || thing == Thing::RobotPushable {
                            let dest_robot_id = RobotId::from(board.level_at(&coord).2);
                            let context = CounterContext::from(board, robot, state);
                            let l = l.eval(counters, context);
                            send_robot_to_label(robots.get_mut(dest_robot_id), l);
                        }
                    }
                }
            }

            Command::SendXY(ref x, ref y, ref l) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let pos = mode.resolve_xy(x, y, counters, context, RelativePart::First);
                let l = l.eval(counters, context);
                if board.thing_at(&pos).is_robot() {
                    let dest_robot_id = RobotId::from(board.level_at(&pos).2);
                    send_robot_to_label(robots.get_mut(dest_robot_id), l);
                }
            }

            Command::Walk(ref d) => {
                let robot = robots.get_mut(robot_id);
                robot.walk = dir_to_cardinal_dir(robot, d);
            }

            Command::Slash(ref s) => {
                let robot = robots.get_mut(robot_id);
                if robot.current_loc as usize == s.as_bytes().len() {
                    robot.current_loc = 0;
                    no_end_cycle = true;
                } else {
                    let dir = match s.as_bytes()[robot.current_loc as usize] {
                        b'n' => Some(CardinalDirection::North),
                        b's' => Some(CardinalDirection::South),
                        b'e' => Some(CardinalDirection::East),
                        b'w' => Some(CardinalDirection::West),
                        _ => None,
                    };
                    if let Some(dir) = dir {
                        move_robot(robot, board, dir);
                    }
                    robot.current_loc += 1;
                    advance = false;
                }
            }

            Command::Go(ref d, ref n) => {
                let robot = robots.get_mut(robot_id);
                if robot.current_loc > 0 {
                    robot.current_loc -= 1;
                } else {
                    robot.current_loc = {
                        let context = CounterContext::from(board, robot, state);
                        n.resolve(counters, context) as u8
                    };
                }
                if robot.current_loc != 0 {
                    let dir = dir_to_cardinal_dir(&robot, d);
                    if let Some(dir) = dir {
                        move_robot(robot, board, dir);
                    }
                    advance = false;
                } else {
                    advance = true;
                    no_end_cycle = true;
                }
            }

            Command::TryDir(ref d, ref l) => {
                let robot = robots.get_mut(robot_id);
                let dir = dir_to_cardinal_dir(robot, d);
                if let Some(dir) = dir {
                    if move_robot(robot, board, dir) == Move::Blocked {
                        let context = CounterContext::from(board, robot, state);
                        let l = l.eval(counters, context);
                        jump_robot_to_label(robot, l);
                    }
                }
            }

            Command::Cycle(ref n) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                robot.cycle = (n.resolve(counters, context) % 256) as u8;
            }

            Command::Explode(ref n) => {
                let robot = robots.get_mut(robot_id);
                robot.alive = false;
                let context = CounterContext::from(board, robot, state);
                let n = n.resolve(counters, context) as u8;
                let &mut (ref mut id, ref mut c, ref mut param) =
                    board.level_at_mut(&robot.position);
                *id = Thing::Explosion.to_u8().unwrap();
                *c = state.char_id(CharId::ExplosionStage1);
                *param = Explosion { stage: 0, size: n }.to_param();
            }

            Command::OverlayMode(mode) => if let Some(ref mut overlay) = board.overlay {
                overlay.0 = mode;
            },

            Command::GotoXY(ref x, ref y) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                let coord = mode.resolve_xy(x, y, counters, context, RelativePart::First);
                move_robot_to(robot, board, coord);
            }

            Command::RelSelf(ref part) => {
                mode = Relative::Coordinate(*part, robots.get(robot_id).position);
                reset_mode = false;
                lines_run -= 1;
            }

            Command::RelPlayer(ref part) => {
                mode = Relative::Coordinate(*part, board.player_pos);
                reset_mode = false;
                lines_run -= 1;
            }

            Command::RelCounters(ref part) => {
                //FIXME: casting is probably the wrong solution here
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let coord = Coordinate(
                    counters.get(&BuiltInCounter::Xpos.into(), context) as u16,
                    counters.get(&BuiltInCounter::Ypos.into(), context) as u16,
                );
                mode = Relative::Coordinate(*part, coord);
                reset_mode = false;
                lines_run -= 1;
            }

            Command::Teleport(ref b, ref x, ref y) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let coord = Coordinate(
                    x.resolve(counters, context) as u16,
                    y.resolve(counters, context) as u16,
                );
                state_change = Some(StateChange::Teleport(b.clone(), coord));
            }

            Command::SavePlayerPosition(ref n) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let n = n.resolve(counters, context) as usize;
                state.saved_positions[n] = (board_id, board.player_pos);
            }

            Command::RestorePlayerPosition(ref n) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let n = n.resolve(counters, context) as usize;
                let (board_id, pos) = state.saved_positions[n];
                state_change = Some(StateChange::Restore(board_id, pos));
            }

            Command::LoopStart => {
                robots.get_mut(robot_id).loop_count = 0;
            }

            Command::LoopFor(ref n) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                let n = n.resolve(counters, context);
                if (robot.loop_count as u32) < n {
                    let start = robot
                        .program[0..robot.current_line as usize]
                        .iter()
                        .rev()
                        .position(|c| *c == Command::LoopStart);
                    if let Some(idx) = start {
                        robot.current_line -= idx as u16 + 1;
                    }
                    robot.loop_count += 1;
                }
            }

            Command::Label(_) |
            Command::ZappedLabel(_) |
            Command::Comment(_) |
            Command::BlankLine => lines_run -= 1,

            Command::CopyBlock(ref src_x, ref src_y, ref w, ref h, ref dst_x, ref dst_y) |
            Command::CopyOverlayBlock(ref src_x, ref src_y, ref w, ref h, ref dst_x, ref dst_y) => {
                let overlay = if let Command::CopyOverlayBlock(..) = cmd { true } else { false };
                let (src, w, h, dest) = {
                    let context = CounterContext::from(
                        board, robots.get(robot_id), state
                    );
                    let mut src = mode.resolve_xy(
                        src_x,
                        src_y,
                        counters,
                        context,
                        RelativePart::First
                    );
                    if src.0 as usize > board.width {
                        src.0 = (board.width - 1) as u16;
                    }
                    if src.1 as usize > board.height {
                        src.1 = (board.height - 1) as u16;
                    }

                    let mut w = w.resolve(counters, context)
                        .max(1)
                        .min(board.width as u32 - src.0 as u32) as u16;
                    let mut h = h.resolve(counters, context).max(1)
                        .max(1)
                        .min(board.height as u32 - src.1 as u32) as u16;

                    let dest = mode.resolve_xy(
                        dst_x,
                        dst_y,
                        counters,
                        context,
                        RelativePart::Last
                    );
                    if dest.0 + w > board.width as u16 {
                        w = (board.width as i16 - dest.0 as i16).max(0) as u16;
                    }
                    if dest.1 + h > board.height as u16 {
                        h = (board.height as i16 - dest.1 as i16).max(0) as u16;
                    }
                    (src, w, h, dest)
                };

                debug!("copying from {:?} for {}x{} to {:?}", src, w, h, dest);
                if overlay {
                    board.copy_overlay(src, Size(w, h), dest);
                } else {
                    board.copy(src, Size(w, h), dest);
                }
            }

            Command::WriteOverlay(ref c, ref s, ref x, ref y) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let pos = mode.resolve_xy(
                    x,
                    y,
                    counters,
                    context,
                    RelativePart::First,
                );
                let c = c.resolve(counters, context);
                let s = s.evaluate(counters, context);
                board.write_overlay(&pos, &s, c.0);
            }

            Command::PutXY(ref color, ref thing, ref param, ref x, ref y) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let color = color.resolve(counters, context);
                let param = param.resolve(counters, context);
                let pos = mode.resolve_xy(
                    x,
                    y,
                    counters,
                    context,
                    RelativePart::First,
                );
                put_thing(board, color, *thing, param, pos);
            }

            Command::PutDir(ref color, ref thing, ref param, ref dir) => {
                let robot = robots.get(robot_id);
                let context = CounterContext::from(board, robot, state);
                let color = color.resolve(counters, context);
                let param = param.resolve(counters, context);
                let dir = dir_to_cardinal_dir(robot, dir);
                if let Some(dir) = dir {
                    let adjusted = adjust_coordinate(robot.position, board, dir);
                    if let Some(coord) = adjusted {
                        put_thing(board, color, *thing, param, coord);
                    }
                }
            }

            Command::CopyRobotXY(ref x, ref y) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let pos = mode.resolve_xy(
                    x,
                    y,
                    counters,
                    context,
                    RelativePart::First,
                );

                let &(thing, _color, param) = board.level_at(&pos);
                if Thing::from_u8(thing).unwrap().is_robot() {
                    copy_robot(RobotId::from(param), robot_id, &mut robots, board);
                }
            }

            Command::CopyRobotNamed(ref name) => {
                let source_id = robots.find(name);
                if let Some(source_id) = source_id {
                    copy_robot(source_id, robot_id, &mut robots, board);
                }
            }

            Command::LockSelf(is_locked) => {
                robots.get_mut(robot_id).locked = is_locked;
            }

            Command::MesgEdge(enabled) => {
                state.message_edge = enabled;
            }

            Command::CenterMessage => {
                board.message_col = None;
            }

            Command::MessageColumn(ref n) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                board.message_col = Some(n.resolve(counters, context) as u8);
            }

            Command::MessageRow(ref n) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                board.message_row = n.resolve(counters, context) as u8;
            }

            Command::MessageLine(ref s) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                board.message_line = s.evaluate(counters, context);
                board.remaining_message_cycles = 80;
            }

            Command::ClearMessage => {
                board.remaining_message_cycles = 0;
            }

            Command::PutPlayerXY(ref x, ref y) => {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let pos = mode.resolve_xy(
                    x,
                    y,
                    counters,
                    context,
                    RelativePart::First,
                );
                let old_player_pos = board.player_pos;
                board.move_level_to(&old_player_pos, &pos);
                board.player_pos = pos;
            }

            Command::MovePlayerDir(ref dir, ref blocked) => {
                let robot = robots.get_mut(robot_id);
                let context = CounterContext::from(board, robot, state);
                let blocked = blocked.as_ref().map(|b| b.eval(counters, context));
                let new_pos = dir_to_cardinal_dir(robot, dir)
                    .and_then(|dir| adjust_coordinate(board.player_pos, board, dir));
                if let Some(new_pos) = new_pos {
                    let player_pos = board.player_pos;
                    board.move_level_to(&player_pos, &new_pos);
                } else if let Some(blocked) = blocked {
                    advance = !jump_robot_to_label(robot, blocked);
                }
            }

            ref cmd => warn!("ignoring {:?}", cmd),
        }

        if reset_mode {
            mode = Relative::None;
        }

        if advance {
            robots.get_mut(robot_id).current_line += 1;
        }

        if !no_end_cycle && cmd.is_cycle_ending() {
            break;
        }
    }

    robots.get_mut(robot_id).status = RunStatus::FinishedRunning;

    state_change
}

fn put_thing(
    board: &mut Board,
    color: ExtendedColorValue,
    thing: Thing,
    param: ExtendedParam,
    pos: Coordinate<u16>
) {
    let color = match color {
        ExtendedColorValue::Known(c) =>
            c.0,
        // TODO: have a table of default foreground colors for things,
        //       get the current background color at destination.
        ExtendedColorValue::Unknown(Some(_), None) |
        ExtendedColorValue::Unknown(None, Some(_)) |
        ExtendedColorValue::Unknown(None, None) |
        ExtendedColorValue::Unknown(Some(_), Some(_)) =>
            0x07, //HACK
    };

    // TODO: have a table of default parameters for things.
    let param = match param {
        ExtendedParam::Specific(p) => p.0,
        ExtendedParam::Any => 0x00, //HACK
    };

    board.put_at(&pos, thing.to_u8().unwrap(), color, param);
}

fn copy_robot(source_id: RobotId, robot_id: RobotId, robots: &mut Robots, board: &mut Board) {
    let dest_position = robots.get(robot_id).position;
    let source_robot = robots.get_mut(source_id);
    let color = board.level_at(&source_robot.position).1;
    board.level_at_mut(&dest_position).1 = color;
    *robots.get_mut(robot_id) = Robot::copy_from(
        source_robot,
        dest_position,
    );
}

enum BuiltInCounter {
    Xpos,
    Ypos,
}

impl Into<ByteString> for BuiltInCounter {
    fn into(self) -> ByteString {
        ByteString::from(match self {
            BuiltInCounter::Xpos => "xpos",
            BuiltInCounter::Ypos => "ypos",
        })
    }
}

impl Into<EvaluatedByteString> for BuiltInCounter {
    fn into(self) -> EvaluatedByteString {
        EvaluatedByteString(self.into())
    }
}

enum BuiltInLabel {
    Thud,
    Edge,
    Bombed,
    JustEntered,
    Touch,
}

impl Into<EvaluatedByteString> for BuiltInLabel {
    fn into(self) -> EvaluatedByteString {
        EvaluatedByteString(ByteString::from(match self {
            BuiltInLabel::Thud => "thud",
            BuiltInLabel::Edge => "edge",
            BuiltInLabel::Bombed => "bombed",
            BuiltInLabel::JustEntered => "justentered",
            BuiltInLabel::Touch => "touch",
        }))
    }
}

#[derive(Clone, Debug)]
struct EvaluatedByteString(ByteString);

impl Deref for EvaluatedByteString {
    type Target = ByteString;
    fn deref(&self) -> &ByteString {
        &self.0
    }
}

trait Evaluator {
    fn eval<'a>(&self, counters: &Counters, context: CounterContext<'a>) -> EvaluatedByteString;
}

impl Evaluator for ByteString {
    fn eval<'a>(&self, counters: &Counters, context: CounterContext<'a>) -> EvaluatedByteString {
        EvaluatedByteString(self.evaluate(counters, context))
    }
}

fn send_robot_to_label<S: Into<EvaluatedByteString>>(robot: &mut Robot, label: S) -> bool {
    if robot.locked {
        return false;
    }
    jump_robot_to_label(robot, label)
}

fn jump_robot_to_label<S: Into<EvaluatedByteString>>(robot: &mut Robot, label: S) -> bool {
    let label = label.into();
    let label_pos = robot
        .program
        .iter()
        .position(|c| c == &Command::Label(label.deref().clone()));
    if let Some(pos) = label_pos {
        debug!("jumping {:?} to {:?}", robot.name, label);
        robot.current_loc = 0;
        robot.current_line = pos as u16 + 1;
        true
    } else {
        false
    }
}

enum MoveResult {
    Move(Coordinate<u16>),
    Edge,
}

fn move_robot_to(robot: &mut Robot, board: &mut Board, pos: Coordinate<u16>) {
    let thing = board.thing_at(&pos);
    if thing == Thing::Player {
        return;
    }
    // TODO: check if thing can move to under layer
    board.move_level_to(&robot.position, &pos);
    robot.position = pos;
}

#[derive(PartialEq)]
enum Move {
    Moved,
    Blocked,
}

fn move_robot(robot: &mut Robot, board: &mut Board, dir: CardinalDirection) -> Move {
    let result = match dir {
        CardinalDirection::North => {
            if robot.position.1 == 0 {
                MoveResult::Edge
            } else {
                MoveResult::Move(Coordinate(robot.position.0, robot.position.1 - 1))
            }
        }
        CardinalDirection::South => {
            if robot.position.1 as usize == board.height - 1 {
                MoveResult::Edge
            } else {
                MoveResult::Move(Coordinate(robot.position.0, robot.position.1 + 1))
            }
        }
        CardinalDirection::East => {
            if robot.position.0 as usize == board.width - 1 {
                MoveResult::Edge
            } else {
                MoveResult::Move(Coordinate(robot.position.0 + 1, robot.position.1))
            }
        }
        CardinalDirection::West => {
            if robot.position.0 == 0 {
                MoveResult::Edge
            } else {
                MoveResult::Move(Coordinate(robot.position.0 - 1, robot.position.1))
            }
        }
    };
    match result {
        MoveResult::Edge => {
            if !jump_robot_to_label(robot, BuiltInLabel::Edge) {
                jump_robot_to_label(robot, BuiltInLabel::Thud);
            }
            Move::Blocked
        }
        MoveResult::Move(new_pos) => {
            let thing = board.thing_at(&new_pos);
            if thing.is_solid() {
                jump_robot_to_label(robot, BuiltInLabel::Thud);
                Move::Blocked
            } else {
                board.move_level_to(&robot.position, &new_pos);
                robot.position = new_pos;
                Move::Moved
            }
        }
    }
}

enum StateChange {
    Teleport(ByteString, Coordinate<u16>),
    Restore(usize, Coordinate<u16>),
}

fn update_board(
    state: &mut WorldState,
    key: Option<KeyPress>,
    world_path: &Path,
    counters: &mut Counters,
    board: &mut Board,
    board_id: usize,
    all_robots: &mut Vec<Robot>,
) -> Option<StateChange> {
    let mut robots = Robots::new(board, all_robots);
    robots.foreach(|robot, _| {
        robot.status = RunStatus::NotRun;
    });

    let change = update_robot(
        state,
        key,
        world_path,
        counters,
        board,
        board_id,
        robots,
        RobotId::Global,
    );
    if change.is_some() {
        return change;
    }

    for y in 0..board.height {
        for x in 0..board.width {
            let coord = Coordinate(x as u16, y as u16);
            match board.thing_at(&coord) {
                Thing::Robot | Thing::RobotPushable => {
                    let robots = Robots::new(board, all_robots);

                    let change = update_robot(
                        state,
                        key,
                        world_path,
                        counters,
                        board,
                        board_id,
                        robots,
                        RobotId::from(board.level_at(&coord).2)
                    );
                    if change.is_some() {
                        return change;
                    }
                }

                Thing::Explosion => {
                    let mut explosion = Explosion::from_param(board.level_at(&coord).2);
                    if explosion.stage == 0 {
                        if explosion.size > 0 {
                            explosion.size -= 1;
                            board.level_at_mut(&coord).2 = explosion.to_param();

                            let dirs = [
                                CardinalDirection::North,
                                CardinalDirection::South,
                                CardinalDirection::East,
                                CardinalDirection::West,
                            ];
                            for dir in &dirs {
                                let adjusted = adjust_coordinate(
                                    coord,
                                    board,
                                    *dir,
                                );
                                let coord = match adjusted {
                                    Some(coord) => coord,
                                    None => continue,
                                };
                                let thing = board.thing_at(&coord);
                                if !thing.is_solid() && thing != Thing::Explosion {
                                    board.put_at(
                                        &coord,
                                        Thing::Explosion.to_u8().unwrap(),
                                        0x00,
                                        explosion.to_param(),
                                    );
                                } else if thing.is_robot() {
                                    let robot_id = RobotId::from(board.level_at(&coord).2);

                                    let mut robots = Robots::new(board, all_robots);
                                    let robot = robots.get_mut(robot_id);
                                    send_robot_to_label(robot, BuiltInLabel::Bombed);
                                }
                                // TODO: hurt player.
                            }
                        }
                    }

                    if explosion.stage == 3 {
                        let (thing, color) = match board.explosion_result {
                            ExplosionResult::Nothing => (
                                Thing::Space.to_u8().unwrap(),
                                0x07,
                            ),
                            ExplosionResult::Ash => (
                                Thing::Floor.to_u8().unwrap(),
                                0x08,
                            ),
                            ExplosionResult::Fire => (
                                Thing::Fire.to_u8().unwrap(),
                                0x0C,
                            ),
                        };
                        board.put_at(&coord, thing, color, 0x00);
                    } else {
                        explosion.stage += 1;
                        board.level_at_mut(&coord).2 = explosion.to_param();
                    }
                }

                Thing::Fire => {
                    if rand::random::<u8>() >= 20 {
                        let cur_param = board.level_at(&coord).2;
                        if cur_param < 5 {
                            board.level_at_mut(&coord).2 += 1;
                        } else {
                            board.level_at_mut(&coord).2 = 0;
                        }
                    }

                    let rval = rand::random::<u8>();
                    if rval < 8 {
                        if rval == 1 && !board.fire_burns_forever {
                            board.put_at(
                                &coord,
                                Thing::Floor.to_u8().unwrap(),
                                0x08,
                                0x00,
                            );
                        }

                        let dirs = [
                            CardinalDirection::North,
                            CardinalDirection::South,
                            CardinalDirection::East,
                            CardinalDirection::West,
                        ];
                        for dir in &dirs {
                            let adjusted = adjust_coordinate(
                                coord,
                                board,
                                *dir,
                            );
                            let coord = match adjusted {
                                Some(coord) => coord,
                                None => continue,
                            };

                            let thing = board.thing_at(&coord);
                            let level = board.level_at(&coord);
                            let thing_id = level.0;

                            let spread =
                                (thing == Thing::Space && board.fire_burns_space) ||
                                (thing_id >= Thing::Fake.to_u8().unwrap() &&
                                 thing_id <= Thing::ThickWeb.to_u8().unwrap() &&
                                 board.fire_burns_fakes) ||
                                (thing == Thing::Tree && board.fire_burns_trees) ||
                                (level.1 == 0x06 &&
                                 board.fire_burns_brown &&
                                 thing_id < Thing::Sensor.to_u8().unwrap());

                            if spread {
                                board.put_at(
                                    &coord,
                                    Thing::Fire.to_u8().unwrap(),
                                    0x0C,
                                    0x00,
                                );
                            }
                        }
                    }
                }

                _ => (),
            }
        }
    }

    state.message_color += 1;
    if state.message_color > 0x0F {
        state.message_color = 0x01;
    }
    if board.remaining_message_cycles > 0 {
        board.remaining_message_cycles -= 1;
    }

    None
}

fn enter_board(board: &mut Board, player_pos: Coordinate<u16>, robots: &mut [Robot]) {
    let old_pos = board.player_pos;
    if old_pos != player_pos {
        board.move_level_to(&old_pos, &player_pos);
    }
    board.player_pos = player_pos;
    reset_view(board);

    Robots::new(board, robots).foreach(|robot, _id| {
        send_robot_to_label(robot, BuiltInLabel::JustEntered);
    })
}

fn run(world_path: &Path) {
    let world_data = match File::open(&world_path) {
        Ok(mut file) => {
            let mut v = vec![];
            file.read_to_end(&mut v).unwrap();
            v
        }
        Err(e) => {
            println!("Error opening {} ({})", world_path.display(), e);
            exit(1)
        }
    };

    let mut world = match load_world(&world_data) {
        Ok(world) => world,
        Err(e) => {
            println!("Error reading {} ({:?})", world_path.display(), e);
            exit(1)
        }
    };

    let world_path = Path::new(&world_path).parent().unwrap();

    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem.window("revenge of megazeux", 640, 350)
      .position_centered()
      .build()
      .unwrap();

    let mut canvas = window.into_canvas().software().build().unwrap();

    canvas.clear();
    canvas.present();

    canvas.set_draw_color(Color::RGBA(255, 255, 255, 255));

    let mut events = sdl_context.event_pump().unwrap();

    let mut board_id = 0;
    let mut is_title_screen = true;
    let mut game_speed: u64 = 4;

    let mut input_state = InputState::default();
    let mut counters = Counters::new();

    'mainloop: loop {
        let mut orig_player_pos = world.boards[board_id].player_pos;

        let start = time::precise_time_ns();
        for event in events.poll_iter() {
            let change = match event {
                Event::Quit{..} |
                Event::KeyDown {keycode: Option::Some(Keycode::Escape), ..} =>
                    break 'mainloop,
                Event::KeyDown {timestamp, keycode, keymod, repeat, ..} => {
                    handle_key_input(
                        &mut input_state,
                        timestamp,
                        keycode,
                        keymod,
                        repeat,
                        true,
                        is_title_screen,
                    )
                }
                Event::KeyUp {timestamp, keycode, keymod, repeat, ..} => {
                    handle_key_input(
                        &mut input_state,
                        timestamp,
                        keycode,
                        keymod,
                        repeat,
                        false,
                        is_title_screen,
                    )
                }
                _ => None,
            };
            match change {
                Some(GameStateChange::BeginGame) => {
                    is_title_screen = false;
                    board_id = world.starting_board_number.0 as usize;
                    let pos = world.boards[board_id].player_pos;
                    orig_player_pos = pos;
                    enter_board(&mut world.boards[board_id], pos, &mut world.all_robots);
                    world.state.charset = world.state.initial_charset;
                    world.state.palette = world.state.initial_palette.clone();
                }
                Some(GameStateChange::Speed(n)) => {
                    println!("changing speed to {}", n);
                    game_speed = n;
                }
                None => (),
            }
        }

        let key = convert_input(&input_state);
        let result = process_input(&mut world.boards[board_id], &input_state, &mut world.state);
        match result {
            Some(InputResult::ExitBoard(dir)) => {
                let id = {
                    let board = &world.boards[board_id];
                    match dir {
                        CardinalDirection::North => board.exits.0,
                        CardinalDirection::South => board.exits.1,
                        CardinalDirection::East => board.exits.2,
                        CardinalDirection::West => board.exits.3,
                    }
                };
                if let Some(id) = id {
                    let old_player_pos = world.boards[board_id].player_pos;
                    board_id = id.0 as usize;
                    let board = &mut world.boards[board_id];
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
                    enter_board(board, player_pos, &mut world.all_robots);
                } else {
                    warn!("Edge of board with no exit.");
                }
            }

            Some(InputResult::Transport(id, color, dest_board_id)) => {
                let dest_board = &mut world.boards[dest_board_id as usize];
                let coord = dest_board.find(id, color).unwrap_or(dest_board.player_pos);
                board_id = dest_board_id as usize;
                enter_board(dest_board, coord, &mut world.all_robots);
            }

            Some(InputResult::Collide(pos)) => {
                let board = &world.boards[board_id];
                let (_id, _color, param) = board.level_at(&pos);
                let thing = board.thing_at(&pos);
                match thing {
                    Thing::Robot | Thing::RobotPushable => {
                        let robot_id = RobotId::from(*param);
                        let mut robots = Robots::new(board, &mut world.all_robots);
                        let robot = robots.get_mut(robot_id);
                        send_robot_to_label(robot, BuiltInLabel::Touch);
                    }

                    _ => warn!("ignoring collision with {:?} at {:?}", thing, pos)

                }
            }

            Some(InputResult::KeyLabel(k)) => {
                let mut name = b"key".to_vec();
                name.push(k);
                let label = ByteString::from(name);
                let board = &world.boards[board_id];
                let mut robots = Robots::new(board, &mut world.all_robots);
                robots.foreach(|robot, _id| {
                    send_robot_to_label(robot, EvaluatedByteString(label.clone()));
                });
            }

            None => (),
        }

        if world.boards[board_id].player_pos != orig_player_pos &&
            !world.state.scroll_locked
        {
            reset_view(&mut world.boards[board_id]);
        }

        let change = update_board(
            &mut world.state,
            key,
            world_path,
            &mut counters,
            &mut world.boards[board_id],
            board_id,
            &mut world.all_robots,
        );

        if let Some(change) = change {
            let new_board = match change {
                StateChange::Teleport(board, coord) => {
                    let id = world.boards.iter().position(|b| b.title == board);
                    if let Some(id) = id {
                        Some((id, coord))
                    } else {
                        warn!("Couldn't find board {:?}", board);
                        None
                    }
                }
                StateChange::Restore(id, coord) => {
                    Some((id, coord))
                }
            };
            if let Some((id, coord)) = new_board {
                board_id = id;
                enter_board(&mut world.boards[id], coord, &mut world.all_robots);
            }
        }

        {
            let mut renderer = SdlRenderer {
                canvas: &mut canvas,
            };
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
        canvas.present();

        let now = time::precise_time_ns();
        let elapsed_ms = (now - start) / 1_000_000;
        let total_ticks = (16 * (game_speed - 1)).checked_sub(elapsed_ms);
        if let Some(diff) = total_ticks {
            ::std::thread::sleep(Duration::from_millis(diff));
        }
    }
}

fn main() {
    env_logger::init();
    let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run /path/to/world.mzx")
    } else {
        run(Path::new(&args[1]));
    }
}
