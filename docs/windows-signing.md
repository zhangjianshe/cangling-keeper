# Windows code signing

Windows release EXEs are signed after the compatibility check and before either
GitHub or software-center upload. Signing and RFC3161 timestamping use SHA-256.
Signing, timestamp, or verification failure stops publication.

The leaf certificate is issued by the existing **Cangling Local Root CA** with
Code Signing EKU and CA:FALSE. Its publisher is **Cangling Software Signing**.
It expires on **2028-11-16 01:48:04 UTC**; renew and replace the two secrets before
that date. The encrypted root private key and its password remain local and are
not supplied to Actions. Only the dedicated leaf private key is uploaded.

Repository secrets:

- `KEEPER_WINDOWS_SIGNING_PFX`: base64-encoded password-encrypted PFX containing
  only the code-signing leaf certificate and its private key.
- `KEEPER_WINDOWS_SIGNING_PASSWORD`: the PFX's independently generated password.

Public certificates (safe to distribute):

- [Root CA](certificates/cangling-root-ca.cer)
- [Code-signing certificate](certificates/cangling-code-signing.cer)

The root DER certificate's SHA-256 fingerprint is:
`89706BE8C7A9B1ACE3F16926EEADAB529BEF814DF666A03E3FC3B6308A1D8BB8`.
The script pins this certificate, uses SignTool and Get-AuthenticodeSignature to
verify the signer and timestamp, then removes the temporary signing key and
root trust from the runner. It never puts a PFX in release assets or artifacts.

This is a private CA, not a publicly trusted Microsoft/CA-issued signature.
On managed Windows computers, verify the root fingerprint through your trusted
administrative channel and import the root into **Trusted Root Certification
Authorities**. SmartScreen or enterprise policies can still show warnings or
block execution. Do not distribute a private key or PFX to end users.

The manually dispatched **Test Windows signing** workflow downloads an existing
release EXE, signs a copy, verifies it on a Windows runner, and saves the signed
copy and public certificates as a 7-day artifact. It does not replace published
release assets or software-center downloads.
