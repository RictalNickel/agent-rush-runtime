'use strict';
const $ = s => document.querySelector(s);
const ui = { csrf: null, jobs: [], selected: null, status: null, refreshing: false, stamped: new Set() };
const terminal = s => ['succeeded', 'failed', 'cancelled'].includes(s);

const EXAMPLES = {
  revenue: { timeout: '60', prompt: 'Generate 1,000 deterministic sales rows (seeded PRNG, regions North/South/East/West, dates across 2025) in Rust, compute revenue totals per quarter and per region, and write them to /work/artifacts/result.json. Print the grand total.' },
  primes: { timeout: '30', prompt: 'Find the first 200 primes two ways, by trial division and with a sieve, and prove the two lists match. Write the count, the sum and the largest prime to /work/artifacts/result.json.' },
  runaway: { timeout: '15', prompt: 'Write a Rust program that walks the Collatz sequence for every number from 1 upward and never stops, printing a new record whenever a longer sequence appears. Run it and report what happened.' },
};
const STATE_LABEL = { queued: 'Queued', planning: 'Planning', executing: 'Running', verifying: 'Checking', cleaning: 'Cleaning up', succeeded: 'Succeeded', failed: 'Failed', cancelled: 'Cancelled' };
const REASON_LABEL = {
  completed: 'Completed', timeout: 'Stopped at the time limit', output_limit: 'Stopped at the output limit',
  execution_failed: 'Program failed', review_rejected: 'Reviewer rejected the answer', cancelled: 'Cancelled',
  preflight_rejected: 'Rejected before running', controller_error: 'Stopped by an error', worker_error: 'Worker error',
  worker_restarted: 'Worker restarted', controller_restarted: 'Controller restarted', creation_uncertain: 'Sandbox start uncertain',
};
const EVENT_LABEL = {
  queued: 'Queued', admitted: 'Memory reserved', planning: 'Planning', plan: 'Plan', tool_started: 'Agent action',
  tool_finished: 'Action recorded', executing: 'Sandbox started', node_run: 'Sent to the worker', verifying: 'Results checked',
  subordinate: 'Coder reported back', review: 'Review', explanation: 'Answer written', succeeded: 'Succeeded',
  failed: 'Failed', cancelled: 'Cancelled', error: 'Error',
};
const PROVIDER_LABEL = { vultr: 'Vultr Serverless Inference', anthropic: 'Anthropic Claude' };
const ROLE_ORDER = ['orchestrator', 'coder', 'worker', 'reviewer'];

const label = (map, key) => map[key] || String(key || '').replace(/_/g, ' ').replace(/^./, c => c.toUpperCase());
const seconds = ms => ms < 10000 ? `${(ms / 1000).toFixed(1)} s` : ms < 120000 ? `${Math.round(ms / 1000)} s` : `${Math.floor(ms / 60000)} min ${Math.round(ms % 60000 / 1000)} s`;
const money = usd => usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(3)}`;
const choice = name => document.querySelector(`input[name="${name}"]:checked`).value;
function el(tag, { dataset, ...props } = {}, ...children) {
  const node = Object.assign(document.createElement(tag), props);
  if (dataset) Object.assign(node.dataset, dataset);
  node.append(...children);
  return node;
}

function toast(message) {
  const box = $('#toast');
  box.textContent = message;
  // Re-showing moves it above any dialog opened since it last appeared.
  if (box.matches(':popover-open')) box.hidePopover();
  box.showPopover();
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => box.hidePopover(), 5500);
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    credentials: 'same-origin', ...options,
    headers: { 'Content-Type': 'application/json', ...(ui.csrf ? { 'X-CSRF-Token': ui.csrf } : {}), ...options.headers },
  });
  const value = await response.json().catch(() => ({ error: 'The server sent an unexpected response.' }));
  if (!response.ok) {
    if (response.status === 401) { ui.csrf = null; updateSession(); }
    throw new Error(value.error || value.detail || `Request failed with status ${response.status}.`);
  }
  return value;
}

function cost(receipt) {
  const calls = Array.isArray(receipt?.inference_calls) ? receipt.inference_calls : [];
  return { calls: calls.length, usd: calls.reduce((total, call) => total + (Number(call.accounted_usd) || 0), 0) };
}

function elapsed(job) {
  const events = job.events || [];
  if (events.length < 2) return null;
  return new Date(events[events.length - 1].at) - new Date(events[0].at);
}

// Agent ids are for the evidence export; the timeline names the role instead.
function readable(message) {
  const match = /^Agent [0-9a-f-]{36} at depth (\d+) requested an allowed tool$/.exec(message || '');
  if (!match) return message;
  return match[1] === '0' ? 'The orchestrator used one of its tools.' : 'A delegated agent used one of its tools.';
}

function stateNode(state) {
  const tone = state === 'succeeded' || state === 'failed' ? state : ['queued', 'cancelled'].includes(state) ? '' : 'running';
  return el('span', { className: `state ${tone}`.trim(), textContent: label(STATE_LABEL, state) });
}

// System status, team roles and the sandbox panel come from the public status endpoint.
function updateDeployment() {
  const s = ui.status;
  if (!s) return;
  const signal = $('#worker-signal');
  signal.className = `signal ${s.worker_connected ? 'online' : 'offline'}`;
  $('#deploy-status').textContent = s.worker_connected ? 'Worker online' : 'Worker unavailable';
  $('#deploy-label').textContent = s.inference_provider ? label(PROVIDER_LABEL, s.inference_provider) : 'Inference not reported';
  $('#spec-runtime').textContent = s.runtime === 'runsc' ? 'gVisor' : s.runtime || 'gVisor';
  const roles = s.agent_roles || {};
  const models = [...new Set(ROLE_ORDER.map(r => roles[r]?.model).filter(Boolean))];
  if (models.length === 1 && s.inference_provider) {
    $('#team-model').textContent = `Every role runs ${models[0]} on ${label(PROVIDER_LABEL, s.inference_provider)}, each at its own reasoning effort.`;
  } else if (models.length) {
    $('#team-model').textContent = 'Each role runs the model chosen for its job.';
  }
  for (const role of ROLE_ORDER) {
    const spec = roles[role];
    const slot = document.querySelector(`[data-role="${role}"] .effort`);
    if (!slot) continue;
    const effort = spec?.reasoning_effort || spec?.effort;
    slot.textContent = !spec ? '' : models.length > 1 ? `${spec.model}, ${effort} effort` : effort ? `Reasoning effort: ${effort}` : '';
  }
}

function updateSession() {
  const signedIn = Boolean(ui.csrf);
  $('#login-button').textContent = signedIn ? 'Sign out' : 'Operator sign in';
  $('#empty-login').hidden = signedIn;
  updateReadiness();
  renderJobs();
}

function updateReadiness() {
  const ready = ui.status?.inference_configured && ui.status?.worker_connected;
  $('#run-button').textContent = ui.csrf && !ready ? 'Queue task' : 'Run in sandbox';
  $('#readiness').textContent = !ui.csrf
    ? 'Sign in as the operator to run tasks.'
    : ready
      ? 'Each run gets a fresh sandbox with the limits shown here.'
      : 'The worker or inference is unavailable. Tasks wait in the queue until both are back.';
}

function updateSpec() {
  const memory = Number(choice('memory'));
  const timeout = Number(choice('timeout'));
  const set = (id, text) => {
    const node = $(id);
    if (node.textContent === text) return;
    node.textContent = text;
    node.classList.remove('changed');
    void node.offsetWidth;
    node.classList.add('changed');
  };
  set('#spec-memory', memory >= 1024 ? `${memory / 1024} GiB` : `${memory} MiB`);
  set('#spec-timeout', timeout >= 120 ? `${timeout / 60} minutes per run` : `${timeout} seconds per run`);
}

function renderJobs() {
  const body = $('#runs-body');
  body.replaceChildren();
  const signedIn = Boolean(ui.csrf);
  $('#empty-state').hidden = signedIn && ui.jobs.length > 0;
  $('#empty-text').textContent = signedIn
    ? 'No runs yet. Describe a task above and run it.'
    : 'Sign in as the operator to run tasks and see their receipts.';
  if (!signedIn) return;
  for (const job of ui.jobs) {
    const receipt = job.receipt;
    const spent = cost(receipt);
    const total = elapsed(job);
    const task = el('button', { className: 'run-task', textContent: job.request.prompt, title: job.request.prompt, type: 'button' });
    task.addEventListener('click', () => openRun(job.id));
    const cleanup = receipt?.cleanup_state ?? (['queued', 'cancelled'].includes(job.state) ? 'Not started' : 'Pending');
    body.append(el('tr', {},
      el('td', {}, task),
      el('td', { dataset: { label: 'State' } }, stateNode(job.state)),
      el('td', { className: 'num', dataset: { label: 'Time' }, textContent: terminal(job.state) && total != null ? seconds(total) : '…' }),
      el('td', { className: 'num', dataset: { label: 'Model cost' }, textContent: spent.calls ? money(spent.usd) : '—' }),
      el('td', { className: cleanup === 'verified' ? 'verified' : '', dataset: { label: 'Cleanup' }, textContent: cleanup === 'verified' ? 'Verified' : label({}, cleanup) }),
    ));
  }
}

async function refresh() {
  if (ui.refreshing) return;
  ui.refreshing = true;
  try {
    ui.status = await api('/api/status');
    updateReadiness();
    updateDeployment();
    if (ui.csrf) {
      ui.jobs = await api('/api/jobs');
      renderJobs();
      if (ui.selected && $('#run-dialog').open) {
        const job = ui.jobs.find(j => j.id === ui.selected);
        if (job) renderRun(job);
      }
    }
  } catch (error) {
    toast(error.message);
  } finally {
    ui.refreshing = false;
  }
}

async function openRun(id) {
  try {
    const job = await api(`/api/jobs/${encodeURIComponent(id)}`);
    ui.selected = id;
    renderRun(job);
    if (!$('#run-dialog').open) $('#run-dialog').showModal();
  } catch (error) {
    toast(error.message);
  }
}

function fact(term, value, tone = '') {
  return el('div', {}, el('dt', { textContent: term }), el('dd', { className: tone, textContent: value }));
}

function renderRun(job) {
  const receipt = job.receipt;
  const spent = cost(receipt);
  const total = elapsed(job);
  const id = encodeURIComponent(job.id);
  $('#run-id').textContent = `Run ${job.id.slice(0, 8)}`;
  $('#run-title').textContent = job.request.prompt;
  $('#run-state').replaceWith(Object.assign(stateNode(job.state), { id: 'run-state' }));
  $('#cancel-button').hidden = terminal(job.state);
  $('#contexts-link').href = `/api/jobs/${id}/contexts`;
  $('#evidence-link').href = `/api/jobs/${id}/evidence`;

  const facts = [];
  if (receipt?.reason) facts.push(fact('Outcome', label(REASON_LABEL, receipt.reason), receipt.reason === 'completed' ? 'good' : 'bad'));
  if (terminal(job.state) && total != null) facts.push(fact('Total time', seconds(total)));
  if (typeof receipt?.duration_ms === 'number') facts.push(fact('In the sandbox', seconds(receipt.duration_ms)));
  if (receipt && receipt.exit_code !== undefined && receipt.exit_code !== null) facts.push(fact('Exit code', String(receipt.exit_code), receipt.exit_code === 0 ? '' : 'bad'));
  if (spent.calls) facts.push(fact('Model calls', String(spent.calls)), fact('Model cost', money(spent.usd)));
  if (receipt?.review) {
    const verdict = receipt.review.approved === true ? 'Approved' : receipt.review.approved === false ? 'Rejected' : 'Not available';
    facts.push(fact('Review', verdict, receipt.review.approved === true ? 'good' : receipt.review.approved === false ? 'bad' : ''));
  }
  facts.push(fact('Limits', `${job.memory_mb} MiB, ${job.request.timeout_seconds} s`));
  facts.push(fact('Cleanup', receipt?.cleanup_state === 'verified' ? 'Verified' : label({}, receipt?.cleanup_state ?? 'not started'), receipt?.cleanup_state === 'verified' ? 'good' : ''));
  $('#run-facts').replaceChildren(...facts);

  // The stamp lands once per run, when its cleanup is confirmed.
  const stamp = $('#run-stamp');
  const destroyed = receipt?.cleanup_state === 'verified';
  stamp.hidden = !destroyed;
  if (destroyed) {
    $('#stamp-detail').textContent = receipt.reason && receipt.reason !== 'completed' ? `${label(REASON_LABEL, receipt.reason)}. Cleanup verified.` : 'Sandbox cleanup verified';
    if (!ui.stamped.has(job.id)) {
      ui.stamped.add(job.id);
      stamp.classList.remove('land');
      void stamp.offsetWidth;
      stamp.classList.add('land');
    }
  }

  const events = $('#run-events');
  events.replaceChildren();
  for (const event of job.events) {
    const tone = ['failed', 'error'].includes(event.kind) ? 'bad' : event.kind === 'succeeded' ? 'good' : '';
    const time = el('time', { dateTime: event.at, textContent: new Date(event.at).toLocaleTimeString(undefined, { hour12: false }) });
    const what = el('span', { className: 'what' },
      el('span', { className: 'kind', textContent: label(EVENT_LABEL, event.kind) }),
      el('span', { className: 'detail', textContent: readable(event.message) }));
    events.append(el('li', { className: tone }, time, what));
  }

  $('#output-section').hidden = !receipt;
  if (!receipt) return;
  $('#run-explanation').textContent = receipt.explanation || 'The agent did not write an answer.';
  $('#run-output').textContent = [receipt.stdout, receipt.stderr].filter(Boolean).join('\n') || 'The program printed nothing.';
  const artifacts = $('#artifact-list');
  artifacts.replaceChildren();
  for (const artifact of receipt.artifacts || []) {
    const link = el('a', { textContent: artifact.name, href: `/api/jobs/${id}/artifacts/${encodeURIComponent(artifact.name)}`, download: artifact.name });
    const seal = el('code', { textContent: `${artifact.bytes.toLocaleString()} bytes, SHA-256 ${artifact.sha256}` });
    artifacts.append(el('div', { className: 'artifact' }, link, seal));
  }
  if (!artifacts.children.length) artifacts.append(el('p', { className: 'note', textContent: 'This run produced no files.' }));
}

function openLogin() {
  $('#login-dialog').showModal();
  $('#password').focus();
}

$('#task-form').addEventListener('submit', async event => {
  event.preventDefault();
  if (!ui.csrf) { openLogin(); return; }
  const prompt = $('#prompt').value.trim();
  if (prompt.length < 3) { toast('Describe the task in a few words first.'); $('#prompt').focus(); return; }
  const button = $('#run-button');
  button.disabled = true;
  try {
    const payload = { prompt, profile: 'cli', memory_mb: Number(choice('memory')), timeout_seconds: Number(choice('timeout')) };
    const job = await api('/api/jobs', { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify(payload) });
    await refresh();
    await openRun(job.id);
  } catch (error) {
    toast(error.message);
  } finally {
    button.disabled = false;
  }
});

$('#login-button').addEventListener('click', async () => {
  if (!ui.csrf) { openLogin(); return; }
  try {
    await api('/api/logout', { method: 'POST', body: 'null' });
    ui.csrf = null;
    ui.jobs = [];
    updateSession();
    toast('Signed out.');
  } catch (error) {
    toast(error.message);
  }
});
$('#empty-login').addEventListener('click', openLogin);

$('#login-form').addEventListener('submit', async event => {
  event.preventDefault();
  const button = event.target.querySelector('button[type="submit"]');
  button.disabled = true;
  $('#login-error').textContent = '';
  try {
    const session = await api('/api/login', { method: 'POST', body: JSON.stringify({ password: $('#password').value }) });
    ui.csrf = session.csrf;
    $('#password').value = '';
    $('#login-dialog').close();
    updateSession();
    await refresh();
    toast('Signed in as the operator.');
  } catch (error) {
    $('#login-error').textContent = error.message;
  } finally {
    button.disabled = false;
  }
});

$('#cancel-button').addEventListener('click', async () => {
  if (!ui.selected) return;
  const button = $('#cancel-button');
  button.disabled = true;
  try {
    await api(`/api/jobs/${encodeURIComponent(ui.selected)}/cancel`, { method: 'POST', body: 'null' });
    await refresh();
    toast('Cancelled. The sandbox memory stays reserved until cleanup is confirmed.');
  } catch (error) {
    toast(error.message);
  } finally {
    button.disabled = false;
  }
});

for (const chip of document.querySelectorAll('[data-example]')) {
  chip.addEventListener('click', () => {
    const example = EXAMPLES[chip.dataset.example];
    $('#prompt').value = example.prompt;
    document.querySelector(`input[name="timeout"][value="${example.timeout}"]`).checked = true;
    updateSpec();
    $('#prompt').focus();
  });
}
document.querySelectorAll('input[name="memory"], input[name="timeout"]').forEach(input => input.addEventListener('change', updateSpec));
$('#refresh-button').addEventListener('click', refresh);
$('#safety-button').addEventListener('click', () => $('#safety-dialog').showModal());
document.querySelectorAll('[data-close]').forEach(button => button.addEventListener('click', () => document.getElementById(button.dataset.close).close()));
$('#run-dialog').addEventListener('close', () => { ui.selected = null; });

(async () => {
  try {
    const response = await fetch('/api/session', { credentials: 'same-origin' });
    if (response.ok) ui.csrf = (await response.json()).csrf;
  } catch { /* signed out */ }
  updateSession();
  await refresh();
  setInterval(() => { if (!document.hidden) refresh(); }, 2500);
})();
