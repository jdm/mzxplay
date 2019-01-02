use crate::board::put_thing;
use crate::game::{
    reset_view, GameStateChange,
};
use libmzx::{
    KeyPress, Counters, RunStatus, CounterContext, Board, Robot, Command, Thing, WorldState,
    Resolve, adjust_coordinate, dir_to_cardinal_dir, Size, Coordinate, Explosion, ParamValue,
    ColorValue, Color as MzxColor, ByteString, CharId, CardinalDirection, dir_to_cardinal_dir_rel,
    RelativeDirBasis, ExtendedColorValue, ExtendedParam, Operator, CounterContextMut, RelativePart,
    SignedNumeric, MessageBoxLineType, MessageBoxLine,
};
use num_traits::{FromPrimitive, ToPrimitive};
use std::fs::File;
use std::io::Read;
use std::mem;
use std::ops::Deref;
use std::path::Path;

enum MoveResult {
    Move(Coordinate<u16>),
    Edge,
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

fn move_robot_to(robot: &mut Robot, board: &mut Board, pos: Coordinate<u16>) {
    let thing = board.thing_at(&pos);
    if thing == Thing::Player {
        return;
    }
    // TODO: check if thing can move to under layer
    board.move_level_to(&robot.position, &pos);
    robot.position = pos;
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

pub(crate) enum BuiltInLabel {
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
pub(crate) struct EvaluatedByteString(ByteString);

impl EvaluatedByteString {
    pub(crate) fn no_eval_needed(s: ByteString) -> EvaluatedByteString {
        assert!(!s.contains(&b'&'));
        EvaluatedByteString(s)
    }
}

impl Deref for EvaluatedByteString {
    type Target = ByteString;
    fn deref(&self) -> &ByteString {
        &self.0
    }
}

pub(crate) trait Evaluator {
    fn eval<'a>(&self, counters: &Counters, context: CounterContext<'a>) -> EvaluatedByteString;
}

impl Evaluator for ByteString {
    fn eval<'a>(&self, counters: &Counters, context: CounterContext<'a>) -> EvaluatedByteString {
        EvaluatedByteString(self.evaluate(counters, context))
    }
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

#[derive(Copy, Clone, PartialEq)]
pub(crate) enum RobotId {
    Board(usize),
    Global,
}

impl RobotId {
    pub fn from(param: u8) -> RobotId {
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

pub(crate) struct Robots<'a> {
    robots_start: usize,
    num_robots: usize,
    robots: &'a mut [Robot],
}

impl<'a> Robots<'a> {
    pub fn new(board: &Board, robots: &'a mut [Robot]) -> Robots<'a> {
        Robots {
            robots_start: board.robot_range.0,
            num_robots: board.robot_range.1,
            robots,
        }
    }

    pub fn foreach<F: FnMut(&mut Robot, RobotId)>(&mut self, mut f: F) {
        f(&mut self.robots[0], RobotId::Global);
        let end = self.robots_start + self.num_robots;
        for (idx, robot) in self.robots[self.robots_start..end].iter_mut().enumerate() {
            f(robot, RobotId::Board(idx));
        }
    }

    pub fn get(&self, id: RobotId) -> &Robot {
        match id {
            RobotId::Board(id) => &self.robots[self.robots_start + id],
            RobotId::Global => &self.robots[0],
        }
    }

    pub fn get_mut(&mut self, id: RobotId) -> &mut Robot {
        match id {
            RobotId::Board(id) => &mut self.robots[self.robots_start + id],
            RobotId::Global => &mut self.robots[0],
        }
    }

    pub fn find(&self, name: &ByteString) -> Option<RobotId> {
        if &self.robots[0].name == name {
            return Some(RobotId::Global);
        }
        let robots = &self.robots[self.robots_start..self.robots_start + self.num_robots];
        robots.iter().position(|r| &r.name == name).map(RobotId::Board)
    }
}

enum Update {
    Mode(Relative),
    MessageBox(MessageBoxLine)
}

enum CommandResult {
    NoAdvance,
    Advance,
    AdvanceAndChangeState(GameStateChange),
    IgnoreLine(Option<Update>),
    EndCycle,
}

fn run_one_command(
    state: &mut WorldState,
    key: Option<KeyPress>,
    world_path: &Path,
    counters: &mut Counters,
    board: &mut Board,
    board_id: usize,
    robots: &mut Robots,
    robot_id: RobotId,
    mode: Relative,
    cmd: &Command,
) -> CommandResult {
    match *cmd {
        Command::End => return CommandResult::NoAdvance,

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
            if robot.current_loc == 0 {
                return CommandResult::Advance;
            } else {
                return CommandResult::NoAdvance;
            }
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
                if jump_robot_to_label(robot, l) {
                    return CommandResult::NoAdvance;
                }
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
                if jump_robot_to_label(robot, l) {
                    return CommandResult::NoAdvance;
                }
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
                if jump_robot_to_label(robot, l) {
                    return CommandResult::NoAdvance;
                }
            }
        }

        Command::IfPlayerXY(ref x, ref y, ref l) => {
            let robot = robots.get_mut(robot_id);
            let context = CounterContext::from(board, robot, state);
            let pos = mode.resolve_xy(x, y, counters, context, RelativePart::First);
            let l = l.eval(counters, context);
            if board.player_pos == pos {
                if jump_robot_to_label(robot, l) {
                    return CommandResult::NoAdvance;
                }
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
            if jump_robot_to_label(robot, l) {
                return CommandResult::NoAdvance;
            }
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
            let mut result = CommandResult::Advance;
            robots.foreach(|robot, id| {
                if r.as_ref() == b"all" || robot.name == r {
                    let did_send = send_robot_to_label(robot, l.clone());
                    if id == robot_id && did_send {
                        result = CommandResult::NoAdvance;
                    }
                }
            });
            return result;
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
                        //FIXME: might skip a command if sending to own id and advancing
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
                //FIXME: might skip a command if sending to own id and advancing
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
                return CommandResult::Advance;
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
                return CommandResult::NoAdvance;
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
                return CommandResult::NoAdvance;
            } else {
                return CommandResult::Advance;
            }
        }

        Command::TryDir(ref d, ref l) => {
            let robot = robots.get_mut(robot_id);
            let dir = dir_to_cardinal_dir(robot, d);
            if let Some(dir) = dir {
                if move_robot(robot, board, dir) == Move::Blocked {
                    let context = CounterContext::from(board, robot, state);
                    let l = l.eval(counters, context);
                    if jump_robot_to_label(robot, l) {
                        return CommandResult::NoAdvance;
                    }
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
            let mode = Relative::Coordinate(*part, robots.get(robot_id).position);
            return CommandResult::IgnoreLine(Some(Update::Mode(mode)));
        }

        Command::RelPlayer(ref part) => {
            let mode = Relative::Coordinate(*part, board.player_pos);
            return CommandResult::IgnoreLine(Some(Update::Mode(mode)));
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
            let mode = Relative::Coordinate(*part, coord);
            return CommandResult::IgnoreLine(Some(Update::Mode(mode)));
        }

        Command::Teleport(ref b, ref x, ref y) => {
            let context = CounterContext::from(
                board, robots.get(robot_id), state
            );
            let coord = Coordinate(
                x.resolve(counters, context) as u16,
                y.resolve(counters, context) as u16,
            );
            return CommandResult::AdvanceAndChangeState(GameStateChange::Teleport(b.clone(), coord));
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
            return CommandResult::AdvanceAndChangeState(GameStateChange::Restore(board_id, pos));
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
        Command::BlankLine => return CommandResult::IgnoreLine(None),

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
                copy_robot(RobotId::from(param), robot_id, robots, board);
            }
        }

        Command::CopyRobotNamed(ref name) => {
            let source_id = robots.find(name);
            if let Some(source_id) = source_id {
                copy_robot(source_id, robot_id, robots, board);
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

        Command::TakeKey(ref _c, ref l) => {
            // FIXME: actually take a key.
            if let Some(ref l) = *l {
                let context = CounterContext::from(
                    board, robots.get(robot_id), state
                );
                let l = l.eval(counters, context);
                let did_send = send_robot_to_label(robots.get_mut(robot_id), l);
                if !did_send {
                    return CommandResult::NoAdvance;
                }
            }
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
                if jump_robot_to_label(robot, blocked) {
                    return CommandResult::NoAdvance;
                }
            }
        }

        Command::MessageBoxLine(ref s, line_type) => {
            return CommandResult::IgnoreLine(Some(Update::MessageBox(
                MessageBoxLine::Text(s.clone(), line_type)
            )));
        }

        Command::MessageBoxOption(ref counter, ref label, ref text) => {
            return CommandResult::IgnoreLine(Some(Update::MessageBox(
                MessageBoxLine::Option {
                    counter: counter.clone(),
                    label: label.clone(),
                    text: text.clone(),
                }
            )));
        }

        ref cmd => warn!("ignoring {:?}", cmd),
    };

    if cmd.is_cycle_ending() {
        CommandResult::EndCycle
    } else {
        CommandResult::Advance
    }
}

pub(crate) fn update_robot(
    state: &mut WorldState,
    key: Option<KeyPress>,
    world_path: &Path,
    counters: &mut Counters,
    board: &mut Board,
    board_id: usize,
    mut robots: Robots,
    robot_id: RobotId,
) -> Option<GameStateChange> {
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
    let mut message_box_lines = vec![];

    const CYCLES: u8 = 40;
    let state_change = loop {
        let robot = robots.get_mut(robot_id);
        if !robot_id.is_global() {
            if !board.thing_at(&robot.position).is_robot() {
                warn!("current robot not present at reported position; terminating");
                robot.alive = false;
            } else if RobotId::from(board.level_at(&robot.position).2) != robot_id {
                warn!("current robot does not match robot ID at reported position; terminating");
                robot.alive = false;
            }
        }

        if lines_run >= CYCLES || !robot.alive || robot.current_line as usize >= robot.program.len() {
            break None;
        }
        let cmd = robot.program[robot.current_line as usize].clone();
        if !message_box_lines.is_empty() && !cmd.is_message_box() {
            // TODO: restore execution from current robot and cycle.
            break Some(GameStateChange::MessageBox(message_box_lines, robot.name.clone()));
        }

        debug!("evaluating {:?} ({})", cmd, robot.current_line);

        lines_run += 1;

        let old_mode = mem::replace(&mut mode, Relative::None);
        match run_one_command(
            state,
            key,
            world_path,
            counters,
            board,
            board_id,
            &mut robots,
            robot_id,
            old_mode,
            &cmd,
        ) {
            CommandResult::Advance => {
                robots.get_mut(robot_id).current_line += 1;
            }
            CommandResult::AdvanceAndChangeState(state) => {
                robots.get_mut(robot_id).current_line += 1;
                break Some(state);
            }
            CommandResult::IgnoreLine(new_mode) => {
                robots.get_mut(robot_id).current_line += 1;
                lines_run -= 1;
                match new_mode {
                    None if !message_box_lines.is_empty() => {
                        message_box_lines.push(
                            MessageBoxLine::Text("".into(), MessageBoxLineType::Plain)
                        );
                    }
                    None => (),
                    Some(Update::Mode(new_mode)) => mode = new_mode,
                    Some(Update::MessageBox(line)) => {
                        message_box_lines.push(line);
                    }
                }
            }
            CommandResult::NoAdvance => {
                if cmd.is_cycle_ending() {
                    break None;
                }
            }
            CommandResult::EndCycle => {
                robots.get_mut(robot_id).current_line += 1;
                break None;
            }
        }
    };

    robots.get_mut(robot_id).status = RunStatus::FinishedRunning;

    state_change
}

pub(crate) fn send_robot_to_label<S: Into<EvaluatedByteString>>(robot: &mut Robot, label: S) -> bool {
    if robot.locked {
        return false;
    }
    jump_robot_to_label(robot, label)
}

pub(crate) fn jump_robot_to_label<S: Into<EvaluatedByteString>>(robot: &mut Robot, label: S) -> bool {
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
