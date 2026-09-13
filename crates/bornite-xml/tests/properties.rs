//! Arbitrary input never panics the readers.

use bornite_xml::{read_rules_document, read_vote_document};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..1024)) {
        let text = String::from_utf8_lossy(&bytes);
        let _ = read_rules_document(&text);
        let _ = read_vote_document(&text);
    }

    #[test]
    fn xml_shaped_input_never_panics(
        parts in prop::collection::vec(
            prop::sample::select(vec![
                "<voting-rules", "<vote", " version=\"1.0\"", " version=\"9\"", ">", "/>",
                "</voting-rules>", "</vote>", "<weight type=\"equal\"/>", "<weight type=\"x\"/>",
                "<exclusions enabled=\"true\"/>", "<quorum type=\"none\"/>",
                "<quorum type=\"fraction\" numerator=\"1\" denominator=\"0\" basis=\"total-electorate\"/>",
                "<threshold type=\"simple-majority\" basis=\"votes-cast\"/>", "<threshold/>",
                "<abstentions treatment=\"exclude\"/>", "<tie treatment=\"reject\"/>",
                "<electorate>", "</electorate>", "<voter id=\"a\"/>", "<voter id=\"a\" weight=\"0\"/>",
                "<ballots>", "</ballots>", "<ballot voter=\"a\" choice=\"yes\"/>", "<ballot/>",
                "<!-- c -->", "text", "&amp;", "\u{0}",
            ]),
            0..30,
        ),
    ) {
        let text = parts.concat();
        let _ = read_rules_document(&text);
        let _ = read_vote_document(&text);
    }
}
