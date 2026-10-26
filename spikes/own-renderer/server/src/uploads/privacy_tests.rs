//! Confirmed ignored-byte carriers, with complete valid image neighbors.
use super::strip::tests::{fixture, profiles_in};
use super::tests::Required;
use super::{icc, strip};

#[test]
fn png_ambiguous_color_chunks_are_refused() {
    for name in ["privacy-late-chrm.png", "privacy-duplicate-chrm.png"] {
        assert!(
            strip::strip(&fixture(name)).is_err(),
            "ambiguous color chunk is refused"
        );
    }
}

#[test]
fn png_unused_compressed_or_decoded_bytes_are_refused() {
    for name in [
        "privacy-idat-suffix.png",
        "privacy-later-idat-suffix.png",
        "privacy-nonconsecutive-complete-idat.png",
        "privacy-decoded-suffix.png",
        "privacy-bad-adler.png",
        "privacy-truncated-zlib.png",
    ] {
        assert!(
            strip::strip(&fixture(name)).is_err(),
            "unused image bytes are refused"
        );
    }
}

#[test]
fn png_animation_streams_are_complete_bounded_and_well_formed() {
    for name in [
        "privacy-apng-suffix.png",
        "privacy-orphan-fdat.png",
        "privacy-apng-sequence.png",
        "privacy-apng-rectangle.png",
        "privacy-apng-count.png",
        "privacy-nonconsecutive-idat.png",
    ] {
        assert!(
            strip::strip(&fixture(name)).is_err(),
            "invalid frame stream is refused"
        );
    }
}

#[test]
fn png_split_streams_and_both_animation_layouts_remain_images() {
    for name in [
        "privacy-clean.png",
        "privacy-split-idat.png",
        "privacy-apng-included.png",
        "privacy-apng-excluded.png",
        "privacy-apng-split.png",
    ] {
        let kept = strip::strip(&fixture(name)).required("a valid image is kept");
        assert_eq!(
            strip::strip(&kept),
            Ok(kept),
            "valid image rewriting is idempotent"
        );
    }
}

#[test]
fn icc_duplicate_signatures_are_dropped_without_dropping_shared_curves() {
    let kept = strip::strip(&fixture("privacy-duplicate-icc.png"))
        .required("a PNG without its ambiguous profile");
    assert!(
        profiles_in(&kept).is_empty(),
        "duplicate ICC signature drops the whole profile"
    );
    assert!(!kept.windows(12).any(|part| part == b"ICC-PRIVATE!"));
    let valid =
        strip::strip(&fixture("privacy-valid-icc.png")).required("a real shared-curve profile");
    assert_eq!(profiles_in(&valid).len(), 1);
    let profile = fixture("sRGB2014.icc");
    let scrubbed =
        icc::scrub(&profile).required("distinct signatures sharing one curve remain valid");
    assert_eq!(icc::scrub(&scrubbed), Some(scrubbed));
}

#[test]
fn identical_duplicate_icc_signature_is_also_ambiguous() {
    let mut profile = fixture("sRGB2014.icc");
    let signature = profile[132..136].to_vec();
    profile[144..148].copy_from_slice(&signature);
    assert_eq!(
        icc::scrub(&profile),
        None,
        "duplicate signature at a distinct range"
    );
    let mut shared = fixture("sRGB2014.icc");
    let entry = shared[132..144].to_vec();
    shared[144..156].copy_from_slice(&entry);
    assert_eq!(
        icc::scrub(&shared),
        None,
        "duplicate signature at the identical offset and size"
    );
}

#[test]
fn packed_sixteen_bit_and_interlaced_neighbors_rewrite_identically() {
    for name in [
        "layout-0-1",
        "layout-0-8",
        "layout-0-16",
        "layout-2-8",
        "layout-2-16",
        "layout-4-16",
        "layout-6-16",
        "layout-3-1",
        "layout-3-2",
        "layout-3-4",
        "adam7-tiny",
    ] {
        let image = fixture(&format!("privacy-{name}.png"));
        assert_eq!(strip::strip(&image), Ok(image));
    }
}

#[test]
fn png_interleaved_frame_metadata_is_explicitly_unsupported() {
    assert!(
        strip::strip(&fixture("privacy-apng-interleaved.png")).is_err(),
        "a browser-ambiguous APNG arrangement is refused"
    );
}
