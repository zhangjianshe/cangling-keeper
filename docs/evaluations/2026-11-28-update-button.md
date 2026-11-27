# Update-button verification — 2026-11-28

**Both paths passed by clicking the actual update button** in the installed
Keeper's account settings. No administrator password, sudo, pkexec or permission
dialog was used during this verification.

| Case | Actual browser/backend result |
|---|---|
| Healthy update, fixture 0.0.139 → 0.0.140 | Downloaded the executable, validated its version probe, launched the separate user-systemd helper, replaced the program, restarted the system service, passed PID/version health checks, and automatically reloaded the page. Settings showed v0.0.140 and 已是最新版本. |
| Startup failure, fixture 0.0.140 → 0.0.141 | Downloaded a valid ELF whose version probe succeeds but normal startup exits with failure. The helper restored the prior executable and restarted it through systemd. Backend reported rolled_back; the browser displayed 新程序启动验证失败，已恢复原程序. Serving version remained 0.0.140. |

Both helpers ran in separate `/user.slice/user-1000.slice/user@1000.service/app.slice/`
cgroups. The serving PIDs matched `cangling-keeper-local.service`'s MainPID.
Systemd retained ports 80 and 443 throughout. An intermediate restart of the old
version was observed when the browser's reconnect request socket-activated it
before the helper's explicit stop; the helper then stopped it and completed the
intended update. This did not prevent success or rollback reporting.

The tests used controlled localhost downloads without changing the public release
server. Healthy fixtures were private copies of the current installed ELF, with
same-length replacements of only the two download-base URL literals and the three
package-version literals. Executable logic and the browser update handler were
unchanged. The failing fixture was a minimal compiled ELF with a successful
version probe and a failing normal startup. Actual downloads and outcomes were
observed, not substituted with mocked responses.

**Cleanup verified:** original v0.0.139 executable restored byte-for-byte; original
production update feed and previous update-result file restored; localhost feed
and monitor stopped. HTTP and certificate-verified HTTPS returned the restored
systemd PID, with zero effective capabilities. Session, message and project counts
matched the pre-test database snapshot. No code changes were needed for these
button flows.

Download transport used localhost HTTP, so this verifies the button, download
handling, unprivileged helper, systemd lifecycle and rollback; it does not certify
availability or TLS of the public release host. Application HTTPS was separately
verified after restoration.

Machine-readable evidence is in `2026-11-28-update-button.json`. Screenshots and raw
request/event logs remain in the local paths recorded there.
