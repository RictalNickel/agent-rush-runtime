'use strict';
// The web chat is a client of agent sessions (/v1). One thread is one session.
const $ = s => document.querySelector(s);
const ui = {
  csrf: null, status: null, v1: null, sessions: [], current: null,
  lastSeq: 0, replayUntil: 0, source: null, sseErrors: 0, poll: null, pollBusy: false,
  pending: [], urls: [], speak: false, audio: null, recorder: null, sending: false,
  filesPath: '.', refreshing: false,
};
const KINDS = ['user', 'assistant', 'tool_call', 'tool_result', 'interjection', 'status', 'config_change', 'error'];
const MAX_OUTPUT = 8000;

const EXAMPLES = {
  webapp: 'Create a small Flask to-do app in /workspace/todo with one HTML page. Start it, use the browser to add and complete an item, and attach a screenshot of the result.',
  report: 'Research which browsers support WebAssembly garbage collection today. Write a two-page report in Markdown with your sources, convert it to report.pdf and attach it.',
  csv: 'Generate a CSV of 1,000 seeded sales rows across four regions and the twelve months of 2025. Compute revenue by quarter and region with Python and save a bar chart as revenue.png.',
  bug: 'Create a small Python package with a function that has an off-by-one bug and a pytest suite that catches it. Run the tests, fix the bug and show the passing run.',
};
const PROVIDER_LABEL = { vultr: 'Vultr Serverless Inference', anthropic: 'Anthropic Claude' };
const STATE_LABEL = { idle: 'Ready', running: 'Working', failed: 'Failed', closed: 'Closed' };
const FAILED_LABEL = { time_limit: 'The turn reached its time limit.', step_limit: 'The turn reached its step limit.' };

// 24 px stroke icons, one per tool family.
const ICONS = {
  terminal: 'M4 6l5 5-5 5M11.5 18H20',
  file: 'M14 3H6v18h12V7zM14 3v4h4',
  edit: 'M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4',
  folder: 'M3 6h6l2 2h10v11H3z',
  search: 'M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM15.5 15.5L21 21',
  globe: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c3.2 3.6 3.2 14.4 0 18M12 3c-3.2 3.6-3.2 14.4 0 18',
  camera: 'M4 8h3l2-3h6l2 3h3v11H4zM12 10.5a3 3 0 1 0 0 6 3 3 0 0 0 0-6z',
  convert: 'M4 8h13l-4-4M20 16H7l4 4',
  book: 'M5 4h9a4 4 0 0 1 4 4v12H9a4 4 0 0 1-4-4zM5 16a4 4 0 0 1 4-4h9',
  plug: 'M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4',
  gear: 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1',
  alert: 'M12 3.5L21.5 20h-19zM12 10v4.5M12 17.2v.3',
  tool: 'M14.5 4.5a4 4 0 0 0-5 5L4 15l5 5 5.5-5.5a4 4 0 0 0 5-5l-2.5 2.5-3-3z',
};
const TOOL_ICON = {
  bash: 'terminal', read_file: 'file', write_file: 'edit', edit_file: 'edit', list_files: 'folder', grep: 'search', glob: 'folder',
  browser: 'globe', web_search: 'search', web_fetch: 'globe', convert_file: 'convert', skill: 'book', mcp: 'plug', agent_config: 'gear',
};

const enc = encodeURIComponent;
const clip = (s, n) => { const t = String(s ?? ''); return t.length > n ? `${t.slice(0, n - 1)}…` : t; };
const oneLine = s => String(s ?? '').replace(/\s+/g, ' ').trim();
const host = url => { try { return new URL(url).host || url; } catch { return url || 'a page'; } };
const baseName = path => String(path).split('/').filter(Boolean).pop() || 'file';
const filesUrl = (id, path) => `/v1/sessions/${enc(id)}/files?path=${enc(path)}`;
const isImage = path => /\.(png|jpe?g|gif|webp)$/i.test(path);
const bytes = n => n < 1024 ? `${n} bytes` : n < 1048576 ? `${(n / 1024).toFixed(n < 10240 ? 1 : 0)} KB` : `${(n / 1048576).toFixed(1)} MB`;

function el(tag, { dataset, ...props } = {}, ...children) {
  const node = Object.assign(document.createElement(tag), props);
  if (dataset) Object.assign(node.dataset, dataset);
  node.append(...children.filter(c => c !== null && c !== undefined && c !== false));
  return node;
}

function icon(name) {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  svg.classList.add('ico');
  const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  path.setAttribute('d', ICONS[name] || ICONS.tool);
  svg.append(path);
  return svg;
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

function authHeaders(extra = {}) {
  return { ...(ui.csrf ? { 'X-CSRF-Token': ui.csrf } : {}), ...extra };
}

async function failure(response) {
  const value = await response.json().catch(() => ({}));
  if (response.status === 401) { ui.csrf = null; updateSession(); }
  return new Error(value.error || value.detail || `Request failed with status ${response.status}.`);
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    credentials: 'same-origin', ...options,
    headers: authHeaders({ 'Content-Type': 'application/json', ...options.headers }),
  });
  if (!response.ok) throw await failure(response);
  return response.json().catch(() => { throw new Error('The server sent an unexpected response.'); });
}

async function fetchBlob(path, options = {}) {
  const response = await fetch(path, { credentials: 'same-origin', ...options, headers: authHeaders(options.headers) });
  if (!response.ok) throw await failure(response);
  return response.blob();
}

function objectUrl(blob) {
  const url = URL.createObjectURL(blob);
  ui.urls.push(url);
  return url;
}

// Bar and readiness.

function updateDeployment() {
  const s = ui.status;
  if (!s) return;
  $('#worker-signal').className = `signal ${s.worker_connected ? 'online' : 'offline'}`;
  $('#deploy-status').textContent = s.worker_connected ? 'Worker online' : 'Worker unavailable';
  const provider = s.inference_provider;
  $('#deploy-label').textContent = provider ? PROVIDER_LABEL[provider] || provider : 'Inference not reported';
}

function updateReadiness() {
  const note = $('#readiness');
  if (!ui.csrf) note.textContent = 'Sign in as the operator to start a session.';
  else if (ui.v1 && !ui.v1.sessions_available) note.textContent = 'Sessions need a connected worker. Ask the operator to connect one.';
  else if (ui.current?.status === 'running') note.textContent = 'The agent is working. A message now reaches it before its next step.';
  else if (ui.current?.status === 'closed') note.textContent = 'This session is closed. A new message starts a new session.';
  else note.textContent = 'Enter sends and Shift+Enter adds a line. The workspace keeps its files between messages.';
}

function updateSession() {
  const signedIn = Boolean(ui.csrf);
  $('#login-button').textContent = signedIn ? 'Sign out' : 'Sign in';
  $('#prompt').disabled = !signedIn;
  $('#run-button').disabled = !signedIn;
  if (!signedIn) {
    stopLive();
    ui.v1 = null; ui.sessions = []; ui.current = null;
    revokeUrls();
    $('#log').replaceChildren();
  }
  updateVoice();
  renderSessions();
  updateChrome();
}

function updateVoice() {
  const voice = Boolean(ui.csrf && ui.v1?.voice_available);
  const canRecord = voice && typeof MediaRecorder === 'function' && Boolean(navigator.mediaDevices?.getUserMedia);
  $('#mic-button').hidden = !canRecord;
  $('#speaker-button').hidden = !voice;
  if (!voice) setSpeak(false);
}

function updateChrome() {
  const signedIn = Boolean(ui.csrf);
  const s = ui.current;
  $('#welcome').hidden = Boolean(s);
  $('#welcome-login').hidden = signedIn;
  $('#welcome-title').textContent = signedIn ? 'What should the agent work on?' : 'Give an agent its own sealed Linux workspace.';
  $('#chat-head').hidden = !s;
  const running = s?.status === 'running';
  if (s) {
    $('#session-title').textContent = s.title || 'New session';
    const state = $('#session-state');
    state.className = `state ${s.status}`;
    state.textContent = STATE_LABEL[s.status] || s.status;
  }
  $('#stop-button').hidden = !running;
  $('#working').hidden = !running;
  updateReadiness();
}

// Session drawer.

function renderSessions() {
  const note = $('#drawer-note');
  note.hidden = signedInWithSessions();
  note.textContent = !ui.csrf ? 'Sign in to see your sessions.'
    : ui.v1 && !ui.v1.sessions_available ? 'Sessions are unavailable until a worker is connected.'
      : 'No sessions yet. Your first message starts one.';
  const list = $('#session-list');
  // The 5 s refresh must not rebuild an unchanged list: that would drop keyboard focus.
  const signature = JSON.stringify([ui.current?.id, ui.sessions.map(s => [s.id, s.title, s.status])]);
  if (list.dataset.signature === signature) return;
  list.dataset.signature = signature;
  list.replaceChildren(...ui.sessions.map(s => {
    const button = el('button', { type: 'button', className: 'session' },
      el('span', { className: `dot ${s.status}`, ariaHidden: 'true' }),
      el('span', { className: 'session-title', textContent: s.title || 'New session' }),
      el('span', { className: 'sr-only', textContent: `, ${STATE_LABEL[s.status] || s.status}` }));
    if (s.id === ui.current?.id) button.setAttribute('aria-current', 'true');
    button.addEventListener('click', () => {
      // Read the latest copy: its last_seq decides which replies are history (never spoken).
      if (s.id !== ui.current?.id) openSession(ui.sessions.find(x => x.id === s.id) || s);
      closeDrawer();
    });
    return el('li', {}, button);
  }));
}
const signedInWithSessions = () => Boolean(ui.csrf && ui.sessions.length);

function upsertSession(session) {
  const i = ui.sessions.findIndex(s => s.id === session.id);
  if (i >= 0) ui.sessions[i] = { ...ui.sessions[i], ...session };
  else ui.sessions.unshift(session);
}

function openDrawer() {
  $('#drawer').classList.add('open');
  $('#drawer-button').setAttribute('aria-expanded', 'true');
  $('#new-session').focus();
}
function closeDrawer() {
  if (!$('#drawer').classList.contains('open')) return;
  $('#drawer').classList.remove('open');
  $('#drawer-button').setAttribute('aria-expanded', 'false');
}

function newSession() {
  stopLive();
  ui.current = null; ui.lastSeq = 0; ui.pending = [];
  revokeUrls();
  $('#log').replaceChildren();
  renderSessions();
  updateChrome();
  closeDrawer();
  $('#prompt').focus();
}

function openSession(session, { fresh = false } = {}) {
  stopLive();
  ui.current = { ...session };
  ui.lastSeq = 0;
  ui.replayUntil = fresh ? 0 : session.last_seq || 0;
  ui.pending = [];
  revokeUrls();
  $('#log').replaceChildren();
  renderSessions();
  updateChrome();
  startLive();
}

function revokeUrls() {
  for (const url of ui.urls) URL.revokeObjectURL(url);
  ui.urls = [];
}

// Live updates: SSE first, polling the event log if the stream fails.

function stopLive() {
  if (ui.source) { ui.source.close(); ui.source = null; }
  clearTimeout(ui.poll);
  ui.poll = null;
}

function startLive() {
  const session = ui.current;
  if (!session) return;
  // A closed session never changes: read its log once.
  if (session.status === 'closed') { fetchEvents(session.id).catch(error => toast(error.message)); return; }
  if (typeof EventSource !== 'function') { startPolling(); return; }
  const source = new EventSource(`/v1/sessions/${enc(session.id)}/stream?after=${ui.lastSeq}`);
  ui.source = source;
  ui.sseErrors = 0;
  const onEvent = event => {
    if (ui.source !== source || !(event instanceof MessageEvent) || !event.data) return;
    ui.sseErrors = 0;
    let parsed;
    try { parsed = JSON.parse(event.data); } catch { return; }
    ingest(parsed);
    if (ui.current?.status === 'closed') stopLive();
  };
  // Each SSE message is named after its event kind; "error" doubles as the connection error.
  for (const kind of KINDS) source.addEventListener(kind, onEvent);
  source.addEventListener('open', () => { if (ui.source === source) ui.sseErrors = 0; });
  source.addEventListener('error', event => {
    if (ui.source !== source || event instanceof MessageEvent) return;
    ui.sseErrors += 1;
    // EventSource reconnects by itself and resumes with Last-Event-ID (the last seq).
    // A refused stream, or three failed reconnects in a row, falls back to polling.
    if (source.readyState === EventSource.CLOSED || ui.sseErrors >= 3) {
      source.close();
      ui.source = null;
      startPolling();
    }
  });
}

function startPolling() {
  clearTimeout(ui.poll);
  const id = ui.current?.id;
  const tick = async () => {
    if (ui.current?.id !== id) return;
    if (!document.hidden && !ui.pollBusy) {
      ui.pollBusy = true;
      try { await fetchEvents(id); } catch { /* retried on the next tick */ } finally { ui.pollBusy = false; }
    }
    if (ui.current?.id === id && ui.current.status !== 'closed' && !ui.source) ui.poll = setTimeout(tick, 2000);
  };
  tick();
}

async function fetchEvents(id) {
  for (let page = 0; page < 50; page += 1) {
    const result = await api(`/v1/sessions/${enc(id)}/events?after=${ui.lastSeq}&limit=500`);
    if (ui.current?.id !== id) return;
    result.events.forEach(ingest);
    if (!result.more || !result.events.length) return;
  }
}

function ingest(event) {
  if (!ui.current || (event.session_id && event.session_id !== ui.current.id)) return;
  if (typeof event.seq !== 'number' || event.seq <= ui.lastSeq) return;
  ui.lastSeq = event.seq;
  const thread = $('#thread');
  const nearBottom = thread.scrollHeight - thread.scrollTop - thread.clientHeight < 200;
  renderEvent(event, event.seq > ui.replayUntil);
  if (nearBottom) thread.scrollTop = thread.scrollHeight;
}

// Rendering events.

function renderEvent(event, live) {
  const p = event.payload || {};
  const log = $('#log');
  switch (event.kind) {
    case 'user':
      log.append(el('li', { className: 'msg you' }, el('p', { textContent: p.text })));
      break;
    case 'interjection':
      log.append(el('li', { className: 'msg you interjection' },
        el('p', { textContent: p.text }), el('span', { className: 'tag', textContent: 'Sent while working' })));
      break;
    case 'tool_call': {
      const row = toolRow(p);
      ui.pending.push(row);
      log.append(row.node);
      break;
    }
    case 'tool_result': {
      let row = ui.pending.shift();
      if (!row) { row = toolRow({ tool: p.tool, call: null }); log.append(row.node); }
      finishRow(row, p);
      break;
    }
    case 'assistant':
      log.append(renderAnswer(p));
      if (live && ui.speak && p.text) speak(p.text);
      break;
    case 'status':
      setStatus(p.status);
      if (p.status !== 'running') settlePending();
      if (p.status === 'idle' && p.reason === 'cancelled') log.append(note('Stopped at your request.'));
      if (p.status === 'failed' && FAILED_LABEL[p.reason]) log.append(note(FAILED_LABEL[p.reason], 'bad'));
      if (p.status === 'closed') log.append(note('The session was closed and its workspace deleted.'));
      break;
    case 'error':
      log.append(note(p.message || 'Something went wrong.', 'bad'));
      break;
    case 'config_change':
      log.append(note(`The agent changed its own config (version ${p.version})${p.reason ? `: ${p.reason}` : '.'}`));
      break;
    default:
      break;
  }
}

function note(text, tone = '') {
  return el('li', { className: `note ${tone}`.trim(), textContent: text });
}

function setStatus(status) {
  if (!ui.current || !status) return;
  ui.current.status = status;
  upsertSession({ id: ui.current.id, status });
  renderSessions();
  updateChrome();
}

// Rows still waiting when a turn ends did not get a result.
function settlePending() {
  for (const row of ui.pending.splice(0)) {
    row.node.className = 'step bad';
    row.text.textContent = row.done;
    row.result.textContent = ' — did not finish';
  }
}

function describe(tool, call) {
  const c = call && typeof call === 'object' ? call : {};
  const q = s => `“${clip(oneLine(s), 60)}”`;
  const both = (now, past, rest) => [`${now} ${rest}`.trim(), `${past} ${rest}`.trim()];
  switch (tool) {
    case 'bash': return both('Running', 'Ran', clip(oneLine(c.command), 120));
    case 'read_file': return both('Reading', 'Read', c.path);
    case 'write_file': return both('Writing', 'Wrote', c.path);
    case 'edit_file': return both('Editing', 'Edited', c.path);
    case 'list_files': return both('Listing', 'Listed', c.path && c.path !== '.' ? c.path : 'the workspace');
    case 'grep': return both('Searching', 'Searched', `the files for ${q(c.pattern)}`);
    case 'glob': return both('Finding', 'Found', `files matching ${c.pattern}`);
    case 'web_search': return both('Searching', 'Searched', `the web for ${q(c.query)}`);
    case 'web_fetch': return both('Fetching', 'Fetched', host(c.url));
    case 'convert_file': return both('Converting', 'Converted', `${c.input} to ${c.output}`);
    case 'browser': return describeBrowser(c);
    case 'skill':
      if (c.action === 'list') return both('Listing', 'Listed', 'skills');
      return both({ save: 'Saving', delete: 'Deleting' }[c.action] || 'Loading', { save: 'Saved', delete: 'Deleted' }[c.action] || 'Loaded', `the skill ${c.name || ''}`);
    case 'agent_config':
      if (c.action === 'update') return both('Changing', 'Changed', 'its own config');
      if (c.action === 'revert') return both('Reverting', 'Reverted', `its config to version ${c.version}`);
      return both('Reading', 'Read', c.action === 'history' ? 'its config history' : 'its config');
    case 'mcp':
      if (c.action === 'call') return both('Calling', 'Called', `${c.server} ${c.name || ''}`);
      return both('Listing', 'Listed', `tools on ${c.server}`);
    case null: case undefined: case '':
      return ['Reading the tool call', 'Sent a tool call that could not be read'];
    default: return both('Running', 'Ran', `the ${tool} tool`);
  }
}

function describeBrowser(c) {
  const target = c.ref ? `element ${c.ref}` : c.selector ? clip(c.selector, 60) : 'the page';
  const table = {
    goto: ['Opening', 'Opened', host(c.url)], snapshot: ['Reading', 'Read', 'the page'],
    click: ['Clicking', 'Clicked', target], type: ['Typing into', 'Typed into', target],
    press: ['Pressing', 'Pressed', c.key || 'a key'], select: ['Choosing', 'Chose', `an option in ${target}`],
    scroll: ['Scrolling', 'Scrolled', 'the page'], screenshot: ['Taking', 'Took', 'a screenshot'],
    eval: ['Running', 'Ran', 'a script in the page'], back: ['Going', 'Went', 'back'],
    tabs: ['Listing', 'Listed', 'the tabs'], console: ['Reading', 'Read', 'the console'], close: ['Closing', 'Closed', 'the browser'],
  };
  const [now, past, rest] = table[c.action] || ['Using', 'Used', 'the browser'];
  return [`${now} ${rest}`, `${past} ${rest}`];
}

function toolRow(p) {
  const tool = p.tool ?? null;
  const call = p.call || {};
  const [now, done] = describe(tool, call);
  const text = el('span', { className: 'step-text', textContent: now });
  const iconName = tool === null ? 'alert' : tool === 'browser' && call.action === 'screenshot' ? 'camera' : TOOL_ICON[tool] || 'tool';
  const result = el('span', { className: 'step-result' });
  const summary = el('summary', {}, icon(iconName), text, result);
  const details = el('details', {}, summary);
  const node = el('li', { className: 'step pending' }, details);
  return { node, details, text, result, tool, call, done };
}

function finishRow(row, p) {
  const output = String(p.output ?? '');
  let suffix = p.ok === false ? ' — failed' : '';
  let ok = p.ok !== false;
  if (row.tool === 'bash') {
    const exit = /^exit code (-?\d+)/.exec(output);
    if (exit) { suffix = ` — exit ${exit[1]}`; ok = ok && exit[1] === '0'; }
    if (/^exit code -?\d+ \(timed out/.test(output)) suffix += ', timed out';
  }
  row.node.className = `step ${ok ? 'ok' : 'bad'}`;
  row.text.textContent = row.done;
  row.result.textContent = suffix;
  const shown = output.length > MAX_OUTPUT ? output.slice(0, MAX_OUTPUT) : output;
  const command = row.tool === 'bash' && row.call.command ? `$ ${row.call.command}\n` : '';
  const body = el('div', { className: 'step-body' }, el('pre', { textContent: `${command}${shown}` || 'No output.' }));
  if (output.length > MAX_OUTPUT || p.truncated) {
    body.append(el('p', { className: 'step-note', textContent: output.length > MAX_OUTPUT ? `Showing the first ${MAX_OUTPUT.toLocaleString()} characters.` : 'The agent saw a shortened copy of this output.' }));
  }
  row.details.append(body);
  // Browser screenshots land under /workspace; show them inline through the files API.
  if (row.tool === 'browser') {
    const match = /(\/workspace\/[^\s"'\\]+\.png)/.exec(output);
    if (match) row.node.append(picture(match[1]));
  }
}

function picture(path) {
  const img = el('img', { alt: `Screenshot ${baseName(path)}`, decoding: 'async' });
  const figure = el('figure', { className: 'shot' }, img, el('figcaption', { textContent: path }));
  const id = ui.current?.id;
  fetchBlob(filesUrl(id, path)).then(blob => {
    if (ui.current?.id === id) img.src = objectUrl(blob);
  }).catch(() => {
    figure.replaceChildren(el('p', { className: 'step-note', textContent: `The image ${path} could not be loaded.` }));
  });
  return figure;
}

function fileLink(path, label = baseName(path)) {
  return el('a', { className: 'file-link', href: filesUrl(ui.current.id, path), download: baseName(path), textContent: label });
}

function renderAnswer(p) {
  const item = el('li', { className: 'msg agent' }, el('p', { className: 'answer', textContent: p.text || 'The agent replied without text.' }));
  const attachments = Array.isArray(p.attachments) ? p.attachments.filter(a => typeof a === 'string' && a) : [];
  if (attachments.length) {
    item.append(el('ul', { className: 'attachments', ariaLabel: 'Attached files' },
      ...attachments.map(path => el('li', {}, fileLink(path)))));
    for (const path of attachments.filter(isImage)) item.append(picture(path));
  }
  return item;
}

// Sending, interjecting and stopping.

function resize() {
  const box = $('#prompt');
  box.style.setProperty('height', 'auto');
  box.style.setProperty('height', `${Math.min(box.scrollHeight, 220)}px`);
}

async function send() {
  if (!ui.csrf) { openLogin(); return; }
  const text = $('#prompt').value.trim();
  if (!text) { $('#prompt').focus(); return; }
  if (ui.sending) return;
  ui.sending = true;
  try {
    if (!ui.current || ui.current.status === 'closed') {
      const created = await api('/v1/sessions', { method: 'POST', body: JSON.stringify({ message: text }) });
      upsertSession(created.session);
      openSession(created.session, { fresh: true });
    } else {
      await api(`/v1/sessions/${enc(ui.current.id)}/messages`, { method: 'POST', body: JSON.stringify({ text }) });
      if (!ui.source && !ui.pollBusy) fetchEvents(ui.current.id).catch(() => {});
    }
    $('#prompt').value = '';
    resize();
  } catch (error) {
    toast(error.message);
  } finally {
    ui.sending = false;
  }
}

async function stopTurn() {
  if (!ui.current) return;
  const button = $('#stop-button');
  button.disabled = true;
  try {
    await api(`/v1/sessions/${enc(ui.current.id)}/cancel`, { method: 'POST', body: '{}' });
    toast('Stopping after the current step.');
  } catch (error) {
    toast(error.message);
  } finally {
    button.disabled = false;
  }
}

// Files panel.

async function showFiles(path = '.') {
  if (!ui.current) return;
  const dialog = $('#files-dialog');
  if (!dialog.open) dialog.showModal();
  ui.filesPath = path;
  $('#files-path').textContent = path === '.' ? '/workspace' : `/workspace/${path}`;
  $('#files-up').hidden = path === '.';
  const list = $('#files-list');
  list.replaceChildren(el('li', { className: 'files-note', textContent: 'Loading' }));
  try {
    const listing = await api(filesUrl(ui.current.id, path));
    const entries = [...(listing.entries || [])].sort((a, b) => (b.is_dir - a.is_dir) || a.name.localeCompare(b.name));
    const child = name => path === '.' ? name : `${path}/${name}`;
    list.replaceChildren(...entries.map(entry => {
      if (entry.is_dir) {
        const open = el('button', { type: 'button', className: 'entry dir' }, icon('folder'), el('span', { textContent: `${entry.name}/` }));
        open.addEventListener('click', () => showFiles(child(entry.name)));
        return el('li', {}, open);
      }
      const link = fileLink(child(entry.name), entry.name);
      link.className = 'entry';
      link.prepend(icon('file'));
      return el('li', {}, link, el('span', { className: 'size', textContent: bytes(Number(entry.size) || 0) }));
    }));
    if (!entries.length) list.replaceChildren(el('li', { className: 'files-note', textContent: 'This folder is empty.' }));
  } catch (error) {
    list.replaceChildren(el('li', { className: 'files-note bad', textContent: error.message }));
  }
}

// Voice: shown only when /v1/status reports it.

function setSpeak(on) {
  ui.speak = on;
  $('#speaker-button').setAttribute('aria-pressed', String(on));
  if (!on && ui.audio) { ui.audio.pause(); ui.audio = null; }
}

async function speak(text) {
  try {
    const blob = await fetchBlob('/v1/voice/speak', {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ text: clip(text, 1000) }),
    });
    if (!ui.speak) return;
    if (ui.audio) ui.audio.pause();
    ui.audio = new Audio(objectUrl(blob));
    await ui.audio.play();
  } catch (error) {
    toast(`The reply could not be read aloud. ${error.message}`);
  }
}

function setMic(recording) {
  const button = $('#mic-button');
  button.classList.toggle('recording', recording);
  button.setAttribute('aria-pressed', String(recording));
  button.setAttribute('aria-label', recording ? 'Stop recording and send' : 'Record a message');
}

async function toggleMic() {
  if (!ui.csrf) { openLogin(); return; }
  if (ui.recorder) { ui.recorder.stop(); return; }
  let stream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  } catch {
    toast('The microphone is not available. Check the browser permission.');
    return;
  }
  const chunks = [];
  const recorder = new MediaRecorder(stream);
  ui.recorder = recorder;
  recorder.addEventListener('dataavailable', event => { if (event.data.size) chunks.push(event.data); });
  recorder.addEventListener('stop', () => {
    stream.getTracks().forEach(track => track.stop());
    ui.recorder = null;
    setMic(false);
    const audio = new Blob(chunks, { type: recorder.mimeType || 'audio/webm' });
    if (audio.size) transcribe(audio);
  });
  recorder.start();
  setMic(true);
  toast('Recording. Press the microphone again to send.');
}

async function transcribe(audio) {
  const button = $('#mic-button');
  button.disabled = true;
  const form = new FormData();
  form.append('audio', audio, audio.type.includes('ogg') ? 'speech.ogg' : audio.type.includes('mp4') ? 'speech.m4a' : 'speech.webm');
  try {
    const response = await fetch('/v1/voice/transcribe', { method: 'POST', credentials: 'same-origin', headers: authHeaders(), body: form });
    if (!response.ok) throw await failure(response);
    const text = String((await response.json()).text || '').trim();
    if (!text) { toast('No speech was recognised. Try again a little closer to the microphone.'); return; }
    $('#prompt').value = text;
    resize();
    await send();
  } catch (error) {
    toast(error.message);
  } finally {
    button.disabled = false;
  }
}

// Status, sign in and start-up.

async function loadSessions() {
  if (!ui.csrf) return;
  // Voice and session availability rarely change: re-read them only until sessions work.
  if (!ui.v1?.sessions_available) { ui.v1 = await api('/v1/status'); updateVoice(); }
  if (!ui.v1.sessions_available) { ui.sessions = []; renderSessions(); updateChrome(); return; }
  const { sessions } = await api('/v1/sessions');
  // Events are the authority for the open session's status.
  ui.sessions = sessions.map(s => s.id === ui.current?.id ? { ...s, status: ui.current.status } : s);
  if (ui.current) {
    const fresh = ui.sessions.find(s => s.id === ui.current.id);
    if (fresh) ui.current.title = fresh.title;
  }
  renderSessions();
  updateChrome();
}

async function refresh() {
  if (ui.refreshing) return;
  ui.refreshing = true;
  try {
    ui.status = await api('/api/status');
    updateDeployment();
    await loadSessions();
  } catch (error) {
    toast(error.message);
  } finally {
    ui.refreshing = false;
  }
}

function openLogin() {
  $('#login-dialog').showModal();
  $('#password').focus();
}

$('#task-form').addEventListener('submit', event => { event.preventDefault(); send(); });
$('#prompt').addEventListener('input', resize);
$('#prompt').addEventListener('keydown', event => {
  if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); send(); }
});
$('#stop-button').addEventListener('click', stopTurn);
$('#files-button').addEventListener('click', () => showFiles('.'));
$('#files-up').addEventListener('click', () => showFiles(ui.filesPath.split('/').slice(0, -1).join('/') || '.'));
$('#new-session').addEventListener('click', () => { if (!ui.csrf) { openLogin(); return; } newSession(); });
$('#mic-button').addEventListener('click', toggleMic);
$('#speaker-button').addEventListener('click', () => {
  setSpeak(!ui.speak);
  toast(ui.speak ? 'New replies will be read aloud.' : 'Replies will not be read aloud.');
});
$('#drawer-button').addEventListener('click', () => $('#drawer').classList.contains('open') ? closeDrawer() : openDrawer());
document.addEventListener('keydown', event => {
  if (event.key === 'Escape' && $('#drawer').classList.contains('open')) { closeDrawer(); $('#drawer-button').focus(); }
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
  setInterval(() => { if (!document.hidden) refresh(); }, 5000);
})();
