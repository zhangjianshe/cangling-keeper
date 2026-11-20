const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('ui/main.js', 'utf8');
const start = source.indexOf('function promptInitialLogin(');
const end = source.indexOf('\nasync function autoSyncIfLoggedIn(', start);
assert.ok(start >= 0 && end > start);
let prompted = 0;
const data = new Map();
const context = { state: { login: { loggedIn: false } }, sessionStorage: {
  getItem: key => data.get(key), setItem: (key, value) => data.set(key, value),
}, openLoginModal: () => prompted++ };
vm.createContext(context);
vm.runInContext(source.slice(start, end), context);
context.promptInitialLogin(false); assert.equal(prompted, 0, 'failed status lookup does not prompt');
context.state.login.loggedIn = true; context.promptInitialLogin(true); assert.equal(prompted, 0, 'logged-in users are not prompted');
context.state.login.loggedIn = false; context.promptInitialLogin(true); assert.equal(prompted, 1, 'logged-out users can log in');
context.promptInitialLogin(true); assert.equal(prompted, 1, 'refresh does not repeatedly prompt');
console.log('Initial login prompt checks passed.');
