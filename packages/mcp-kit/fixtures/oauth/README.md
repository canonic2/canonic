# OAuth fixtures

`acme-idp-rs256.der` is a 2048-bit RSA private key in PKCS#8 DER. The
identity assertion tests use it to sign assertions as the Acme test identity
provider (`https://idp.example.com`). It protects nothing and must never be
used outside tests.
