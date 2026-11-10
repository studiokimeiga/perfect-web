//! **The standard library's lists and strings, compiled** (ADR-0040).
//!
//! Every operation runs through the E8 host over generated inputs, beside the
//! Rust standard library doing the same thing: `Vec`, `str`, `char`. Where
//! the Pleris operation traps (an `Int` overflow inside a function argument,
//! a value that is not a Unicode scalar value), the reference has no value
//! either.

use std::collections::BTreeMap;

use pw_conformance::{Runnable, compile, units};
use pw_host::engine::Val;

const CASES: usize = 200;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn int(&mut self) -> i64 {
        match self.below(12) {
            0 => [i64::MIN, i64::MAX, i64::MAX / 2 + 1][self.below(3) as usize],
            1..=3 => self.below(2000) as i64 - 1000,
            _ => self.below(300) as i64 - 50,
        }
    }
    fn ints(&mut self) -> Vec<i64> {
        (0..self.below(30)).map(|_| self.int()).collect()
    }
    fn string(&mut self) -> String {
        const POOLS: [&str; 6] = [
            "abcXYZ",
            "人市魚時夢空",
            "ひとアイウ",
            "🀄🍣😀",
            " \t\n\u{3000}\u{a0}\u{2009}",
            "ÀéÇñ",
        ];
        let mut s = String::new();
        for _ in 0..self.below(9) {
            let pool: Vec<char> = POOLS[self.below(POOLS.len() as u64) as usize]
                .chars()
                .collect();
            s.push(pool[self.below(pool.len() as u64) as usize]);
        }
        s
    }
}

const PROGRAM: &str = "module s

import List
import String
import Float

type Word = Word {
    text: String,
    score: Int,
}

fn double(n: Int) -> Int { n * 2 }

public query Count(xs: List<Int>) -> Int { List.length(xs) }

public query Doubled(xs: List<Int>) -> List<Int> { List.map(xs, x => x * 2) }

public query Evens(xs: List<Int>) -> List<Int> { xs |> List.filter(x => x % 2 == 0) }

public query Sum(xs: List<Int>) -> Int { List.fold(xs, 0, (total, x) => total + x) }

public query AnyNegative(xs: List<Int>) -> Bool { List.any(xs, x => x < 0) }

public query AllSmall(xs: List<Int>) -> Bool { List.all(xs, x => x < 100) }

public query FirstBig(xs: List<Int>) -> Option<Int> { List.find(xs, x => x > 100) }

public query Nth(xs: List<Int>, i: Int) -> Option<Int> { List.get(xs, i) }

public query Front(xs: List<Int>, n: Int) -> List<Int> { List.take(xs, n) }

public query Both(xs: List<Int>, ys: List<Int>) -> List<Int> { List.concat(xs, ys) }

public query Sorted(xs: List<Int>) -> List<Int> {
    List.sort_by(xs, (a, b) => if a < b { -1 } else { if a > b { 1 } else { 0 } })
}

public query ByScore(words: List<Word>) -> List<Word> {
    List.sort_by(words, (a, b) => if a.score < b.score { 1 } else { if a.score > b.score { -1 } else { 0 } })
}

public query Texts(words: List<Word>) -> List<String> { List.map(words, w => w.text) }

public query Build(n: Int, text: String) -> List<Word> {
    [Word { text: text, score: n }, Word { text: \"{text}!\", score: n + 1 }]
}

public query Chars(text: String) -> Int { String.length(text) }

public query Points(text: String) -> List<Int> { String.codepoints(text) }

public query Rebuilt(points: List<Int>) -> String { String.from_codepoints(points) }

public query RebuiltLength(points: List<Int>) -> Int { String.length(String.from_codepoints(points)) }

public query Prefix(text: String, part: String) -> Bool { String.starts_with(text, part) }

public query Suffix(text: String, part: String) -> Bool { String.ends_with(text, part) }

public query Inside(text: String, part: String) -> Bool { String.contains(text, part) }

public query Joined(parts: List<String>, separator: String) -> String { String.join(parts, separator) }

public query Trimmed(text: String) -> String { String.trim(text) }

public query Lower(text: String) -> String { String.to_lower_ascii(text) }

public query Lengths(texts: List<String>) -> List<Int> { List.map(texts, String.length) }

public query Twice(xs: List<Int>) -> List<Int> { List.map(xs, double) }

public query Runs(words: List<Word>) -> List<List<Int>> {
    List.map(List.group_by(words, w => w.text), run => List.map(run, w => w.score))
}

public query Totals(rows: List<List<Int>>) -> List<Int> {
    List.map(rows, row => List.fold(row, 0, (total, x) => total + x))
}

public query AsFloat(n: Int) -> Float { Float.from_int(n) }

public query Rest(xs: List<Int>, n: Int) -> List<Int> { List.drop(xs, n) }

public query Part(xs: List<Int>, a: Int, b: Int) -> List<Int> { List.slice(xs, a, b) }

public query Backwards(xs: List<Int>) -> List<Int> { List.reverse(xs) }

public query WordsBackwards(words: List<Word>) -> List<Word> { List.reverse(words) }

public query Range(a: Int, b: Int) -> List<Int> { List.range(a, b) }

public query Swapped(xs: List<Int>, i: Int, j: Int) -> List<Int> { List.swap(xs, i, j) }

public query WordsSwapped(words: List<Word>, i: Int, j: Int) -> List<Word> { List.swap(words, i, j) }

public query TextPart(text: String, a: Int, b: Int) -> String { String.slice(text, a, b) }

public query Total(xs: List<Float>) -> Float { List.sum(xs) }

public query Largest(xs: List<Float>) -> Option<Float> { List.maximum(xs) }
";

fn compiled(id: &str) -> Runnable {
    Runnable::new(compile(&units(&[("s.pw", PROGRAM)]), id))
}

fn call(r: &Runnable, args: &[Val]) -> Result<Val, String> {
    r.call(&BTreeMap::new(), args).map(|mut out| out.remove(0))
}

fn ints(xs: &[i64]) -> Val {
    Val::List(xs.iter().map(|x| Val::S64(*x)).collect())
}

fn strings(xs: &[String]) -> Val {
    Val::List(xs.iter().map(|s| Val::String(s.clone())).collect())
}

fn word(text: &str, score: i64) -> Val {
    Val::Record(vec![
        ("text".to_string(), Val::String(text.to_string())),
        ("score".to_string(), Val::S64(score)),
    ])
}

/// The compiled result must be the reference's value, or a trap where the
/// reference has none.
fn agree(name: &str, got: Result<Val, String>, want: Option<Val>, case: &str) {
    match (got, want) {
        (Ok(g), Some(w)) => assert_eq!(g, w, "{name}({case})"),
        (Err(_), None) => {}
        (Ok(g), None) => panic!("{name}({case}) = {g:?}, and the reference has no value"),
        (Err(e), Some(w)) => panic!("{name}({case}) trapped ({e}), and should be {w:?}"),
    }
}

#[test]
fn list_operations_agree_with_vec() {
    let names = [
        "s.Count",
        "s.Doubled",
        "s.Evens",
        "s.Sum",
        "s.AnyNegative",
        "s.AllSmall",
        "s.FirstBig",
        "s.Sorted",
        "s.Twice",
    ];
    let rs: Vec<Runnable> = names.iter().map(|n| compiled(n)).collect();
    let mut rng = Rng(0x115);
    for _ in 0..CASES {
        let xs = rng.ints();
        let case = format!("{xs:?}");
        let doubled: Option<Vec<i64>> = xs.iter().map(|x| x.checked_mul(2)).collect();
        let sum = xs.iter().try_fold(0i64, |a, x| a.checked_add(*x));
        let mut sorted = xs.clone();
        sorted.sort();
        let wants: [Option<Val>; 9] = [
            Some(Val::S64(xs.len() as i64)),
            doubled.as_deref().map(ints),
            Some(ints(
                &xs.iter()
                    .copied()
                    .filter(|x| x.rem_euclid(2) == 0)
                    .collect::<Vec<_>>(),
            )),
            sum.map(Val::S64),
            Some(Val::Bool(xs.iter().any(|x| *x < 0))),
            Some(Val::Bool(xs.iter().all(|x| *x < 100))),
            Some(Val::Option(
                xs.iter()
                    .find(|x| **x > 100)
                    .map(|x| Box::new(Val::S64(*x))),
            )),
            Some(ints(&sorted)),
            doubled.as_deref().map(ints),
        ];
        for ((name, r), want) in names.iter().zip(&rs).zip(wants) {
            agree(name, call(r, &[ints(&xs)]), want, &case);
        }
    }
}

#[test]
fn indexing_taking_and_joining_lists() {
    let (nth, front, both) = (compiled("s.Nth"), compiled("s.Front"), compiled("s.Both"));
    let mut rng = Rng(0x1d);
    for _ in 0..CASES {
        let (xs, ys) = (rng.ints(), rng.ints());
        let i = rng.below(xs.len() as u64 + 7) as i64 - 3;
        assert_eq!(
            call(&nth, &[ints(&xs), Val::S64(i)]),
            Ok(Val::Option(
                usize::try_from(i)
                    .ok()
                    .and_then(|i| xs.get(i))
                    .map(|x| Box::new(Val::S64(*x)))
            )),
            "get({xs:?}, {i})"
        );
        let want: Vec<i64> = xs.iter().copied().take(i.max(0) as usize).collect();
        assert_eq!(
            call(&front, &[ints(&xs), Val::S64(i)]),
            Ok(ints(&want)),
            "take({xs:?}, {i})"
        );
        let both_want: Vec<i64> = xs.iter().chain(&ys).copied().collect();
        assert_eq!(call(&both, &[ints(&xs), ints(&ys)]), Ok(ints(&both_want)));
    }
    // The extremes of `take` and `get`.
    let xs = [1, 2, 3];
    assert_eq!(
        call(&front, &[ints(&xs), Val::S64(i64::MAX)]),
        Ok(ints(&xs))
    );
    assert_eq!(
        call(&front, &[ints(&xs), Val::S64(i64::MIN)]),
        Ok(ints(&[]))
    );
    assert_eq!(
        call(&nth, &[ints(&xs), Val::S64(i64::MIN)]),
        Ok(Val::Option(None))
    );
}

#[test]
fn records_are_sorted_stably_and_built_in_lists() {
    let (by_score, texts, build) = (
        compiled("s.ByScore"),
        compiled("s.Texts"),
        compiled("s.Build"),
    );
    let mut rng = Rng(0x5c);
    for _ in 0..CASES {
        let words: Vec<(String, i64)> = (0..rng.below(25))
            .map(|_| (rng.string(), rng.below(6) as i64))
            .collect();
        let vals = Val::List(words.iter().map(|(t, s)| word(t, *s)).collect());
        // Stable: equal scores keep their order.
        let mut want = words.clone();
        want.sort_by_key(|w| std::cmp::Reverse(w.1));
        assert_eq!(
            call(&by_score, std::slice::from_ref(&vals)),
            Ok(Val::List(want.iter().map(|(t, s)| word(t, *s)).collect())),
        );
        assert_eq!(
            call(&texts, &[vals]),
            Ok(strings(
                &words.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>()
            )),
        );
    }
    assert_eq!(
        call(&build, &[Val::S64(7), Val::String("人".into())]),
        Ok(Val::List(vec![word("人", 7), word("人!", 8)]))
    );
}

#[test]
fn string_operations_agree_with_str() {
    let names = ["s.Chars", "s.Points", "s.Trimmed", "s.Lower"];
    let rs: Vec<Runnable> = names.iter().map(|n| compiled(n)).collect();
    let pairs = ["s.Prefix", "s.Suffix", "s.Inside"];
    let prs: Vec<Runnable> = pairs.iter().map(|n| compiled(n)).collect();
    let mut rng = Rng(0x57);
    for _ in 0..CASES {
        let s = rng.string();
        let wants = [
            Val::S64(s.chars().count() as i64),
            ints(&s.chars().map(|c| c as i64).collect::<Vec<_>>()),
            Val::String(s.trim().to_string()),
            Val::String(s.to_ascii_lowercase()),
        ];
        for ((name, r), want) in names.iter().zip(&rs).zip(wants) {
            assert_eq!(
                call(r, &[Val::String(s.clone())]),
                Ok(want),
                "{name}({s:?})"
            );
        }
        // A part taken from the string itself half the time, so a match is
        // common.
        let part: String = if rng.below(2) == 0 {
            let cs: Vec<char> = s.chars().collect();
            let a = rng.below(cs.len() as u64 + 1) as usize;
            let b = a + rng.below((cs.len() - a) as u64 + 1) as usize;
            cs[a..b].iter().collect()
        } else {
            rng.string()
        };
        let wants = [s.starts_with(&part), s.ends_with(&part), s.contains(&part)];
        for ((name, r), want) in pairs.iter().zip(&prs).zip(wants) {
            assert_eq!(
                call(r, &[Val::String(s.clone()), Val::String(part.clone())]),
                Ok(Val::Bool(want)),
                "{name}({s:?}, {part:?})"
            );
        }
    }
}

#[test]
fn text_is_rebuilt_from_code_points_and_bad_ones_trap() {
    // Returned, a bad string is refused by the host too: the Canonical ABI
    // lifts only valid UTF-8. Measured inside the component, it is not, so
    // `RebuiltLength` is what shows the component's own check. The mutation
    // controls found that the first version of this test could not tell
    // (2026-09-25).
    let rebuilt = compiled("s.Rebuilt");
    let length = compiled("s.RebuiltLength");
    let mut rng = Rng(0xc0);
    for _ in 0..CASES {
        let mut points: Vec<i64> = rng.string().chars().map(|c| c as i64).collect();
        if rng.below(4) == 0 {
            let bad = [-1, 0xD800, 0xDFFF, 0x110000, i64::MAX][rng.below(5) as usize];
            points.insert(rng.below(points.len() as u64 + 1) as usize, bad);
        }
        let want: Option<String> = points
            .iter()
            .map(|p| u32::try_from(*p).ok().and_then(char::from_u32))
            .collect();
        agree(
            "s.RebuiltLength",
            call(&length, &[ints(&points)]),
            want.as_ref().map(|s| Val::S64(s.chars().count() as i64)),
            &format!("{points:?}"),
        );
        agree(
            "s.Rebuilt",
            call(&rebuilt, &[ints(&points)]),
            want.map(Val::String),
            &format!("{points:?}"),
        );
    }
}

#[test]
fn joining_strings_and_functions_by_name() {
    let (joined, lengths, totals) = (
        compiled("s.Joined"),
        compiled("s.Lengths"),
        compiled("s.Totals"),
    );
    let mut rng = Rng(0x10);
    for _ in 0..CASES {
        let parts: Vec<String> = (0..rng.below(6)).map(|_| rng.string()).collect();
        let sep = rng.string();
        assert_eq!(
            call(&joined, &[strings(&parts), Val::String(sep.clone())]),
            Ok(Val::String(parts.join(&sep))),
        );
        assert_eq!(
            call(&lengths, &[strings(&parts)]),
            Ok(ints(
                &parts
                    .iter()
                    .map(|p| p.chars().count() as i64)
                    .collect::<Vec<_>>()
            )),
        );
        let rows: Vec<Vec<i64>> = (0..rng.below(5)).map(|_| rng.ints()).collect();
        let want: Option<Vec<i64>> = rows
            .iter()
            .map(|r| r.iter().try_fold(0i64, |a, x| a.checked_add(*x)))
            .collect();
        agree(
            "s.Totals",
            call(
                &totals,
                &[Val::List(rows.iter().map(|r| ints(r)).collect())],
            ),
            want.map(|w| ints(&w)),
            &format!("{rows:?}"),
        );
    }
}

fn refused(program: &str, id: &str) -> String {
    let err = pw_core::backend::component::compile(&units(&[("r.pw", program)]), id)
        .map(|_| ())
        .expect_err("refused");
    println!("{id}: {err}");
    err
}

#[test]
fn what_stays_outside_is_refused_by_name() {
    // A function passed to a declaration was refused here until 2026-09-25.
    // It is a function value now (ADR-0052), called where the list
    // operation runs it.
    let held = Runnable::new(compile(
        &units(&[(
            "r.pw",
            "module r\n\nimport List\n\nfn apply(xs: List<Int>, f: fn(Int) -> Int) -> List<Int> { List.map(xs, f) }\n\npublic query Q(xs: List<Int>) -> List<Int> { apply(xs, x => x + 1) }\n",
        )]),
        "r.Q",
    ));
    assert_eq!(
        held.call(&BTreeMap::new(), &[ints(&[1, 2])]),
        Ok(vec![ints(&[2, 3])])
    );

    let empty = refused(
        "module r\n\nimport List\n\npublic query Q(n: Int) -> Int { List.length([]) }\n",
        "r.Q",
    );
    assert!(empty.contains("empty list"), "{empty}");

    let unknown = refused(
        "module r\n\nfn shuffle(xs: List<Int>) -> List<Int> !{}\n    intrinsic \"list.shuffle\"\n\npublic query Q(xs: List<Int>) -> List<Int> { shuffle(xs) }\n",
        "r.Q",
    );
    assert!(
        unknown.contains("an intrinsic this backend does not know"),
        "{unknown}"
    );
}

#[test]
fn group_by_finds_runs_of_equal_keys() {
    let runs = compiled("s.Runs");
    let mut rng = Rng(0x9b);
    for _ in 0..CASES {
        // Few distinct texts, so runs are long and common.
        let words: Vec<(String, i64)> = (0..rng.below(20))
            .map(|_| {
                (
                    ["a", "人", "", "a"][rng.below(4) as usize].to_string(),
                    rng.int(),
                )
            })
            .collect();
        let mut want: Vec<Vec<i64>> = Vec::new();
        let mut last: Option<&str> = None;
        for (text, score) in &words {
            match (last, want.last_mut()) {
                (Some(l), Some(run)) if l == text => run.push(*score),
                _ => want.push(vec![*score]),
            }
            last = Some(text);
        }
        assert_eq!(
            call(
                &runs,
                &[Val::List(words.iter().map(|(t, s)| word(t, *s)).collect())]
            ),
            Ok(Val::List(want.iter().map(|r| ints(r)).collect())),
            "{words:?}"
        );
    }
}

/// **`Float.from_int` is IEEE 754's conversion** (ADR-0043): exact up to
/// 2^53 in magnitude, then the nearest `Float`, ties to even, as Rust's
/// `as f64` is.
#[test]
fn an_int_becomes_the_nearest_float() {
    let r = compiled("s.AsFloat");
    let mut rng = Rng(0xf1);
    let mut cases = vec![
        0,
        1,
        -1,
        i64::MAX,
        i64::MIN,
        1 << 53,
        (1 << 53) + 1,
        (1 << 53) + 3,
        -(1 << 53) - 1,
    ];
    cases.extend((0..CASES).map(|_| rng.int()));
    for n in cases {
        assert_eq!(call(&r, &[Val::S64(n)]), Ok(Val::Float64(n as f64)), "{n}");
    }
}

/// A bound as `List.slice` and `String.slice` clamp it: to `0..=len`.
fn clamp(i: i64, len: usize) -> usize {
    usize::try_from(i.max(0)).unwrap_or(usize::MAX).min(len)
}

/// **Slicing, dropping and reversing** (ADR-0055), against `Vec` and `str`:
/// each bound clamped, an end before the start empty.
#[test]
fn slices_drops_and_reversals_agree_with_vec_and_str() {
    let (rest, part, backwards, words_backwards, text_part) = (
        compiled("s.Rest"),
        compiled("s.Part"),
        compiled("s.Backwards"),
        compiled("s.WordsBackwards"),
        compiled("s.TextPart"),
    );
    let mut rng = Rng(0x51);
    let bound = |rng: &mut Rng, len: usize| match rng.below(6) {
        0 => [i64::MIN, i64::MAX, -1][rng.below(3) as usize],
        _ => rng.below(len as u64 + 4) as i64 - 2,
    };
    for _ in 0..CASES {
        let xs = rng.ints();
        let (n, a, b) = (
            bound(&mut rng, xs.len()),
            bound(&mut rng, xs.len()),
            bound(&mut rng, xs.len()),
        );
        let case = format!("{xs:?}, {n}, {a}, {b}");
        assert_eq!(
            call(&rest, &[ints(&xs), Val::S64(n)]),
            Ok(ints(&xs[clamp(n, xs.len())..])),
            "drop({case})"
        );
        let (from, to) = (clamp(a, xs.len()), clamp(b, xs.len()));
        let want: &[i64] = if to > from { &xs[from..to] } else { &[] };
        assert_eq!(
            call(&part, &[ints(&xs), Val::S64(a), Val::S64(b)]),
            Ok(ints(want)),
            "slice({case})"
        );
        let mut reversed = xs.clone();
        reversed.reverse();
        assert_eq!(call(&backwards, &[ints(&xs)]), Ok(ints(&reversed)));

        let words: Vec<(String, i64)> = (0..rng.below(8))
            .map(|_| (rng.string(), rng.int()))
            .collect();
        let vals = Val::List(words.iter().map(|(t, s)| word(t, *s)).collect());
        let want = Val::List(words.iter().rev().map(|(t, s)| word(t, *s)).collect());
        assert_eq!(call(&words_backwards, &[vals]), Ok(want));

        let text = rng.string();
        let cs: Vec<char> = text.chars().collect();
        let (a, b) = (bound(&mut rng, cs.len()), bound(&mut rng, cs.len()));
        let (from, to) = (clamp(a, cs.len()), clamp(b, cs.len()));
        let want: String = if to > from {
            cs[from..to].iter().collect()
        } else {
            String::new()
        };
        assert_eq!(
            call(
                &text_part,
                &[Val::String(text.clone()), Val::S64(a), Val::S64(b)]
            ),
            Ok(Val::String(want)),
            "String.slice({text:?}, {a}, {b})"
        );
    }
}

/// **`List.range` is Rust's `a..b`**: empty where `b` is not past `a`, and a
/// trap for its cause where it would hold more than 2^28 - 1 numbers, which
/// no list's bytes an `i32` counts can. `MIN` to `MAX` among them: the
/// difference of the two wraps a 64-bit subtraction, to -1.
#[test]
fn ranges_agree_with_rust_and_a_range_too_long_traps_for_its_cause() {
    let range = compiled("s.Range");
    let mut rng = Rng(0x7a);
    let mut cases: Vec<(i64, i64)> = vec![
        (0, 0),
        (3, 1),
        (-2, 3),
        (i64::MAX - 3, i64::MAX),
        (i64::MIN, i64::MIN + 2),
        (i64::MIN, i64::MAX),
        (i64::MAX, i64::MIN),
        (-1, i64::MAX),
        (0, 0x0fff_ffff + 1),
    ];
    cases.extend((0..CASES).map(|_| {
        let a = rng.int();
        (a, a.saturating_add(rng.below(40) as i64 - 8))
    }));
    cases.extend((0..CASES).map(|_| (rng.int(), rng.int())));
    for (a, b) in cases {
        let got = call(&range, &[Val::S64(a), Val::S64(b)]);
        if b > a && i128::from(b) - i128::from(a) > 0x0fff_ffff {
            let why = got.expect_err(&format!("List.range({a}, {b}) has no value"));
            assert!(
                why.contains("a value too long for memory"),
                "List.range({a}, {b}) stopped for another cause: {why}"
            );
        } else {
            assert_eq!(
                got,
                Ok(ints(&(a..b).collect::<Vec<_>>())),
                "List.range({a}, {b})"
            );
        }
    }
}

/// **`List.swap` is `Vec::swap` where both are places in the list**, and the
/// list as it is where either is not: a negative place, one past the end,
/// and the bounds of an `Int` among them. Records are moved whole.
#[test]
fn swaps_agree_with_vec_and_a_place_outside_leaves_the_list() {
    let (swapped, words_swapped) = (compiled("s.Swapped"), compiled("s.WordsSwapped"));
    let mut rng = Rng(0x5a);
    let place = |rng: &mut Rng, len: usize| match rng.below(6) {
        0 => [i64::MIN, i64::MAX, -1, len as i64][rng.below(4) as usize],
        _ => rng.below(len as u64 + 1) as i64,
    };
    for _ in 0..CASES {
        let xs = rng.ints();
        let (i, j) = (place(&mut rng, xs.len()), place(&mut rng, xs.len()));
        let within = |k: i64| usize::try_from(k).ok().filter(|k| *k < xs.len());
        let mut want = xs.clone();
        if let (Some(a), Some(b)) = (within(i), within(j)) {
            want.swap(a, b);
        }
        assert_eq!(
            call(&swapped, &[ints(&xs), Val::S64(i), Val::S64(j)]),
            Ok(ints(&want)),
            "List.swap({xs:?}, {i}, {j})"
        );
        let words: Vec<(String, i64)> = (0..rng.below(8))
            .map(|_| (rng.string(), rng.int()))
            .collect();
        let (i, j) = (place(&mut rng, words.len()), place(&mut rng, words.len()));
        let within = |k: i64| usize::try_from(k).ok().filter(|k| *k < words.len());
        let mut want = words.clone();
        if let (Some(a), Some(b)) = (within(i), within(j)) {
            want.swap(a, b);
        }
        let vals = Val::List(words.iter().map(|(t, s)| word(t, *s)).collect());
        let want = Val::List(want.iter().map(|(t, s)| word(t, *s)).collect());
        assert_eq!(
            call(&words_swapped, &[vals, Val::S64(i), Val::S64(j)]),
            Ok(want),
            "List.swap(words, {i}, {j})"
        );
    }
}

/// **`sum` and `maximum` compute** (ADR-0055). Until 2026-09-25 both had
/// placeholder bodies that answered 0.0.
#[test]
fn sum_and_maximum_compute() {
    let (total, largest) = (compiled("s.Total"), compiled("s.Largest"));
    let floats = |xs: &[f64]| Val::List(xs.iter().map(|x| Val::Float64(*x)).collect());
    let mut rng = Rng(0x3a);
    for _ in 0..CASES {
        let xs: Vec<f64> = (0..rng.below(12))
            .map(|_| match rng.below(8) {
                0 => -0.0,
                1 => f64::INFINITY,
                2 => -1e308,
                _ => (rng.int() as f64) / 7.0,
            })
            .collect();
        let sum = xs.iter().fold(0.0, |t, x| t + x);
        match call(&total, &[floats(&xs)]) {
            Ok(Val::Float64(got)) => assert!(
                got.to_bits() == sum.to_bits() || (got.is_nan() && sum.is_nan()),
                "sum({xs:?}) = {got}, not {sum}"
            ),
            other => panic!("sum({xs:?}) = {other:?}"),
        }
        // IEEE 754's `maximum`: +0 is larger than −0 (ADR-0213).
        let max = xs.iter().copied().reduce(|m, x| {
            if x > m || (x == m && x.is_sign_positive() && m.is_sign_negative()) {
                x
            } else {
                m
            }
        });
        // By its bits: `-0.0 == 0.0`, and a comparison of values would not
        // see which zero it is.
        match (call(&largest, &[floats(&xs)]), max) {
            (Ok(Val::Option(Some(got))), Some(m)) => assert!(
                matches!(*got, Val::Float64(g) if g.to_bits() == m.to_bits()),
                "maximum({xs:?}) = {got:?}, not {m:?}"
            ),
            (Ok(Val::Option(None)), None) => {}
            (other, m) => panic!("maximum({xs:?}) = {other:?}, not {m:?}"),
        }
    }
    // +0 is larger than −0, in either order; two −0s are −0 (ADR-0213).
    for (xs, expected) in [
        (vec![-0.0, 0.0], 0.0f64),
        (vec![0.0, -0.0], 0.0),
        (vec![-0.0, -1.0, 0.0], 0.0),
        (vec![-0.0, -0.0], -0.0),
        (vec![-0.0], -0.0),
    ] {
        match call(&largest, &[floats(&xs)]) {
            Ok(Val::Option(Some(v))) => assert!(
                matches!(*v, Val::Float64(m) if m.to_bits() == expected.to_bits()),
                "{xs:?}: {v:?}"
            ),
            other => panic!("{xs:?}: {other:?}"),
        }
    }
    // A NaN is the maximum of any list with one, wherever it is.
    for xs in [
        [f64::NAN, 1.0, 2.0],
        [1.0, f64::NAN, 2.0],
        [1.0, 2.0, f64::NAN],
    ] {
        match call(&largest, &[floats(&xs)]) {
            Ok(Val::Option(Some(v))) => {
                assert!(matches!(*v, Val::Float64(m) if m.is_nan()), "{xs:?}: {v:?}")
            }
            other => panic!("maximum({xs:?}) = {other:?}"),
        }
    }
    assert_eq!(call(&largest, &[floats(&[])]), Ok(Val::Option(None)));
    assert_eq!(call(&total, &[floats(&[])]), Ok(Val::Float64(0.0)));
}
