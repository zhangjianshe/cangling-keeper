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

- [Root CA](certificates/cangling-code-signing-root.cer)
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

## Install the root from Keeper

The top bar's **安装根证书** button checks the OS trust store and shows the
certificate fingerprint before requesting permission. Installation applies to
**the computer running the Keeper service**, so use its localhost page.

- Windows: approve the UAC prompt to add the public certificate to the Local
  Computer Trusted Root Certification Authorities store, for all users.
- Ubuntu and Debian-based Kylin: authorization uses `pkexec`; Keeper installs the
  public PEM under `/usr/local/share/ca-certificates/` and runs
  `update-ca-certificates`.
- Kylin systems providing `update-ca-trust`: Keeper installs the public PEM under
  `/etc/pki/ca-trust/source/anchors/` and runs `update-ca-trust extract`.

Linux needs `ca-certificates` and, when running without root privileges, a
working polkit authentication agent and `pkexec`. Missing tools, canceled
permission, and failed trust-store updates are reported in the page. Existing
files containing a different certificate are not overwritten. Installation
never needs or distributes a private key.


## 网站根证书与代码签名根证书

顶部“安装根证书”安装新的 `Cangling Root CA`，来源为 `certificates/cangling-root-ca.cer`，与 cangling-update 的网站证书信任链一致，SHA-256 为 `0285F51BB6BEA21C74AD8C13320FFFB3C940438C54A4D8384A3FF2E78E6EBCD1`。

现有 Windows EXE 代码签名仍使用原 `Cangling Local Root CA`，其公开证书保留为 `certificates/cangling-code-signing-root.cer`；签名脚本引用该文件。代码签名证书和 GitHub Secrets 不受本次网站根证书更新影响。新的网站根证书不替代旧代码签名根证书的信任；受管理 Windows 电脑需要验证旧签名时，仍按本文步骤导入代码签名根证书。
