const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('ui/main.js', 'utf8');
const functions = source.slice(source.indexOf('function renderRootCertificateStatus('), source.indexOf('// ---- app self-update'));
const info = { supported: true, installed: false, fingerprint: 'test fingerprint', message: 'Requires administrator authorization' };
async function scenario({ confirmed = true, installed = false, fail = false }) {
  const calls = [], alerts = [], button = {};
  const context = vm.createContext({
    installRootCertBtnEl: button, rootCertificateBusy: false,
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === 'install_root_certificate') {
        if (fail) throw new Error('Authorization canceled');
        installed = true;
      }
      return { ...info, installed };
    },
    uiConfirm: async () => confirmed,
    uiAlert: async text => alerts.push(text),
  });
  vm.runInContext(functions, context);
  await context.installRootCertificate();
  return { calls, alerts, button };
}
(async () => {
  let result = await scenario({});
  assert.equal(result.calls.filter(c => c.command === 'install_root_certificate').length, 1);
  assert.equal(result.button.textContent, '根证书已安装');
  assert.equal(result.button.disabled, true);
  result = await scenario({ confirmed: false });
  assert(!result.calls.some(c => c.command === 'install_root_certificate'));
  assert.equal(result.button.disabled, false);
  result = await scenario({ installed: true });
  assert(!result.calls.some(c => c.command === 'install_root_certificate'));
  result = await scenario({ fail: true });
  assert(result.alerts.some(text => text.includes('Authorization canceled')));
  assert.equal(result.button.disabled, false);
  console.log('Root certificate UI: confirmation, success, existing install and canceled authorization passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
