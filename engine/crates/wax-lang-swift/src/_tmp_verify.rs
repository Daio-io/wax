#[cfg(test)]
mod tmp_verify {
    use crate::swift_recovery::normalize_swift_source;
    #[test]
    fn parenthesized_fn_type_edge() {
        let cases = [
            (b"typealias Handler = (_: ()) -> Void".as_slice(), b"typealias Handler = (_: ()) -> Void".as_slice()),
            (b"init(_: ()) { let x = () }".as_slice(), b"init(_: ()) { let x = {} }".as_slice()),
        ];
        for (input, expected) in cases {
            let got = normalize_swift_source(input).bytes;
            assert_eq!(got, expected, "input={:?}", input);
        }
    }
}
