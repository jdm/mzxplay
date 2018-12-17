#![feature(nll)]

extern crate env_logger;
extern crate libmzx;
#[macro_use] extern crate log;
extern crate num_traits;
extern crate rand;
extern crate sdl2;
extern crate time;

use crate::board::{update_board, enter_board};
use crate::robot::{
    Robots, RobotId, send_robot_to_label, EvaluatedByteString, BuiltInLabel,
};
use libmzx::{
    Renderer, render, load_world, CardinalDirection, Coordinate, Board, Thing, World,
    WorldState, Counters, ByteString, KeyPress,
};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::pixels::Color;
use sdl2::render::Canvas;
use sdl2::video::Window;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::exit;
use std::time::Duration;

mod board;
mod robot;

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

enum StateChange {
    Teleport(ByteString, Coordinate<u16>),
    Restore(usize, Coordinate<u16>),
}

fn tick_game_loop(
    world: &mut World,
    world_path: &Path,
    input_state: &InputState,
    counters: &mut Counters,
    board_id: &mut usize,
) {
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
                enter_board(board, player_pos, &mut world.all_robots);
            } else {
                warn!("Edge of board with no exit.");
            }
        }

        Some(InputResult::Transport(id, color, dest_board_id)) => {
            let dest_board = &mut world.boards[dest_board_id as usize];
            let coord = dest_board.find(id, color).unwrap_or(dest_board.player_pos);
            *board_id = dest_board_id as usize;
            enter_board(dest_board, coord, &mut world.all_robots);
        }

        Some(InputResult::Collide(pos)) => {
            let board = &world.boards[*board_id];
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
        key,
        world_path,
        counters,
        &mut world.boards[*board_id],
        *board_id,
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
            *board_id = id;
            enter_board(&mut world.boards[id], coord, &mut world.all_robots);
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

        tick_game_loop(
            &mut world,
            &world_path,
            &input_state,
            &mut counters,
            &mut board_id,
        );

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
