'use strict';
const $ = s => document.querySelector(s);
const ui = { csrf: null, jobs: [], status: null, refreshing: false, loaded: false, stamped: new Set(), open: new Set(), turns: new Map() };
const terminal = s => ['succeeded', 'failed', 'cancelled'].includes(s);

const EXAMPLES = {
  revenue: 'Generate 1,000 deterministic sales rows (seeded PRNG, regions North/South/East/West, dates across 2025), compute revenue totals per quarter and per region, and write them to /work/artifacts/result.json. Print the grand total.',
  primes: 'Find the first 200 primes two ways, by trial division and with a sieve, and prove the two lists match. Write the count, the sum and the largest prime to /work/artifacts/result.json.',
  memory: 'Generate 20 million pseudo-random u32 values with a seeded generator, sort them, and report the minimum, maximum, median and a checksum of the sorted values in /work/artifacts/result.json.',
  runaway: 'Write a Rust program that walks the Collatz sequence for every number from 1 upward and never stops, printing a new record whenever a longer sequence appears. Run it and report what happened.',
};
const STATE_LABEL = { queued: 'Queued', planning: 'Planning', executing: 'Running', verifying: 'Checking', cleaning: 'Cleaning up', succeeded: 'Succeeded', failed: 'Failed', cancelled: 'Cancelled' };
const REASON_LABEL = {
  completed: 'Completed', timeout: 'Stopped at the time limit', output_limit: 'Stopped at the output limit',
  memory_limit: 'Stopped at the memory ceiling', execution_failed: 'Program failed', review_rejected: 'Reviewer rejected the answer',
  cancelled: 'Cancelled', preflight_rejected: 'Rejected before running', controller_error: 'Stopped by an error',
  worker_error: 'Worker error', worker_restarted: 'Worker restarted', controller_restarted: 'Controller restarted',
  creation_uncertain: 'Sandbox start uncertain',
};
const EVENT_LABEL = {
  queued: 'Queued', admitted: 'Memory reserved', planning: 'Planning', plan: 'Plan', limits: 'Ceiling set',
  tool_started: 'Agent action', tool_finished: 'Action recorded', executing: 'Sandbox started', node_run: 'Sent to the worker',
  verifying: 'Results checked', subordinate: 'Coder reported back', review: 'Review', explanation: 'Answer written',
  succeeded: 'Succeeded', failed: 'Failed', cancelled: 'Cancelled', error: 'Error',
};
const NOW_LABEL = {
  queued: 'Waiting for a free sandbox', admitted: 'Starting', planning: 'Planning the task', plan: 'Planning the task',
  tool_started: 'Working', tool_finished: 'Working', executing: 'Running in the sandbox', node_run: 'Running in the sandbox',
  verifying: 'Checking the results', subordinate: 'The coder reported back', review: 'Reviewing the answer', explanation: 'Writing the answer',
};
const PROVIDER_LABEL = { vultr: 'Vultr Serverless Inference', anthropic: 'Anthropic Claude' };

const label = (map, key) => map[key] || String(key || '').replace(/_/g, ' ').replace(/^./, c => c.toUpperCase());
const seconds = ms => ms < 10000 ? `${(ms / 1000).toFixed(1)} s` : ms < 120000 ? `${Math.round(ms / 1000)} s` : `${Math.floor(ms / 60000)} min ${Math.round(ms % 60000 / 1000)} s`;
const money = usd => usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(3)}`;
const mib = n => n >= 1024 && n % 1024 === 0 ? `${n / 1024} GiB` : `${n} MiB`;

function el(tag, { dataset, ...props } = {}, ...children) {
  const node = Object.assign(document.createElement(tag), props);
  if (dataset) Object.assign(node.dataset, dataset);
  node.append(...children.filter(c => c !== null && c !== undefined && c !== false));
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

// Agent ids are for the evidence export; the timeline names the role instead.
function readable(message) {
  const match = /^Agent [0-9a-f-]{36} at depth (\d+) requested an allowed tool$/.exec(message || '');
  if (!match) return message;
  return match[1] === '0' ? 'The orchestrator used one of its tools.' : 'A delegated agent used one of its tools.';
}

function cost(receipt) {
  const calls = Array.isArray(receipt?.inference_calls) ? receipt.inference_calls : [];
  return { calls: calls.length, usd: calls.reduce((total, call) => total + (Number(call.accounted_usd) || 0), 0) };
}

function elapsed(job) {
  const events = job.events || [];
  return events.length < 2 ? null : new Date(events[events.length - 1].at) - new Date(events[0].at);
}

function stateNode(state) {
  const tone = state === 'succeeded' || state === 'failed' ? state : ['queued', 'cancelled'].includes(state) ? '' : 'running';
  return el('span', { className: `state ${tone}`.trim(), textContent: label(STATE_LABEL, state) });
}

function updateDeployment() {
  const s = ui.status;
  if (!s) return;
  $('#worker-signal').className = `signal ${s.worker_connected ? 'online' : 'offline'}`;
  $('#deploy-status').textContent = s.worker_connected ? 'Worker online' : 'Worker unavailable';
  $('#deploy-label').textContent = s.inference_provider ? label(PROVIDER_LABEL, s.inference_provider) : 'Inference not reported';
}

function updateSession() {
  const signedIn = Boolean(ui.csrf);
  $('#login-button').textContent = signedIn ? 'Sign out' : 'Sign in';
  $('#prompt').disabled = !signedIn;
  $('#run-button').disabled = !signedIn;
  if (!signedIn) { ui.jobs = []; ui.turns.clear(); $('#turns').replaceChildren(); }
  updateReadiness();
  render();
}

function updateReadiness() {
  const ready = ui.status?.inference_configured && ui.status?.worker_connected;
  $('#readiness').textContent = !ui.csrf
    ? 'Sign in as the operator to run tasks.'
    : ready
      ? 'Each task gets a fresh sandbox with no network. It starts at 256 MiB and grows only as the code needs, up to the ceiling the agent sets.'
      : 'The worker or inference is unavailable. New tasks wait in the queue until both are back.';
}

function render() {
  const signedIn = Boolean(ui.csrf);
  const empty = ui.jobs.length === 0;
  $('#welcome').hidden = signedIn && !empty;
  $('#welcome-login').hidden = signedIn;
  $('#welcome-title').textContent = signedIn ? 'What should the agent run?' : 'Run AI-written code in a sealed box that is destroyed after every run.';
  if (!signedIn) return;
  const thread = $('#thread');
  const nearBottom = thread.scrollHeight - thread.scrollTop - thread.clientHeight < 160;
  let added = false;
  // The list arrives newest first; a conversation reads oldest first.
  const ordered = [...ui.jobs].reverse();
  const container = $('#turns');
  for (const job of ordered) {
    let turn = ui.turns.get(job.id);
    if (!turn) {
      turn = el('article', { className: 'turn', dataset: { id: job.id } });
      ui.turns.set(job.id, turn);
      added = true;
      // Runs that were already finished on first load do not replay their stamp.
      if (!ui.loaded && terminal(job.state)) ui.stamped.add(job.id);
    }
    const signature = `${job.state}:${job.events?.length}:${job.receipt?.cleanup_state}`;
    if (turn.dataset.signature !== signature) {
      turn.dataset.signature = signature;
      turn.replaceChildren(...renderTurn(job));
    }
    container.append(turn);
  }
  for (const [id, turn] of ui.turns) {
    if (!ui.jobs.some(j => j.id === id)) { turn.remove(); ui.turns.delete(id); }
  }
  ui.loaded = true;
  if (added || nearBottom) thread.scrollTop = thread.scrollHeight;
}

function renderTurn(job) {
  const receipt = job.receipt;
  const you = el('p', { className: 'you', textContent: job.request.prompt });
  const agent = el('div', { className: 'agent' });
  const last = job.events?.[job.events.length - 1];
  if (!terminal(job.state)) {
    const now = last?.kind === 'limits' ? last.message : label(NOW_LABEL, last?.kind);
    const stop = el('button', { className: 'link', type: 'button', textContent: 'Stop' });
    stop.addEventListener('click', () => cancel(job.id, stop));
    agent.append(el('div', { className: 'status' }, stateNode(job.state), el('span', { className: 'now', textContent: now }), stop));
  } else {
    agent.append(el('div', { className: 'status' }, stateNode(job.state)));
  }
  if (receipt?.explanation) agent.append(el('p', { className: 'answer', textContent: receipt.explanation }));
  else if (terminal(job.state) && job.state !== 'cancelled') {
    agent.append(el('p', { className: 'answer', textContent: last?.kind === 'error' ? readable(last.message) : 'The agent did not write an answer.' }));
  }
  const meter = renderMeter(receipt?.scaling);
  if (meter) agent.append(meter);
  if (receipt) agent.append(renderFacts(job));
  if (receipt?.cleanup_state === 'verified') {
    const stamp = el('p', { className: 'destroyed' }, el('strong', { textContent: 'Destroyed' }), el('span', { textContent: 'Sandbox cleanup verified' }));
    if (!ui.stamped.has(job.id) && terminal(job.state)) { ui.stamped.add(job.id); stamp.classList.add('land'); }
    agent.append(stamp);
  }
  agent.append(renderDetails(job));
  return [you, agent];
}

// Start, live growth and peak use of the last sandbox run, against the agent's ceiling.
function renderMeter(scaling) {
  const start = scaling?.start?.memory_mb, grown = scaling?.final?.memory_mb, ceiling = scaling?.ceiling?.memory_mb;
  if (!start || !grown || !ceiling) return null;
  const peak = Number(scaling.peak_memory_mb) || 0;
  const pct = n => `${Math.min(100, (n / ceiling) * 100).toFixed(1)}%`;
  // Sizes go through the CSSOM: the content security policy allows no inline style attributes.
  const part = (className, property, value) => { const span = el('span', { className }); span.style.setProperty(property, value); return span; };
  const bar = el('div', { className: 'meter-bar', role: 'img', ariaLabel: `Memory: started at ${mib(start)}, limit ${mib(grown)}, peak ${mib(peak)}, ceiling ${mib(ceiling)}` },
    part('meter-limit', 'width', pct(grown)),
    part('meter-peak', 'width', pct(peak)),
    part('meter-start', 'left', `calc(${pct(start)} - 1px)`));
  const text = el('p', { className: 'meter-text' });
  if (grown > start) {
    text.append('Memory started at ', el('strong', { textContent: mib(start) }), ' and grew live to ', el('strong', { textContent: mib(grown) }), ` of a ${mib(ceiling)} ceiling.`);
  } else {
    text.append('Memory stayed at ', el('strong', { textContent: mib(start) }), ` of a ${mib(ceiling)} ceiling.`);
  }
  if (peak) text.append(` Peak use ${mib(peak)}.`);
  return el('div', { className: 'meter' }, bar, text);
}

function renderFacts(job) {
  const receipt = job.receipt;
  const spent = cost(receipt);
  const total = elapsed(job);
  const fact = (name, value, tone = '') => el('li', { className: tone }, `${name} `, el('b', { textContent: value }));
  const facts = el('ul', { className: 'facts' });
  if (receipt.reason) facts.append(fact('Outcome', label(REASON_LABEL, receipt.reason), receipt.reason === 'completed' ? 'good' : 'bad'));
  if (terminal(job.state) && total != null) facts.append(fact('Time', seconds(total)));
  if (typeof receipt.duration_ms === 'number') facts.append(fact('In the sandbox', seconds(receipt.duration_ms)));
  if (spent.calls) facts.append(fact('Model cost', `${money(spent.usd)} over ${spent.calls} calls`));
  if (receipt.review) {
    const approved = receipt.review.approved;
    facts.append(fact('Review', approved === true ? 'Approved' : approved === false ? 'Rejected' : 'Not available', approved === true ? 'good' : approved === false ? 'bad' : ''));
  }
  return facts;
}

function renderDetails(job) {
  const receipt = job.receipt;
  const id = encodeURIComponent(job.id);
  const details = el('details', { className: 'more', open: ui.open.has(job.id) });
  details.addEventListener('toggle', () => details.open ? ui.open.add(job.id) : ui.open.delete(job.id));
  const body = el('div', { className: 'more-body' });
  body.append(el('div', { className: 'links' },
    el('a', { className: 'button small', href: `/api/jobs/${id}/evidence`, download: '', textContent: 'Export evidence' }),
    el('a', { className: 'button small', href: `/api/jobs/${id}/contexts`, download: '', textContent: 'Agent contexts' })));
  const timeline = el('ol', { className: 'timeline' });
  for (const event of job.events || []) {
    const tone = ['failed', 'error'].includes(event.kind) ? 'bad' : '';
    timeline.append(el('li', { className: tone },
      el('time', { dateTime: event.at, textContent: new Date(event.at).toLocaleTimeString(undefined, { hour12: false }) }),
      el('span', {}, el('span', { className: 'kind', textContent: label(EVENT_LABEL, event.kind) }), el('span', { className: 'detail', textContent: readable(event.message) }))));
  }
  body.append(el('h3', { textContent: 'Timeline' }), timeline);
  if (receipt) {
    body.append(el('h3', { textContent: 'Program output' }), el('pre', { textContent: [receipt.stdout, receipt.stderr].filter(Boolean).join('\n') || 'The program printed nothing.' }));
    const files = (receipt.artifacts || []).map(a => el('div', { className: 'file' },
      el('a', { href: `/api/jobs/${id}/artifacts/${encodeURIComponent(a.name)}`, download: a.name, textContent: a.name }),
      el('code', { textContent: `${a.bytes.toLocaleString()} bytes, SHA-256 ${a.sha256}` })));
    body.append(el('h3', { textContent: 'Files the run produced' }), ...(files.length ? files : [el('p', { className: 'meter-text', textContent: 'This run produced no files.' })]));
  }
  details.append(el('summary', { textContent: 'Run details and evidence' }), body);
  return details;
}

async function refresh() {
  if (ui.refreshing) return;
  ui.refreshing = true;
  try {
    ui.status = await api('/api/status');
    updateDeployment();
    updateReadiness();
    if (ui.csrf) { ui.jobs = await api('/api/jobs'); render(); }
  } catch (error) {
    toast(error.message);
  } finally {
    ui.refreshing = false;
  }
}

async function cancel(id, button) {
  button.disabled = true;
  try {
    await api(`/api/jobs/${encodeURIComponent(id)}/cancel`, { method: 'POST', body: 'null' });
    await refresh();
    toast('Stopped. The sandbox memory stays reserved until cleanup is confirmed.');
  } catch (error) {
    toast(error.message);
    button.disabled = false;
  }
}

function openLogin() {
  $('#login-dialog').showModal();
  $('#password').focus();
}

function resize() {
  const box = $('#prompt');
  box.style.height = 'auto';
  box.style.height = `${Math.min(box.scrollHeight, 220)}px`;
}

async function send() {
  if (!ui.csrf) { openLogin(); return; }
  const prompt = $('#prompt').value.trim();
  if (prompt.length < 3) { toast('Describe the task in a few words first.'); $('#prompt').focus(); return; }
  const button = $('#run-button');
  button.disabled = true;
  try {
    await api('/api/jobs', { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ prompt, profile: 'cli' }) });
    $('#prompt').value = '';
    resize();
    await refresh();
  } catch (error) {
    toast(error.message);
  } finally {
    button.disabled = !ui.csrf;
  }
}

$('#task-form').addEventListener('submit', event => { event.preventDefault(); send(); });
$('#prompt').addEventListener('input', resize);
$('#prompt').addEventListener('keydown', event => {
  if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); send(); }
});

$('#login-button').addEventListener('click', async () => {
  if (!ui.csrf) { openLogin(); return; }
  try {
    await api('/api/logout', { method: 'POST', body: 'null' });
    ui.csrf = null;
    updateSession();
    toast('Signed out.');
  } catch (error) {
    toast(error.message);
  }
});
$('#welcome-login').addEventListener('click', openLogin);

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
    $('#prompt').focus();
    toast('Signed in as the operator.');
  } catch (error) {
    $('#login-error').textContent = error.message;
  } finally {
    button.disabled = false;
  }
});

for (const chip of document.querySelectorAll('[data-example]')) {
  chip.addEventListener('click', () => {
    if (!ui.csrf) { openLogin(); return; }
    $('#prompt').value = EXAMPLES[chip.dataset.example];
    resize();
    $('#prompt').focus();
  });
}
$('#safety-button').addEventListener('click', () => $('#safety-dialog').showModal());
document.querySelectorAll('[data-close]').forEach(button => button.addEventListener('click', () => document.getElementById(button.dataset.close).close()));

(async () => {
  try {
    const response = await fetch('/api/session', { credentials: 'same-origin' });
    if (response.ok) ui.csrf = (await response.json()).csrf;
  } catch { /* signed out */ }
  updateSession();
  await refresh();
  setInterval(() => { if (!document.hidden) refresh(); }, 2500);
})();
