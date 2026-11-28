# Public localhost TLS fixture

The certificate and private key are public test data copied from Registry Kit's
owned HTTPS fixture. The certificate covers localhost, is signed by a private
test CA and is valid from October 7, 2026 through September 13, 2126. Production
HTTP clients do not trust that CA. These files identify no real service and must
never be used for real services or release signing.

`ca.pem` is that fixture's public test CA. The nested proxy TLS test and native
certificate-tunnel unit tests trust it in explicit private test configurations;
production roots remain unchanged. Those tests do not establish successful
default-trust installed HTTPS acceptance.
