//! A small expression language that works out a number from the frame: the
//! data-driven half of the pipeline (M11), used today to bake a motion into
//! keys (`Editor::bake_expression`).
//!
//! ```text
//! value + 12 * sin(frame / 4)         a wobble on top of what is keyed
//! blink(frame, 7, 96, 4)              1 for 4 frames, once somewhere in each 96
//! clamp(noise(time * 2, 3), 0, 1)     a drifting number between 0 and 1
//! if(frame < 24, 0, 100)              a step
//! ```
//!
//! Everything is a number (a comparison gives 1 or 0, and anything but 0 is
//! true). There are no loops, no assignment and no recursion: a program is a
//! tree of at most [`MAX_NODES`] nodes, checked when it is parsed (unknown
//! names, wrong argument counts, depth), so evaluating it takes time in
//! proportion to its size and cannot hang. A result that is not a finite
//! number (a division by zero, a square root of a negative) is an error, never
//! a NaN in a document. Every function is pure: the random ones (`noise`,
//! `random`, `blink`) are functions of their arguments (a seed and a place),
//! so a frame gives the same number whatever was evaluated before it, on any
//! machine.
//!
//! | Names | Meaning |
//! |---|---|
//! | `frame`, `time`, `fps` | the frame being worked out, its time in seconds from the first frame, and the frame rate |
//! | `start`, `end` | the animation's first and last frame |
//! | `first`, `last`, `progress` | the range being baked, and how far through it the frame is (0 to 1) |
//! | `value` | what the property has on that frame now (degrees for a rotation, percent for opacity, as the graph shows it) |
//! | `pi`, `e` | constants |
//! | `sin cos tan abs sqrt floor ceil round sign fract` | one number in |
//! | `min max pow mod` | two numbers in |
//! | `clamp(x, lo, hi)` `lerp(a, b, t)` `smoothstep(a, b, x)` `if(c, a, b)` | three in |
//! | `noise(x, seed)` | smooth random between -1 and 1 that drifts as `x` changes |
//! | `random(seed, n)` | a random number from 0 up to (not including) 1 for each whole `n` |
//! | `blink(t, seed, period, length)` | 1 for `length` frames at a random place in every `period` frames, else 0 |

use std::fmt;

/// The longest source text, in characters.
pub const MAX_SOURCE: usize = 1000;
/// The most nodes (numbers, names, operators, calls) a program may have.
pub const MAX_NODES: usize = 200;
/// How deeply brackets and calls may nest.
const MAX_DEPTH: usize = 24;

/// Why an expression could not be read or worked out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprError(pub String);

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExprError {}

fn err<T>(message: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError(message.into()))
}

/// What a program can read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Context {
    pub frame: f64,
    pub time: f64,
    pub fps: f64,
    pub start: f64,
    pub end: f64,
    pub first: f64,
    pub last: f64,
    pub progress: f64,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Var {
    Frame,
    Time,
    Fps,
    Start,
    End,
    First,
    Last,
    Progress,
    Value,
    Pi,
    E,
}

const VARS: [(&str, Var); 11] = [
    ("frame", Var::Frame),
    ("time", Var::Time),
    ("fps", Var::Fps),
    ("start", Var::Start),
    ("end", Var::End),
    ("first", Var::First),
    ("last", Var::Last),
    ("progress", Var::Progress),
    ("value", Var::Value),
    ("pi", Var::Pi),
    ("e", Var::E),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Func {
    Sin,
    Cos,
    Tan,
    Abs,
    Sqrt,
    Floor,
    Ceil,
    Round,
    Sign,
    Fract,
    Min,
    Max,
    Pow,
    Mod,
    Clamp,
    Lerp,
    Smoothstep,
    If,
    Noise,
    Random,
    Blink,
}

/// Name, function and how many arguments it takes.
const FUNCS: [(&str, Func, usize); 21] = [
    ("sin", Func::Sin, 1),
    ("cos", Func::Cos, 1),
    ("tan", Func::Tan, 1),
    ("abs", Func::Abs, 1),
    ("sqrt", Func::Sqrt, 1),
    ("floor", Func::Floor, 1),
    ("ceil", Func::Ceil, 1),
    ("round", Func::Round, 1),
    ("sign", Func::Sign, 1),
    ("fract", Func::Fract, 1),
    ("min", Func::Min, 2),
    ("max", Func::Max, 2),
    ("pow", Func::Pow, 2),
    ("mod", Func::Mod, 2),
    ("clamp", Func::Clamp, 3),
    ("lerp", Func::Lerp, 3),
    ("smoothstep", Func::Smoothstep, 3),
    ("if", Func::If, 3),
    ("noise", Func::Noise, 2),
    ("random", Func::Random, 2),
    ("blink", Func::Blink, 4),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Num(f64),
    Var(Var),
    Neg(Box<Node>),
    Not(Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

/// A program that has been read and checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    root: Node,
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Name(String),
    Sym(&'static str),
}

/// The symbols, longest first so `<=` is not read as `<` then `=`.
const SYMBOLS: [&str; 20] = [
    "<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "^", "<", ">", "!", "(", ")", ",", "?", ":",
];

fn tokens(src: &str) -> Result<Vec<(Tok, usize)>, ExprError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let from = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[from..i].iter().collect();
            let n: f64 = text
                .parse()
                .map_err(|_| ExprError(format!("at character {}: \"{text}\" is not a number", from + 1)))?;
            out.push((Tok::Num(n), from + 1));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let from = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push((Tok::Name(chars[from..i].iter().collect()), from + 1));
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let Some(sym) = SYMBOLS.iter().find(|s| rest.starts_with(**s)) else {
                return err(format!(
                    "at character {}: \"{c}\" is not part of an expression",
                    i + 1
                ));
            };
            out.push((Tok::Sym(sym), i + 1));
            i += sym.len();
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    at: usize,
    nodes: usize,
}

/// How tightly an operator binds, and whether it groups to the right.
fn binding(sym: &str) -> Option<(u8, bool, Op)> {
    Some(match sym {
        "||" => (1, false, Op::Or),
        "&&" => (2, false, Op::And),
        "<" => (3, false, Op::Lt),
        ">" => (3, false, Op::Gt),
        "<=" => (3, false, Op::Le),
        ">=" => (3, false, Op::Ge),
        "==" => (3, false, Op::Eq),
        "!=" => (3, false, Op::Ne),
        "+" => (4, false, Op::Add),
        "-" => (4, false, Op::Sub),
        "*" => (5, false, Op::Mul),
        "/" => (5, false, Op::Div),
        "%" => (5, false, Op::Rem),
        "^" => (7, true, Op::Pow),
        _ => return None,
    })
}

/// A prefix operator's operand binds tighter than any binary operator but `^`,
/// so `-2 ^ 2` is `-(2 ^ 2)`.
const PREFIX: u8 = 6;

impl Parser {
    fn here(&self) -> usize {
        self.toks
            .get(self.at)
            .map_or_else(|| self.toks.last().map_or(1, |t| t.1 + 1), |t| t.1)
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at).map(|t| &t.0)
    }

    fn node(&mut self) -> Result<(), ExprError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return err(format!(
                "the expression is too long: it has more than {MAX_NODES} parts"
            ));
        }
        Ok(())
    }

    fn eat(&mut self, sym: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Sym(s)) if *s == sym) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, sym: &str) -> Result<(), ExprError> {
        if self.eat(sym) {
            Ok(())
        } else {
            err(format!("at character {}: expected \"{sym}\"", self.here()))
        }
    }

    fn expr(&mut self, min: u8, depth: usize) -> Result<Node, ExprError> {
        if depth > MAX_DEPTH {
            return err(format!("the expression nests deeper than {MAX_DEPTH} levels"));
        }
        let mut left = self.prefix(depth)?;
        while let Some(Tok::Sym(sym)) = self.peek() {
            let Some((bp, right, op)) = binding(sym) else {
                break;
            };
            if bp < min {
                break;
            }
            self.at += 1;
            self.node()?;
            let rhs = self.expr(if right { bp } else { bp + 1 }, depth + 1)?;
            left = Node::Bin(op, Box::new(left), Box::new(rhs));
        }
        Ok(left)
    }

    fn prefix(&mut self, depth: usize) -> Result<Node, ExprError> {
        let at = self.here();
        let Some(tok) = self.peek().cloned() else {
            return err(format!("at character {at}: the expression ends too soon"));
        };
        self.at += 1;
        self.node()?;
        match tok {
            Tok::Num(n) => Ok(Node::Num(n)),
            Tok::Sym("-") => Ok(Node::Neg(Box::new(self.expr(PREFIX, depth + 1)?))),
            Tok::Sym("!") => Ok(Node::Not(Box::new(self.expr(PREFIX, depth + 1)?))),
            Tok::Sym("(") => {
                let inner = self.expr(0, depth + 1)?;
                self.expect(")")?;
                Ok(inner)
            }
            Tok::Name(name) => {
                if self.eat("(") {
                    self.call(&name, at, depth)
                } else if let Some((_, v)) = VARS.iter().find(|(n, _)| *n == name) {
                    Ok(Node::Var(*v))
                } else if FUNCS.iter().any(|(n, ..)| *n == name) {
                    err(format!(
                        "at character {at}: \"{name}\" is a function: write {name}(...)"
                    ))
                } else {
                    err(format!(
                        "at character {at}: \"{name}\" is not a name this language knows (try frame, time or value)"
                    ))
                }
            }
            Tok::Sym(s) => err(format!("at character {at}: \"{s}\" cannot start here")),
        }
    }

    fn call(&mut self, name: &str, at: usize, depth: usize) -> Result<Node, ExprError> {
        let Some((_, func, arity)) = FUNCS.iter().find(|(n, ..)| *n == name) else {
            return err(format!("at character {at}: there is no function \"{name}\""));
        };
        let mut args = Vec::new();
        if !self.eat(")") {
            loop {
                args.push(self.expr(0, depth + 1)?);
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        if args.len() != *arity {
            return err(format!(
                "at character {at}: {name} takes {arity} number{} but was given {}",
                if *arity == 1 { "" } else { "s" },
                args.len()
            ));
        }
        Ok(Node::Call(*func, args))
    }
}

impl Expr {
    /// Read and check a program. The message says where it went wrong.
    pub fn parse(src: &str) -> Result<Self, ExprError> {
        if src.chars().count() > MAX_SOURCE {
            return err(format!("the expression is longer than {MAX_SOURCE} characters"));
        }
        if src.trim().is_empty() {
            return err("the expression is empty");
        }
        let toks = tokens(src)?;
        let mut parser = Parser {
            toks,
            at: 0,
            nodes: 0,
        };
        let root = parser.expr(0, 0)?;
        if parser.at < parser.toks.len() {
            return err(format!("at character {}: this is left over", parser.here()));
        }
        Ok(Self { root })
    }

    /// Work the program out. A result that is not a finite number is an error.
    pub fn eval(&self, cx: &Context) -> Result<f64, ExprError> {
        eval(&self.root, cx)
    }
}

fn finite(x: f64, what: &str) -> Result<f64, ExprError> {
    if x.is_finite() {
        Ok(x)
    } else {
        err(format!("{what} is not a finite number"))
    }
}

fn eval(node: &Node, cx: &Context) -> Result<f64, ExprError> {
    let truth = |b: bool| if b { 1.0 } else { 0.0 };
    match node {
        Node::Num(n) => finite(*n, "a number"),
        Node::Var(v) => Ok(match v {
            Var::Frame => cx.frame,
            Var::Time => cx.time,
            Var::Fps => cx.fps,
            Var::Start => cx.start,
            Var::End => cx.end,
            Var::First => cx.first,
            Var::Last => cx.last,
            Var::Progress => cx.progress,
            Var::Value => cx.value,
            Var::Pi => std::f64::consts::PI,
            Var::E => std::f64::consts::E,
        }),
        Node::Neg(a) => Ok(-eval(a, cx)?),
        Node::Not(a) => Ok(truth(eval(a, cx)? == 0.0)),
        Node::Bin(op, a, b) => {
            // && and || only work out the right side when it can matter
            if matches!(op, Op::And | Op::Or) {
                let left = eval(a, cx)? != 0.0;
                return Ok(match (op, left) {
                    (Op::And, false) => 0.0,
                    (Op::Or, true) => 1.0,
                    _ => truth(eval(b, cx)? != 0.0),
                });
            }
            let (x, y) = (eval(a, cx)?, eval(b, cx)?);
            match op {
                Op::Add => finite(x + y, "a sum"),
                Op::Sub => finite(x - y, "a difference"),
                Op::Mul => finite(x * y, "a product"),
                Op::Div if y == 0.0 => err("dividing by zero"),
                Op::Div => finite(x / y, "a quotient"),
                Op::Rem if y == 0.0 => err("taking a remainder by zero"),
                Op::Rem => finite(x.rem_euclid(y), "a remainder"),
                Op::Pow => finite(x.powf(y), "a power"),
                Op::Lt => Ok(truth(x < y)),
                Op::Gt => Ok(truth(x > y)),
                Op::Le => Ok(truth(x <= y)),
                Op::Ge => Ok(truth(x >= y)),
                Op::Eq => Ok(truth(x == y)),
                Op::Ne => Ok(truth(x != y)),
                Op::And | Op::Or => unreachable!("handled above"),
            }
        }
        Node::Call(Func::If, args) => {
            // only the branch taken is worked out
            let branch = if eval(&args[0], cx)? != 0.0 {
                &args[1]
            } else {
                &args[2]
            };
            eval(branch, cx)
        }
        Node::Call(func, args) => {
            let mut a = [0.0; 4];
            for (slot, arg) in a.iter_mut().zip(args) {
                *slot = eval(arg, cx)?;
            }
            call(*func, a)
        }
    }
}

fn call(func: Func, a: [f64; 4]) -> Result<f64, ExprError> {
    let [x, y, z, w] = a;
    match func {
        Func::Sin => finite(x.sin(), "sin"),
        Func::Cos => finite(x.cos(), "cos"),
        Func::Tan => finite(x.tan(), "tan"),
        Func::Abs => Ok(x.abs()),
        Func::Sqrt if x < 0.0 => err("the square root of a negative number"),
        Func::Sqrt => Ok(x.sqrt()),
        Func::Floor => Ok(x.floor()),
        Func::Ceil => Ok(x.ceil()),
        Func::Round => Ok(x.round()),
        Func::Sign => Ok(if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        }),
        Func::Fract => Ok(x - x.floor()),
        Func::Min => Ok(x.min(y)),
        Func::Max => Ok(x.max(y)),
        Func::Pow => finite(x.powf(y), "a power"),
        Func::Mod if y == 0.0 => err("taking a remainder by zero"),
        Func::Mod => finite(x.rem_euclid(y), "a remainder"),
        Func::Clamp => Ok(x.clamp(y.min(z), y.max(z))),
        Func::Lerp => finite(x + (y - x) * z, "a blend"),
        Func::Smoothstep if x == y => Ok(if z < x { 0.0 } else { 1.0 }),
        Func::Smoothstep => {
            let t = ((z - x) / (y - x)).clamp(0.0, 1.0);
            Ok(t * t * (3.0 - 2.0 * t))
        }
        Func::If => unreachable!("handled with its branches"),
        Func::Noise => {
            let (i, f) = (x.floor(), x - x.floor());
            let u = f * f * (3.0 - 2.0 * f);
            let (p, q) = (hash01(y, i), hash01(y, i + 1.0));
            Ok((p + (q - p) * u) * 2.0 - 1.0)
        }
        Func::Random => Ok(hash01(x, y)),
        Func::Blink => {
            let period = z.floor().max(1.0);
            let length = w.floor().clamp(0.0, period);
            let slot = (x / period).floor();
            let offset = x - slot * period;
            let begins = hash01(y, slot) * (period - length);
            Ok(if offset >= begins && offset < begins + length {
                1.0
            } else {
                0.0
            })
        }
    }
}

/// A number from 0 up to (not including) 1 for a seed and a place: SplitMix64
/// of the two, so it is the same on every machine and needs no state.
fn hash01(seed: f64, n: f64) -> f64 {
    let a = (seed.floor() as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let b = (n.floor() as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let mut x = (a ^ b).wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(src: &str, frame: f64) -> Result<f64, ExprError> {
        let cx = Context {
            frame,
            time: frame / 24.0,
            fps: 24.0,
            start: 1.0,
            end: 120.0,
            first: 1.0,
            last: 120.0,
            progress: (frame - 1.0) / 119.0,
            value: 10.0,
        };
        Expr::parse(src)?.eval(&cx)
    }

    fn num(src: &str) -> f64 {
        at(src, 1.0).unwrap_or_else(|e| panic!("{src}: {e}"))
    }

    #[test]
    fn arithmetic_follows_the_usual_precedence() {
        assert_eq!(num("2 + 3 * 4"), 14.0);
        assert_eq!(num("(2 + 3) * 4"), 20.0);
        assert_eq!(num("-2 ^ 2"), -4.0, "a power binds tighter than a minus");
        assert_eq!(num("2 ^ 3 ^ 2"), 512.0, "and groups to the right");
        assert_eq!(num("10 - 4 - 3"), 3.0);
        assert_eq!(num("7 % 3 + -7 % 3"), 1.0 + 2.0, "a remainder is never negative");
        assert_eq!(num("1 < 2 && 2 < 3"), 1.0);
        assert_eq!(num("!(1 == 1) || 0"), 0.0);
        assert_eq!(num("3 >= 3") + num("3 != 3") + num("2 <= 1"), 1.0);
        assert_eq!(num(".5 + 1.25"), 1.75);
    }

    #[test]
    fn names_and_functions_read_the_context() {
        assert_eq!(at("frame * 2", 12.0).unwrap(), 24.0);
        assert_eq!(at("time", 48.0).unwrap(), 2.0);
        assert_eq!(at("value + progress", 120.0).unwrap(), 11.0);
        assert_eq!(num("min(3, 1) + max(3, 1) + abs(-2)"), 6.0);
        assert_eq!(num("clamp(5, 10, 0)"), 5.0, "the bounds may come in either order");
        assert_eq!(num("lerp(10, 20, 0.25)"), 12.5);
        assert_eq!(
            num("floor(2.7) + ceil(2.1) + round(2.5) + sign(-9)"),
            2.0 + 3.0 + 3.0 - 1.0
        );
        assert_eq!(num("if(1 > 2, 5, 7)"), 7.0);
        assert_eq!(num("smoothstep(0, 10, 5)"), 0.5);
        assert!((num("sin(pi / 2) + cos(0) + e - e") - 2.0).abs() < 1e-12);
        assert_eq!(num("mod(-1, 24)"), 23.0);
        assert!((num("fract(2.75)") - 0.75).abs() < 1e-12);
    }

    #[test]
    fn the_random_functions_depend_only_on_their_arguments() {
        for f in [1.0, 2.0, 50.0, 1000.0] {
            let a = at("noise(frame / 10, 3)", f).unwrap();
            // asked again, in any order, the same
            assert_eq!(a, at("noise(frame / 10, 3)", f).unwrap());
            assert!((-1.0..=1.0).contains(&a));
            let r = at("random(7, frame)", f).unwrap();
            assert!((0.0..1.0).contains(&r));
        }
        // a different seed gives a different sequence
        let same = (1..60)
            .filter(|f| at("random(1, frame)", f64::from(*f)) == at("random(2, frame)", f64::from(*f)))
            .count();
        assert!(same < 3);
        // noise is continuous: neighbouring tenths differ little
        let step =
            (at("noise(frame / 100, 5)", 51.0).unwrap() - at("noise(frame / 100, 5)", 50.0).unwrap()).abs();
        assert!(step < 0.1);
    }

    #[test]
    fn a_blink_is_one_burst_of_the_right_length_in_every_period() {
        let (period, length) = (24, 3);
        let src = format!("blink(frame, 9, {period}, {length})");
        for slot in 0..20 {
            let lit: Vec<i32> = (slot * period..(slot + 1) * period)
                .filter(|f| at(&src, f64::from(*f)).unwrap() == 1.0)
                .collect();
            assert_eq!(lit.len() as i32, length, "slot {slot}");
            assert!(lit.windows(2).all(|w| w[1] == w[0] + 1), "one burst: {lit:?}");
        }
        // the bursts do not all sit at the same place in their period
        let places: std::collections::HashSet<i32> = (0..20)
            .map(|slot| {
                (slot * period..(slot + 1) * period)
                    .find(|f| at(&src, f64::from(*f)).unwrap() == 1.0)
                    .unwrap()
                    - slot * period
            })
            .collect();
        assert!(places.len() > 5, "{places:?}");
        // a length longer than the period is the whole period, and a period of 0 is read as 1
        assert_eq!(at("blink(5, 1, 4, 99)", 0.0).unwrap(), 1.0);
        assert!(at("blink(5, 1, 0, 3)", 0.0).is_ok());
    }

    #[test]
    fn a_wrong_program_says_where_and_what() {
        let bad = |src: &str, needle: &str| {
            let e = Expr::parse(src)
                .and_then(|x| x.eval(&Context::default()))
                .unwrap_err()
                .0;
            assert!(e.contains(needle), "{src:?} gave {e:?}, wanted {needle:?}");
        };
        bad("", "empty");
        bad("   ", "empty");
        bad("1 +", "ends too soon");
        bad("(1 + 2", "expected \")\"");
        bad("1 2", "left over");
        bad("1 # 2", "character 3");
        bad("foo", "\"foo\" is not a name");
        bad("sin", "is a function");
        bad("sin(1, 2)", "sin takes 1 number but was given 2");
        bad("clamp(1)", "clamp takes 3 numbers but was given 1");
        bad("bogus(1)", "no function \"bogus\"");
        bad("1 / 0", "dividing by zero");
        bad("5 % 0", "remainder by zero");
        bad("sqrt(-1)", "square root");
        bad("pow(10, 400)", "not a finite number");
        bad("1.2.3", "not a number");
        bad(")", "cannot start here");
        bad("sin(", "ends too soon");
        // an unused branch is not worked out
        assert_eq!(num("if(1, 4, 1 / 0)"), 4.0);
        assert_eq!(num("0 && (1 / 0)"), 0.0);
    }

    #[test]
    fn hostile_programs_are_refused_or_finish_and_never_panic() {
        // too long, too deep, too many parts
        assert!(Expr::parse(&"1+".repeat(600)).is_err());
        assert!(
            Expr::parse(&format!("{}1{}", "(".repeat(50), ")".repeat(50)))
                .unwrap_err()
                .0
                .contains("deeper")
        );
        assert!(Expr::parse(&"-".repeat(100)).unwrap_err().0.contains("deeper"));
        let many = vec!["1"; 150].join("+");
        assert!(Expr::parse(&many).unwrap_err().0.contains("too long"));
        assert!(Expr::parse("1+1+1+1+1+1+1+1+1+1").is_ok());
        // 5,000 random strings from the language's own pieces
        let pieces = [
            "1",
            "2.5",
            "frame",
            "value",
            "(",
            ")",
            ",",
            "+",
            "-",
            "*",
            "/",
            "%",
            "^",
            "<",
            "==",
            "&&",
            "!",
            "sin",
            "if",
            "noise",
            "blink",
            "clamp",
            " ",
            "9999999999999999999999",
            ".",
            "e",
            "pi",
        ];
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..5_000 {
            let len = (next() % 14) as usize;
            let src: String = (0..len)
                .map(|_| pieces[(next() % pieces.len() as u64) as usize])
                .collect();
            if let Ok(program) = Expr::parse(&src) {
                let _ = program.eval(&Context::default());
            }
        }
        // and arbitrary text, including non-ASCII
        for src in ["é", "1 + ٣", "𝔘", "\u{0}", "1\n+\n2", "a\u{200b}b"] {
            let _ = Expr::parse(src);
        }
    }
}
