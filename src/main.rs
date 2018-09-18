extern crate env_logger;
extern crate libmzx;
#[macro_use] extern crate log;
extern crate num_traits;
extern crate rand;
extern crate sdl2;
extern crate time;

use libmzx::{
    Renderer, render, load_world, CardinalDirection, Coordinate, Board, Robot, Command, Thing,
    WorldState, Counters, Resolve, Direction, Operator, ExtendedColorValue, ExtendedParam,
    ColorValue, ParamValue, CharId, ByteString, Explosion, ExplosionResult
};
use num_traits::{FromPrimitive, ToPrimitive};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
//use sdl2::mouse::Cursor;
use sdl2::pixels::Color;
//use sdl2::rect::Rect;
use sdl2::render::Canvas;
//use sdl2::surface::Surface;
use sdl2::video::Window;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::exit;
use std::time::Duration;

//TODO: deal with sending a robot to a label while in the middle of a multi-cycle command

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
}

struct InputState {
    left_pressed: bool,
    right_pressed: bool,
    up_pressed: bool,
    down_pressed: bool,
    space_pressed: bool,
    _to_process: Option<Keycode>,
}

fn handle_key_input(
    input_state: &mut InputState,
    _timestamp: u32,
    keycode: Option<Keycode>,
    _keymod: Mod,
    _repeat: bool,
    down: bool
) {
    let keycode = match keycode {
        Some(k) => k,
        None => return,
    };
    match keycode {
        Keycode::Up => input_state.up_pressed = down,
        Keycode::Down => input_state.down_pressed = down,
        Keycode::Left => input_state.left_pressed = down,
        Keycode::Right => input_state.right_pressed = down,
        Keycode::Space => input_state.space_pressed = down,
        _ => (),
    }
}

enum InputResult {
    ExitBoard(CardinalDirection),
    Collide(Coordinate<u16>),
}

fn process_input(
    board: &mut Board,
    input_state: &InputState,
) -> Option<InputResult> {
    let player_pos = board.player_pos;
    let xdiff  = if input_state.left_pressed {
        if player_pos.0 > 0 {
            -1i8
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::West));
        }
    } else if input_state.right_pressed {
        if (player_pos.0 as usize) < board.width - 1 {
            1i8
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::East));
        }
    } else {
        0i8
    };

    let ydiff  = if xdiff == 0 && input_state.up_pressed {
        if (player_pos.1 as usize) > 0 {
            -1
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::North));
        }
    } else if xdiff == 0 && input_state.down_pressed {
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
    }

    None
}

fn update_robot(
    state: &mut WorldState,
    world_path: &Path,
    counters: &mut Counters,
    board: &mut Board,
    robots: &mut [Robot],
    robot_id: usize,
) {
    if !robots[robot_id].alive {
        return;
    }

    if robots[robot_id].cycle_count < robots[robot_id].cycle {
        robots[robot_id].cycle_count += 1;
        debug!("delaying {:?}", robots[robot_id].name);
        return;
    }
    robots[robot_id].cycle_count = 0;

    debug!("executing {:?}", robots[robot_id].name);

    let mut lines_run = 0;

    const CYCLES: u8 = 40;
    loop {
        if lines_run >= CYCLES ||
            !robots[robot_id].alive ||
            robots[robot_id].current_line as usize >= robots[robot_id].program.len()
        {
            break;
        }
        let mut advance = true;
        let cmd = robots[robot_id].program[robots[robot_id].current_line as usize].clone();
        debug!("evaluating {:?} ({})", cmd, robots[robot_id].current_line);
        let mut no_end_cycle = false;

        lines_run += 1;

        match cmd {
            Command::End => advance = false,

            Command::Die => {
                board.remove_thing_at(&robots[robot_id].position);
                robots[robot_id].alive = false;
            }

            Command::Wait(ref n) => {
                if robots[robot_id].current_loc > 0 {
                    robots[robot_id].current_loc -= 1;
                } else {
                    robots[robot_id].current_loc = n.resolve(counters) as u8;
                }
                advance = robots[robot_id].current_loc == 0;
                no_end_cycle = advance;
            }

            Command::PlayerChar(ref c) => {
                let c = c.resolve(counters);
                state.set_char_id(CharId::PlayerNorth, c);
                state.set_char_id(CharId::PlayerSouth, c);
                state.set_char_id(CharId::PlayerEast, c);
                state.set_char_id(CharId::PlayerWest, c);
            }

            Command::PlayerCharDir(ref d, ref c) => {
                let c = c.resolve(counters);
                match d.dir {
                    Direction::North => state.set_char_id(CharId::PlayerNorth, c),
                    Direction::South => state.set_char_id(CharId::PlayerSouth, c),
                    Direction::East => state.set_char_id(CharId::PlayerEast, c),
                    Direction::West => state.set_char_id(CharId::PlayerWest, c),
                    _ => (),
                }
            }

            Command::PlayerColor(ref c) => {
                let c = c.resolve(counters);
                state.set_char_id(CharId::PlayerColor, c.0);
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

            Command::LoadPalette(ref p) => {
                let path = world_path.join(p.to_string());
                match File::open(&path) {
                    Ok(mut file) => {
                        let mut v = vec![];
                        file.read_to_end(&mut v).unwrap();
                        for (new, old) in v.chunks(3).zip(state.palette.colors.iter_mut()) {
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

            Command::ResetView => {
                let vwidth = board.viewport_size.0 as u16;
                let vheight = board.viewport_size.1 as u16;

                let xpos = if board.player_pos.0 < vwidth / 2 {
                    0
                } else if board.player_pos.0 > board.width as u16 - vwidth / 2 {
                    board.width as u16 - vwidth
                } else {
                    board.player_pos.0 - vwidth / 2
                };

                let ypos = if board.player_pos.1 < vheight / 2 {
                    0
                } else if board.player_pos.1 > board.height as u16 - vheight / 2 {
                    board.height as u16 - vheight
                } else {
                    board.player_pos.1 - vheight / 2
                };

                board.scroll_offset = Coordinate(xpos, ypos);
            }

            Command::ScrollViewXY(ref x, ref y) => {
                let x = x.resolve(counters);
                let y = y.resolve(counters);
                board.scroll_offset = Coordinate(x as u16, y as u16);
            }

            Command::ScrollView(ref dir, ref n) => {
                let n = n.resolve(counters);
                match dir.dir {
                    Direction::West => if board.scroll_offset.0 < n {
                        board.scroll_offset.0 = 0;
                    } else {
                        board.scroll_offset.0 -= n;
                    },
                    Direction::East => if (board.scroll_offset.0 + n) as usize > board.width - board.viewport_size.0 as usize {
                        board.scroll_offset.0 = board.width as u16 - board.viewport_size.0 as u16;
                    } else {
                        board.scroll_offset.0 += n;
                    },
                    Direction::North => if board.scroll_offset.1 < n {
                        board.scroll_offset.1 = 0;
                    } else {
                        board.scroll_offset.1 -= n;
                    },
                    Direction::South => if (board.scroll_offset.1 + n) as usize > board.height - board.viewport_size.1 as usize {
                        board.scroll_offset.1 = board.height as u16 - board.viewport_size.1 as u16;
                    } else {
                        board.scroll_offset.1 += n;
                    }
                    _ => ()
                };
            }

            Command::Set(ref s, ref n) => {
                let val = n.resolve(counters) as i16;
                counters.set(s.clone(), val);
            }

            Command::Dec(ref s, ref n) => {
                let val = n.resolve(counters) as i16;
                let initial = counters.get(s);
                counters.set(s.clone(), initial - val);
            }

            Command::If(ref s, op, ref n, ref l) => {
                let val = counters.get(s);
                let cmp = n.resolve(counters) as i16;
                let result = match op {
                    Operator::Equals => val == cmp,
                    Operator::NotEquals => val != cmp,
                    Operator::LessThan => val < cmp,
                    Operator::GreaterThan => val > cmp,
                    Operator::LessThanEquals => val <= cmp,
                    Operator::GreaterThanEquals => val >= cmp,
                };
                if result {
                    advance = !send_robot_to_label(&mut robots[robot_id], l);
                }
            }

            Command::Change(ref c1, ref t1, ref p1, ref c2, ref t2, ref p2) => {
                let c1 = c1.resolve(counters);
                let c2 = c2.resolve(counters);
                let p1 = p1.resolve(counters);
                let p2 = p2.resolve(counters);
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

            Command::Color(ref c) => {
                board.level_at_mut(&robots[robot_id].position).1 = c.resolve(counters).0;
            }

            Command::Char(ref c) => {
                robots[robot_id].ch = c.resolve(counters);
            }

            Command::Goto(ref l) => {
                advance = !send_robot_to_label(&mut robots[robot_id], l);
            }

            Command::Zap(ref l, ref n) => {
                let n = n.resolve(counters);
                for _ in 0..n {
                    let label = robots[robot_id]
                        .program
                        .iter_mut()
                        .find(|c| **c == Command::Label(l.clone()));
                    if let Some(cmd) = label {
                        *cmd = Command::ZappedLabel(l.clone());
                    }
                }
            }

            Command::Send(ref r, ref l) => {
                for (idx, robot) in robots.iter_mut().enumerate() {
                    if r.as_ref() == b"all" || robot.name == *r {
                        let did_send = send_robot_to_label(robot, l);
                        if idx == robot_id {
                            advance = !did_send;
                        }
                    }
                }
            }

            Command::Walk(ref d) => {
                //FIXME: support modified directions.
                robots[robot_id].walk = d.dir;
            }

            Command::Slash(ref s) => {
                if robots[robot_id].current_loc as usize == s.as_bytes().len() {
                    robots[robot_id].current_loc = 0;
                    no_end_cycle = true;
                } else {
                    let dir = match s.as_bytes()[robots[robot_id].current_loc as usize] {
                        b'n' => Some(CardinalDirection::North),
                        b's' => Some(CardinalDirection::South),
                        b'e' => Some(CardinalDirection::East),
                        b'w' => Some(CardinalDirection::West),
                        _ => None,
                    };
                    if let Some(dir) = dir {
                        move_robot(&mut robots[robot_id], board, dir);
                    }
                    robots[robot_id].current_loc += 1;
                    advance = false;
                }
            }

            Command::Go(ref d, ref n) => {
                if robots[robot_id].current_loc > 0 {
                    robots[robot_id].current_loc -= 1;
                } else {
                    robots[robot_id].current_loc = n.resolve(counters) as u8;
                }
                if robots[robot_id].current_loc != 0 {
                    //FIXME: support modified directions.
                    let dir = match d.dir {
                        Direction::North => Some(CardinalDirection::North),
                        Direction::South => Some(CardinalDirection::South),
                        Direction::East => Some(CardinalDirection::East),
                        Direction::West => Some(CardinalDirection::West),
                        _ => None,
                    };
                    if let Some(dir) = dir {
                        move_robot(&mut robots[robot_id], board, dir);
                    }
                    advance = false;
                } else {
                    advance = true;
                    no_end_cycle = true;
                }
            }

            Command::Cycle(ref n) => {
                let n = (n.resolve(counters) % 256) as u8;
                robots[robot_id].cycle = n;
            }

            Command::Explode(ref n) => {
                robots[robot_id].alive = false;
                let n = n.resolve(counters) as u8;
                let &mut (ref mut id, ref mut c, ref mut param) =
                    board.level_at_mut(&robots[robot_id].position);
                *id = Thing::Explosion.to_u8().unwrap();
                *c = state.char_id(CharId::ExplosionStage1);
                *param = Explosion { stage: 0, size: n }.to_param();
            }

            Command::OverlayMode(mode) => if let Some(ref mut overlay) = board.overlay {
                overlay.0 = mode;
            },

            Command::Label(_) => lines_run -= 1,
            Command::ZappedLabel(_) => lines_run -= 1,

            _ => (),
        }

        if advance {
            robots[robot_id].current_line += 1;
        }

        let dir = match robots[robot_id].walk {
            Direction::Idle => None,
            Direction::North => Some(CardinalDirection::North),
            Direction::South => Some(CardinalDirection::South),
            Direction::East => Some(CardinalDirection::East),
            Direction::West => Some(CardinalDirection::West),
            _ => None,
        };

        if let Some(dir) = dir {
            move_robot(&mut robots[robot_id], board, dir);
        }

        if !no_end_cycle && cmd.is_cycle_ending() {
            break;
        }
    }
}

enum BuiltInLabel {
    Thud,
    Edge,
    Bombed,
}

impl Into<ByteString> for BuiltInLabel {
    fn into(self) -> ByteString {
        ByteString::from(match self {
            BuiltInLabel::Thud => "thud",
            BuiltInLabel::Edge => "edge",
            BuiltInLabel::Bombed => "bombed",
        })
    }
}

fn send_robot_to_label<S: Into<ByteString>>(robot: &mut Robot, label: S) -> bool {
    let label = label.into();
    let label_pos = robot
        .program
        .iter()
        .position(|c| c == &Command::Label(label.clone()));
    if let Some(pos) = label_pos {
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

fn move_robot(robot: &mut Robot, board: &mut Board, dir: CardinalDirection) {
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
            if !send_robot_to_label(robot, BuiltInLabel::Edge) {
                send_robot_to_label(robot, BuiltInLabel::Thud);
            }
        }
        MoveResult::Move(new_pos) => {
            let thing = board.thing_at(&new_pos);
            if thing.is_solid() {
                send_robot_to_label(robot, BuiltInLabel::Thud);
            } else {
                board.move_level_to(&robot.position, &new_pos);
                robot.position = new_pos;
            }
        }
    }
}

fn update_board(
    state: &mut WorldState,
    world_path: &Path,
    counters: &mut Counters,
    board: &mut Board,
    robots: &mut Vec<Robot>
) {
    for y in 0..board.height {
        for x in 0..board.width {
            let level_idx = y * board.width + x;
            let thing = Thing::from_u8(board.level[level_idx].0).unwrap();
            match thing {
                Thing::Robot | Thing::RobotPushable => {
                    // FIXME: Account for missing global robot
                    let robot_id = board.level[level_idx].2 - 1;
                    update_robot(state, world_path, counters, board, &mut *robots, robot_id as usize);
                }

                Thing::Explosion => {
                    let mut explosion = Explosion::from_param(board.level[level_idx].2);
                    if explosion.stage == 0 {
                        if explosion.size > 0 {
                            explosion.size -= 1;
                            board.level[level_idx].2 = explosion.to_param();

                            let dirs = [
                                (0i16, -1),
                                (0, 1),
                                (1, 0),
                                (-1, 0),
                            ];
                            for &(xdiff, ydiff) in &dirs {
                                if (y == 0 && ydiff < 0) ||
                                    (x == 0 && xdiff < 0) ||
                                    (y == board.height - 1 && ydiff > 0) ||
                                    (x == board.width - 1  && xdiff > 0)
                                {
                                    continue;
                                }
                                let level_idx = (y as i16 + ydiff) as usize * board.width + (x as i16 + xdiff) as usize;
                                let thing = Thing::from_u8(board.level[level_idx].0).unwrap();
                                if !thing.is_solid() && thing != Thing::Explosion {
                                    board.level[level_idx] = (
                                        Thing::Explosion.to_u8().unwrap(),
                                        0x00,
                                        explosion.to_param(),
                                    );
                                } else if thing == Thing::Robot || thing == Thing::RobotPushable {
                                    let robot_id = board.level[level_idx].2 - 1;
                                    send_robot_to_label(&mut robots[robot_id as usize], BuiltInLabel::Bombed);
                                }
                            }
                        }
                    }

                    if explosion.stage == 3 {
                        board.level[level_idx] = match board.explosion_result {
                            ExplosionResult::Nothing => (
                                Thing::Space.to_u8().unwrap(),
                                0x07,
                                0x00
                            ),
                            ExplosionResult::Ash => (
                                Thing::Floor.to_u8().unwrap(),
                                0x08,
                                0x00,
                            ),
                            ExplosionResult::Fire => (
                                Thing::Fire.to_u8().unwrap(),
                                0x0C,
                                0x00,
                            ),
                        };
                    } else {
                        explosion.stage += 1;
                        board.level[level_idx].2 = explosion.to_param();
                    }
                }

                Thing::Fire => {
                    if rand::random::<u8>() >= 20 {
                        let cur_param = board.level[level_idx].2;
                        if cur_param < 5 {
                            board.level[level_idx].2 += 1;
                        } else {
                            board.level[level_idx].2 = 0;
                        }
                    }

                    let rval = rand::random::<u8>();
                    if rval < 8 {
                        if rval == 1 && !board.fire_burns_forever {
                            board.level[level_idx] = (
                                Thing::Floor.to_u8().unwrap(),
                                0x08,
                                0x00,
                            );
                        }

                        let dirs = [
                            (0i16, -1),
                            (0, 1),
                            (1, 0),
                            (-1, 0),
                        ];
                        for &(xdiff, ydiff) in &dirs {
                            if (y == 0 && ydiff < 0) ||
                                (x == 0 && xdiff < 0) ||
                                (y == board.height - 1 && ydiff > 0) ||
                                (x == board.width - 1  && xdiff > 0)
                            {
                                continue;
                            }
                            let level_idx = (y as i16 + ydiff) as usize * board.width + (x as i16 + xdiff) as usize;
                            let thing_id = board.level[level_idx].0;
                            let thing = Thing::from_u8(thing_id).unwrap();

                            let spread =
                                (thing == Thing::Space && board.fire_burns_space) ||
                                (thing_id >= Thing::Fake.to_u8().unwrap() &&
                                 thing_id <= Thing::ThickWeb.to_u8().unwrap() &&
                                 board.fire_burns_fakes) ||
                                (thing == Thing::Tree && board.fire_burns_trees) ||
                                (board.level[level_idx].1 == 0x06 &&
                                 board.fire_burns_brown &&
                                 board.level[level_idx].0 < Thing::Sensor.to_u8().unwrap());

                            if spread {
                                board.level[level_idx] = (
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
    let window = video_subsystem.window("revenge of megazeux", 640, 480)
      .position_centered()
      .build()
      .unwrap();

    let mut canvas = window.into_canvas().software().build().unwrap();

    canvas.clear();
    canvas.present();

    canvas.set_draw_color(Color::RGBA(255, 255, 255, 255));

    let mut events = sdl_context.event_pump().unwrap();

    const BOARD_ID: usize = 0;
    let is_title_screen = BOARD_ID == 0;

    let mut input_state = InputState {
        left_pressed: false,
        right_pressed: false,
        up_pressed: false,
        down_pressed: false,
        space_pressed: false,
        _to_process: None,
    };

    let mut counters = Counters::new();

    'mainloop: loop {
        let start = time::precise_time_ns();
        for event in events.poll_iter() {
            match event {
                Event::Quit{..} |
                Event::KeyDown {keycode: Option::Some(Keycode::Escape), ..} =>
                    break 'mainloop,
                Event::KeyDown {timestamp, keycode, keymod, repeat, ..} => {
                    handle_key_input(&mut input_state, timestamp, keycode, keymod, repeat, true);
                }
                Event::KeyUp {timestamp, keycode, keymod, repeat, ..} => {
                    handle_key_input(&mut input_state, timestamp, keycode, keymod, repeat, false);
                }
                _ => {}
            }
        }

        let _result = process_input(&mut world.boards[BOARD_ID], &input_state);
        update_board(
            &mut world.state,
            world_path,
            &mut counters,
            &mut world.boards[BOARD_ID],
            &mut world.board_robots[BOARD_ID]
        );

        {
            let mut renderer = SdlRenderer {
                canvas: &mut canvas,
            };
            render(
                &world.state,
                (
                    world.boards[BOARD_ID].upper_left_viewport,
                    world.boards[BOARD_ID].viewport_size,
                ),
                world.boards[BOARD_ID].scroll_offset,
                &world.boards[BOARD_ID],
                &world.board_robots[BOARD_ID],
                &mut renderer,
                is_title_screen,
            );
        }
        canvas.present();
        let now = time::precise_time_ns();
        let _elapsed = now - start;
        const IDEAL: u32 = 1_000_000_000u32 / 60;
        ::std::thread::sleep(Duration::new(0, IDEAL));
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
