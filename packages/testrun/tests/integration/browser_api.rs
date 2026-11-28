//! Source compatibility for the public browser library.
use testrun::browser::{
    connection,
    driver::{Diagnostics, Error},
};

#[test]
fn bootstrap_error_exposes_diagnostics_by_value() {
    let mut diagnostics = Diagnostics::default();
    diagnostics.omitted_bytes = 17;
    let error = Error::Bootstrap {
        cause: Box::new(connection::Error::Version),
        diagnostics,
    };
    let Error::Bootstrap { diagnostics, .. } = error else {
        panic!("Expected a bootstrap error");
    };
    let diagnostics: Diagnostics = diagnostics;
    assert_eq!(diagnostics.omitted_bytes, 17);
}
