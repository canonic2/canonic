# Owned TLS test fixtures

These certificates and the server private key are public test data for the owned
localhost TLS server in the `test-support` feature. The CA certificate is
supplied only to the test acquisition client. Production acquisition uses
bundled public CA roots; its constructor exposes no verification bypass.

The leaf certificate covers `localhost`. It and its CA are valid from October 7,
2026 through September 13, 2126. The CA signing key is not retained. These files
must never be used for real services, signing releases or identifying publishers.
