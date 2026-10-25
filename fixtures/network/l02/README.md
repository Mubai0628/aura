# Local L02 / B0 network fixture

`certificate.pem` is a self-signed synthetic test certificate for `proxy-target.test` and
`cn.bing.com`. It reuses L01's published synthetic test key embedded in the Rust and
Electron tests. No owner credential is involved. Issued with the local OpenSSL executable
using SHA-256 and an explicit SAN, CA constraint, key usage and server-auth usage.

The test gateway maps these names to loopback while separately classifying the target as
public. This verifies hostname checks and the proxy route without external DNS or network
access. Trust is installed only by Rust tests or the existing contract-driver test features;
the certificate is never added to the host trust store or the production trust configuration.
