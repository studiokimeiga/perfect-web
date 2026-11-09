//! **An identity is the same bytes whichever build computes it.** A document
//! written by one build is resumed by another, so what these functions make
//! of their inputs is a format. Each case states it by hand: FNV-1a's published
//! values, and each hash's text as written out here.

use pw_resume::{
    DependencySet, HandlerId, Hash, HashScheme, ImplementationHash, PlatformAbi, SchemaHash,
};

/// FNV-1a at 64 bits, from the algorithm's published test values.
#[test]
fn a_hash_is_fnv_1a_in_sixteen_lower_case_digits() {
    assert_eq!(Hash::of("").to_string(), "cbf29ce484222325");
    assert_eq!(Hash::of("a").to_string(), "af63dc4c8601ec8c");
    assert_eq!(Hash::of("foobar").to_string(), "85944171f73967e8");
}

/// Leading zeroes are digits too: this input's hash begins with one.
#[test]
fn a_hash_keeps_its_leading_zeroes() {
    assert_eq!(Hash::of("10").to_string(), "07f89207b4ba08a4");
}

#[test]
fn a_schema_is_its_fields_as_name_colon_type() {
    assert_eq!(
        SchemaHash::of_fields(&[("a", "T"), ("b", "U")]).0,
        Hash::of("a:T,b:U")
    );
    assert_eq!(SchemaHash::of_fields(&[]).0, Hash::of(""));
}

const X: [&str; 5] = ["p", "m", "n", "x", "r"];
const Y: [&str; 5] = ["p", "m", "n", "y", "r"];

/// A set: however its members were found, and however often.
#[test]
fn a_dependency_set_is_sorted_and_has_no_repeats() {
    assert_eq!(DependencySet::of(&[X, Y]), DependencySet::of(&[Y, X]));
    assert_eq!(DependencySet::of(&[X, Y, X]), DependencySet::of(&[X, Y]));
    assert_ne!(DependencySet::of(&[X]), DependencySet::of(&[Y]));
}

/// **A handler's identity, as its text is written**: the scheme and the
/// digest, then the dependencies', the schema's and the ABI's, each after
/// `\u{1}`. A build whose identities differ refuses every open tab.
#[test]
fn a_handlers_identity_is_this_text_hashed() {
    let dependencies = Hash::of(&format!("{}\u{2}{}", X.join("\u{3}"), Y.join("\u{3}")));
    let schema = Hash::of("a:T");
    let derived = HandlerId::derive(
        &ImplementationHash::new(HashScheme(2), "digest"),
        &DependencySet::of(&[X, Y]),
        &SchemaHash::of_fields(&[("a", "T")]),
        &PlatformAbi(1),
    );
    assert_eq!(
        derived.0,
        Hash::of(&format!("2:digest\u{1}{dependencies}\u{1}{schema}\u{1}1"))
    );
}

/// Numbers with more than one digit, written whole and in order.
#[test]
fn a_handlers_identity_writes_each_number_whole() {
    let derived = HandlerId::derive(
        &ImplementationHash::new(HashScheme(120), "d"),
        &DependencySet::of(&[]),
        &SchemaHash::of_fields(&[]),
        &PlatformAbi(3045),
    );
    let (none, nothing) = (Hash::of(""), Hash::of(""));
    assert_eq!(
        derived.0,
        Hash::of(&format!("120:d\u{1}{none}\u{1}{nothing}\u{1}3045"))
    );
}
